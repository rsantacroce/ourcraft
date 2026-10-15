//! Client-side prediction and server reconciliation.
//!
//! **The problem.** With a server-authoritative game, the server decides
//! where you are. If the client waited for the server before moving, every
//! keypress would feel delayed by the round trip (100 ms or more online).
//!
//! **Prediction.** So the client doesn't wait: every physics tick it runs the
//! same [`physics::step`] the server will run, on the same input, and shows
//! the result right away. It sends the input to the server, numbered with
//! its tick, and remembers (tick, input, predicted state) in a history.
//!
//! **Reconciliation.** The server replies now and then with "after your input
//! #T, you were *here*". Usually that matches the prediction for tick T
//! exactly, because it's the same code and the same input. If it doesn't
//! (someone placed a block in your way, a packet was lost…) the client
//! resets to the server's state at T and quickly *replays* every input after
//! T. The result is where you'd be now given the server's truth. The
//! difference is then smoothed out visually so you don't see a jump.
//!
//! The server side is [`InputBuffer`]: inputs arrive in bursts and
//! sometimes twice (we resend recent ones in case of loss); the buffer keeps
//! them in order and drops duplicates.

use std::collections::VecDeque;

use glam::{Vec2, Vec3};

use crate::block::BlockRegistry;
use crate::physics::{self, PlayerInput, PlayerState};
use crate::protocol::NetInput;
use crate::world::BlockSource;

/// Ticks of history kept (2 seconds at 60 Hz). If the server falls further
/// behind than this, we simply snap to its state.
const HISTORY_LEN: usize = 120;

/// Prediction errors smaller than this (in blocks) are ignored; tiny float
/// differences aren't worth a correction.
pub const TOLERANCE: f32 = 0.01;

impl NetInput {
    /// The physics input this network input represents.
    pub fn physics(&self) -> PlayerInput {
        PlayerInput {
            movement: Vec2::from_array(self.movement),
            jump: self.jump,
            descend: self.descend,
            sprint: self.sprint,
            yaw: self.yaw,
            auto_jump: self.auto_jump,
            speed: self.speed,
        }
    }
}

/// Apply one input to a player: the "fly" switch, then one physics step.
/// Client and server both use exactly this function.
pub fn apply_input(
    state: &mut PlayerState,
    input: &NetInput,
    world: &impl BlockSource,
    registry: &BlockRegistry,
) {
    state.flying = input.fly;
    physics::step(state, &input.physics(), world, registry);
}

#[derive(Clone, Debug)]
struct Entry {
    input: NetInput,
    /// Predicted state *after* applying `input`.
    state: PlayerState,
}

/// The client's record of recent inputs and predictions.
#[derive(Default, Debug)]
pub struct PredictionHistory {
    entries: VecDeque<Entry>,
}

/// What reconciliation did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reconciled {
    /// The prediction was right.
    Matched,
    /// The prediction was off; `error` is how far the corrected current
    /// position is from what we were showing.
    Corrected { error: Vec3 },
    /// The server's tick wasn't in our history; we jumped to its state.
    Snapped,
}

impl PredictionHistory {
    pub fn push(&mut self, input: NetInput, state: PlayerState) {
        self.entries.push_back(Entry { input, state });
        while self.entries.len() > HISTORY_LEN {
            self.entries.pop_front();
        }
    }

    /// The most recent inputs (newest last), for resending.
    pub fn recent_inputs(&self, n: usize) -> Vec<NetInput> {
        let skip = self.entries.len().saturating_sub(n);
        self.entries.iter().skip(skip).map(|e| e.input).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Compare the server's state after input `ack_tick` with our prediction
    /// and fix up `current` (our latest predicted state) if needed.
    pub fn reconcile(
        &mut self,
        ack_tick: u32,
        server: PlayerState,
        current: &mut PlayerState,
        world: &impl BlockSource,
        registry: &BlockRegistry,
    ) -> Reconciled {
        // Inputs up to the acknowledged one are settled; forget them.
        let predicted = loop {
            match self.entries.front() {
                Some(e) if e.input.tick < ack_tick => {
                    self.entries.pop_front();
                }
                Some(e) if e.input.tick == ack_tick => break self.entries.pop_front(),
                _ => break None,
            }
        };
        let Some(predicted) = predicted else {
            // Too old (or from before we started): just trust the server.
            *current = server;
            self.entries.clear();
            return Reconciled::Snapped;
        };

        let close = predicted.state.position.distance(server.position) <= TOLERANCE
            && predicted.state.velocity.distance(server.velocity) <= TOLERANCE * 10.0
            && predicted.state.flying == server.flying;
        if close {
            return Reconciled::Matched;
        }

        // Rewind to the server's truth and replay everything after it.
        let before = current.position;
        let mut state = server;
        for e in self.entries.iter_mut() {
            apply_input(&mut state, &e.input, world, registry);
            e.state = state;
        }
        *current = state;
        Reconciled::Corrected {
            error: before - state.position,
        }
    }
}

/// The server's queue of a player's inputs.
#[derive(Default, Debug)]
pub struct InputBuffer {
    pending: VecDeque<NetInput>,
    /// Newest tick accepted so far.
    last_received: u32,
    /// Newest tick applied so far (the "ack" sent back).
    pub last_applied: u32,
}

impl InputBuffer {
    /// Most inputs waiting at once (1 s). Anything beyond that is a client
    /// sending too fast; extra inputs are dropped.
    const MAX_PENDING: usize = 60;

    /// Add received inputs, ignoring ones we've already seen.
    pub fn receive(&mut self, inputs: &[NetInput]) {
        for input in inputs {
            if input.tick > self.last_received && self.pending.len() < Self::MAX_PENDING {
                self.last_received = input.tick;
                self.pending.push_back(*input);
            }
        }
    }

    /// Take up to `max` inputs to apply this server tick. Normally one input
    /// arrives per tick; a little more is allowed to catch up after a burst.
    pub fn take(&mut self, max: usize) -> impl Iterator<Item = NetInput> + '_ {
        let n = self.pending.len().min(max);
        self.pending
            .drain(..n)
            .inspect(|i| self.last_applied = i.tick)
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks;
    use crate::chunk::Chunk;
    use crate::world::{ColumnPos, VoxelWorld, WORLD_HEIGHT_CHUNKS};

    fn flat_world() -> VoxelWorld {
        let mut w = VoxelWorld::new();
        for cx in -2..2 {
            for cz in -2..2 {
                let col = (0..WORLD_HEIGHT_CHUNKS).map(|_| Chunk::empty()).collect();
                w.insert_column(ColumnPos::new(cx, cz), col);
            }
        }
        for x in -32..32 {
            for z in -32..32 {
                w.set_block(x, 9, z, blocks::STONE);
            }
        }
        w
    }

    fn walk(tick: u32) -> NetInput {
        NetInput {
            tick,
            movement: [0.0, 1.0],
            ..Default::default()
        }
    }

    /// Run the "client": predict `n` ticks, recording history.
    fn predict(
        n: u32,
        state: &mut PlayerState,
        history: &mut PredictionHistory,
        world: &VoxelWorld,
        reg: &BlockRegistry,
    ) -> Vec<NetInput> {
        let mut sent = Vec::new();
        for tick in 1..=n {
            let input = walk(tick);
            apply_input(state, &input, world, reg);
            history.push(input, *state);
            sent.push(input);
        }
        sent
    }

    #[test]
    fn same_inputs_same_world_means_no_correction() {
        let world = flat_world();
        let reg = BlockRegistry::with_builtin_blocks();
        let start = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        let (mut client, mut server) = (start, start);
        let mut history = PredictionHistory::default();
        let sent = predict(30, &mut client, &mut history, &world, &reg);

        // The server applies the first 20 inputs and acknowledges tick 20.
        let mut buffer = InputBuffer::default();
        buffer.receive(&sent[..20]);
        for input in buffer.take(100).collect::<Vec<_>>() {
            apply_input(&mut server, &input, &world, &reg);
        }
        assert_eq!(buffer.last_applied, 20);
        let r = history.reconcile(20, server, &mut client, &world, &reg);
        assert_eq!(r, Reconciled::Matched);
        // Ticks 21..=30 are still waiting for confirmation.
        assert_eq!(history.len(), 10);
    }

    #[test]
    fn mismatch_is_corrected_by_replaying() {
        let mut client_world = flat_world();
        let reg = BlockRegistry::with_builtin_blocks();
        let start = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        let mut history = PredictionHistory::default();
        let mut client = start;
        predict(60, &mut client, &mut history, &client_world, &reg);

        // On the server, a wall blocks the way at z = -2 (walking goes -Z).
        let mut server_world = flat_world();
        for x in -3..4 {
            server_world.set_block(x, 10, -2, blocks::STONE);
            server_world.set_block(x, 11, -2, blocks::STONE);
        }
        let mut server = start;
        for tick in 1..=30 {
            apply_input(&mut server, &walk(tick), &server_world, &reg);
        }
        // The client learns about the wall too (it arrives with the state).
        client_world.set_block(0, 10, -2, blocks::STONE);
        client_world.set_block(0, 11, -2, blocks::STONE);

        let r = history.reconcile(30, server, &mut client, &client_world, &reg);
        assert!(matches!(r, Reconciled::Corrected { .. }), "{r:?}");
        // After replaying ticks 31..=60 against the wall we're stopped at it.
        assert!(client.position.z > -1.0, "z = {}", client.position.z);
    }

    #[test]
    fn unknown_tick_snaps_to_server() {
        let world = flat_world();
        let reg = BlockRegistry::with_builtin_blocks();
        let mut history = PredictionHistory::default();
        let mut client = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        let server = PlayerState::new(Vec3::new(5.5, 10.0, 5.5));
        let r = history.reconcile(999, server, &mut client, &world, &reg);
        assert_eq!(r, Reconciled::Snapped);
        assert_eq!(client.position, server.position);
    }

    #[test]
    fn input_buffer_drops_duplicates_and_limits_rate() {
        let mut b = InputBuffer::default();
        // Each packet repeats the last few inputs (in case one was lost).
        b.receive(&[walk(1), walk(2), walk(3)]);
        b.receive(&[walk(2), walk(3), walk(4)]);
        assert_eq!(b.pending(), 4);
        let ticks: Vec<u32> = b.take(2).map(|i| i.tick).collect();
        assert_eq!(ticks, vec![1, 2]);
        assert_eq!(b.last_applied, 2);
        // A flood of inputs is capped.
        let flood: Vec<NetInput> = (5..500).map(walk).collect();
        b.receive(&flood);
        assert_eq!(b.pending(), InputBuffer::MAX_PENDING);
    }

    #[test]
    fn recent_inputs_returns_newest() {
        let world = flat_world();
        let reg = BlockRegistry::with_builtin_blocks();
        let mut history = PredictionHistory::default();
        let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        predict(10, &mut s, &mut history, &world, &reg);
        let ticks: Vec<u32> = history.recent_inputs(3).iter().map(|i| i.tick).collect();
        assert_eq!(ticks, vec![8, 9, 10]);
    }
}

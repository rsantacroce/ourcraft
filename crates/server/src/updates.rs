//! Block updates: when a block changes, the blocks around it get a look a
//! moment later, so water and lava can flow and sand and gravel can fall.
//!
//! The rules themselves are in `ourcraft_common::fluids`; this module keeps
//! the queue of spots to look at and applies the changes.

use std::collections::{BTreeMap, HashSet};

use bevy::prelude::*;
use ourcraft_common::block::{BlockId, blocks};
use ourcraft_common::fluids::{delay_ticks, next_state};
use ourcraft_common::protocol::{Channel, ServerMessage};

use crate::net::Network;
use crate::world::{ColumnCache, OpenWorld, Registry, ServerWorld, set_block};

/// Block updates per second.
const UPDATE_RATE: f32 = 20.0;
/// Most spots looked at per update, so a big flood can't stall the server.
const MAX_PER_UPDATE: usize = 400;
/// Ticks before a block falls one step.
const FALL_TICKS: u64 = 2;

pub struct UpdatesPlugin;

impl Plugin for UpdatesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockUpdates>()
            .add_systems(Update, run_updates.after(crate::players::PlayersSet));
    }
}

/// Spots waiting to be looked at, by the update tick they're due.
#[derive(Resource, Default)]
pub struct BlockUpdates {
    now: u64,
    due: BTreeMap<u64, Vec<IVec3>>,
    pending: HashSet<IVec3>,
}

const AROUND: [IVec3; 7] = [
    IVec3::ZERO,
    IVec3::X,
    IVec3::NEG_X,
    IVec3::Y,
    IVec3::NEG_Y,
    IVec3::Z,
    IVec3::NEG_Z,
];

impl BlockUpdates {
    /// Look at `pos` and its six neighbours in `delay` ticks.
    pub fn around(&mut self, pos: IVec3, delay: u64) {
        for d in AROUND {
            let p = pos + d;
            if self.pending.insert(p) {
                self.due.entry(self.now + delay).or_default().push(p);
            }
        }
    }

    pub fn is_idle(&self) -> bool {
        self.due.is_empty()
    }
}

fn run_updates(
    time: Res<Time>,
    mut updates: ResMut<BlockUpdates>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut net: ResMut<Network>,
    mut clock: Local<f32>,
) {
    *clock += time.delta_secs();
    if *clock < 1.0 / UPDATE_RATE {
        return;
    }
    *clock = 0.0;
    updates.now += 1;
    let now = updates.now;
    let reg = &registry.0;

    // Take what's due (up to the limit; the rest waits a tick).
    let mut batch = Vec::new();
    while batch.len() < MAX_PER_UPDATE {
        let Some(mut entry) = updates.due.first_entry() else {
            break;
        };
        if *entry.key() > now {
            break;
        }
        let list = entry.get_mut();
        let take = (MAX_PER_UPDATE - batch.len()).min(list.len());
        batch.extend(list.drain(..take));
        if list.is_empty() {
            entry.remove();
        }
    }

    let mut changed: Vec<(IVec3, BlockId, u64)> = Vec::new();
    for pos in batch {
        updates.pending.remove(&pos);
        let Some(here) = world.get_block(pos.x, pos.y, pos.z) else {
            continue;
        };
        // Sand and gravel drop into air (or flowing water) below.
        let below = pos - IVec3::Y;
        if reg.falls(here)
            && let Some(under) = world.get_block(below.x, below.y, below.z)
            && reg.is_replaceable(under)
        {
            changed.push((below, here, FALL_TICKS));
            changed.push((pos, BlockId::AIR, FALL_TICKS));
            continue;
        }
        // Unloaded places count as stone: fluids don't flow off the map.
        let get = |o: [i32; 3]| {
            world
                .get_block(pos.x + o[0], pos.y + o[1], pos.z + o[2])
                .unwrap_or(blocks::STONE)
        };
        if let Some(new) = next_state(reg, get) {
            let family = reg
                .fluid(new)
                .or(reg.fluid(here))
                .map_or("core:water", |f| f.family.as_str());
            changed.push((pos, new, delay_ticks(family)));
        }
    }
    for (pos, block, delay) in changed {
        if set_block(&mut world, &mut cache, &open, pos, block).is_some() {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::BlockChanged {
                    pos: pos.to_array(),
                    block: block.0,
                },
            );
            updates.around(pos, delay);
        }
    }
}

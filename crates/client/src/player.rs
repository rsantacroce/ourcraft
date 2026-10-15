//! The local player: physics on a fixed timestep, mouse look, flying, and
//! a camera that follows the player's eyes.

use std::f32::consts::FRAC_PI_2;

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::physics::{self, PlayerState, TICK_RATE};
use ourcraft_common::prediction::{PredictionHistory, Reconciled, apply_input};
use ourcraft_common::protocol::{Channel, ClientMessage, NetInput, NetPlayerState, ServerMessage};

use crate::input::Actions;
use crate::interaction::PlayerInventory;
use crate::net::{Connection, FromServer, NetReceiveSet};
use crate::world::{Blocks, ChunkViewer, ClientWorld};

/// How many recent inputs each input packet carries. If a packet is lost,
/// the next one still has its inputs.
const INPUT_REDUNDANCY: usize = 4;
/// Corrections bigger than this (in blocks) are applied instantly rather
/// than smoothed (we probably teleported).
const MAX_SMOOTHED_ERROR: f32 = 4.0;
/// How fast a visual correction fades, per second.
const SMOOTHING_RATE: f32 = 12.0;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(TICK_RATE))
            .init_resource::<GameMode>()
            .add_systems(Startup, spawn_player)
            .add_systems(
                PreUpdate,
                (handle_welcome, reconcile).chain().after(NetReceiveSet),
            )
            .add_systems(FixedUpdate, player_physics.in_set(PlayerPhysicsSet))
            .add_systems(
                Update,
                (look, toggle_fly, toggle_game_mode, follow_camera)
                    .chain()
                    .in_set(PlayerUpdateSet),
            )
            .add_systems(Update, send_inventory.run_if(resource_exists::<Connection>));
    }
}

/// The fixed-step physics system, so networking (M7) can order around it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerPhysicsSet;

/// Per-frame player systems (look, camera follow). Anything that uses the
/// camera position should run after this.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerUpdateSet;

/// Creative: fly, infinite blocks. Survival: walk, collect what you break.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    #[default]
    Creative,
    Survival,
}

/// The local player.
#[derive(Component, Debug)]
pub struct Player {
    pub state: PlayerState,
    /// Position at the previous physics step, for smoothing the camera.
    pub prev_position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Seconds since jump was last pressed, for double-tap-to-fly.
    last_jump_press: f32,
    /// Held in place until the server says where we are.
    pub frozen: bool,
    /// Physics ticks run so far (numbers our inputs for the server).
    pub tick: u32,
    /// Recent inputs and predictions, for reconciliation.
    pub history: PredictionHistory,
    /// Added to the camera position and faded out, so corrections from
    /// the server look smooth instead of a jump.
    pub visual_offset: Vec3,
    pub stats: PredictionStats,
}

/// Counters shown in the debug overlay.
#[derive(Debug, Default, Clone, Copy)]
pub struct PredictionStats {
    pub corrections: u32,
    pub snaps: u32,
    /// Size of the most recent correction, in blocks.
    pub last_error: f32,
}

impl Player {
    pub fn new(feet: Vec3) -> Self {
        Self {
            state: PlayerState::new(feet),
            prev_position: feet,
            yaw: 0.0,
            pitch: 0.0,
            last_jump_press: f32::MAX,
            frozen: true,
            tick: 0,
            history: PredictionHistory::default(),
            visual_offset: Vec3::ZERO,
            stats: PredictionStats::default(),
        }
    }

    pub fn net_state(&self) -> NetPlayerState {
        NetPlayerState {
            position: self.state.position.to_array(),
            velocity: self.state.velocity.to_array(),
            yaw: self.yaw,
            pitch: self.pitch,
            flying: self.state.flying,
            on_ground: self.state.on_ground,
        }
    }

    /// The direction the player is looking.
    pub fn look_direction(&self) -> Vec3 {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0) * Vec3::NEG_Z
    }
}

/// The camera that renders the player's view.
#[derive(Component)]
pub struct PlayerCamera;

fn spawn_player(mut commands: Commands) {
    // Parked high up and frozen until the server's welcome message says
    // where we really are.
    let feet = Vec3::new(0.5, 200.0, 0.5);
    let player = Player::new(feet);
    commands.spawn((Name::new("Player"), player));

    commands.spawn((
        Name::new("Player camera"),
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            near: 0.05,
            ..default()
        }),
        // Our block shader outputs final colours itself; no film-style
        // colour grading on top, which keeps the colours bright and cartoony.
        Tonemapping::None,
        Transform::from_translation(feet + Vec3::Y * physics::EYE_HEIGHT),
        PlayerCamera,
        ChunkViewer,
    ));
}

/// The server's welcome: where we are, our inventory and the game mode.
fn handle_welcome(
    mut messages: MessageReader<FromServer>,
    mut player: Single<&mut Player>,
    mut inventory: ResMut<PlayerInventory>,
    mut mode: ResMut<GameMode>,
) {
    for FromServer(msg) in messages.read() {
        let ServerMessage::Welcome {
            state,
            inventory: saved,
            creative,
            ..
        } = msg
        else {
            continue;
        };
        *mode = if *creative {
            GameMode::Creative
        } else {
            GameMode::Survival
        };
        let feet = Vec3::from_array(state.position);
        player.state = PlayerState::new(feet);
        player.state.flying = state.flying;
        player.prev_position = feet;
        player.yaw = state.yaw;
        player.pitch = state.pitch;
        player.frozen = false;
        // A fresh start: no predictions to check yet.
        player.history = PredictionHistory::default();
        player.visual_offset = Vec3::ZERO;
        if let Some((slots, selected)) = saved {
            for (slot, saved) in inventory.slots.iter_mut().zip(slots) {
                *slot = saved.map(|(id, count)| ItemStack {
                    block: BlockId(id),
                    count,
                });
            }
            inventory.select(*selected as usize);
        }
    }
}

/// Send the inventory when it changes (at most once a second), so the
/// server can save it.
fn send_inventory(
    time: Res<Time>,
    inventory: Res<PlayerInventory>,
    mut conn: ResMut<Connection>,
    mut pending: Local<bool>,
    mut timer: Local<f32>,
) {
    if inventory.is_changed() {
        *pending = true;
    }
    *timer += time.delta_secs();
    if *pending && *timer >= 1.0 && conn.is_welcomed() {
        *timer = 0.0;
        *pending = false;
        let msg = ClientMessage::Inventory {
            slots: inventory
                .slots
                .iter()
                .map(|s| s.map(|s| (s.block.0, s.count)))
                .collect(),
            selected: inventory.selected as u8,
        };
        conn.send(Channel::Reliable, &msg);
    }
}

/// One fixed physics step: predict our own movement immediately, remember
/// the input, and send it (with a few previous ones) to the server.
fn player_physics(
    actions: Res<Actions>,
    touch: Res<crate::touch::TouchControls>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    conn: Option<ResMut<Connection>>,
    mut player: Single<&mut Player>,
) {
    if player.frozen {
        return;
    }
    let player = &mut **player;
    player.tick += 1;
    let input = NetInput {
        tick: player.tick,
        movement: actions.movement.to_array(),
        jump: actions.jump,
        descend: actions.descend,
        sprint: actions.sprint,
        auto_jump: touch.enabled,
        fly: player.state.flying,
        yaw: player.yaw,
        pitch: player.pitch,
    };
    player.prev_position = player.state.position;
    apply_input(&mut player.state, &input, &world.0, &blocks.0);
    player.history.push(input, player.state);
    if let Some(mut conn) = conn
        && conn.is_welcomed()
    {
        let recent = player.history.recent_inputs(INPUT_REDUNDANCY);
        conn.send(Channel::Unreliable, &ClientMessage::Inputs(recent));
    }
}

/// Check the server's view of our position against what we predicted.
fn reconcile(
    mut messages: MessageReader<FromServer>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    mut player: Single<&mut Player>,
) {
    for FromServer(msg) in messages.read() {
        let ServerMessage::YourState { ack_tick, state } = msg else {
            continue;
        };
        let player = &mut **player;
        if player.frozen {
            continue;
        }
        let mut server = PlayerState::new(Vec3::from_array(state.position));
        server.velocity = Vec3::from_array(state.velocity);
        server.flying = state.flying;
        server.on_ground = state.on_ground;
        let before = player.state.position;
        match player
            .history
            .reconcile(*ack_tick, server, &mut player.state, &world.0, &blocks.0)
        {
            Reconciled::Matched => {}
            Reconciled::Corrected { error } => {
                player.stats.corrections += 1;
                player.stats.last_error = error.length();
                // Keep the in-between-ticks blend consistent.
                player.prev_position -= error;
                if error.length() < MAX_SMOOTHED_ERROR {
                    player.visual_offset += error;
                } else {
                    player.visual_offset = Vec3::ZERO;
                }
            }
            Reconciled::Snapped => {
                player.stats.snaps += 1;
                player.stats.last_error = before.distance(player.state.position);
                player.prev_position = player.state.position;
                player.visual_offset = Vec3::ZERO;
            }
        }
    }
}

/// Turning happens every frame (not every physics step) so the view
/// responds instantly to the mouse.
fn look(actions: Res<Actions>, mut player: Single<&mut Player>) {
    const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
    // Yaw is negated because Bevy's rotation is counter-clockwise when seen
    // from above, while "look right" is clockwise.
    player.yaw -= actions.look.x;
    player.pitch = (player.pitch + actions.look.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
}

/// F (or double-tapping jump) toggles flying in creative mode.
fn toggle_fly(
    time: Res<Time>,
    actions: Res<Actions>,
    mode: Res<GameMode>,
    mut player: Single<&mut Player>,
) {
    player.last_jump_press += time.delta_secs();
    let mut toggle = actions.toggle_fly;
    if actions.jump_pressed {
        toggle |= player.last_jump_press < 0.3;
        player.last_jump_press = 0.0;
    }
    if *mode != GameMode::Creative {
        player.state.flying = false;
    } else if toggle {
        player.state.flying = !player.state.flying;
        player.state.velocity = Vec3::ZERO;
        player.last_jump_press = f32::MAX;
    }
}

/// G switches between creative and survival.
fn toggle_game_mode(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<crate::input::UiFocus>,
    mut mode: ResMut<GameMode>,
) {
    if keys.just_pressed(KeyCode::KeyG) && !focus.chat_open {
        *mode = match *mode {
            GameMode::Creative => GameMode::Survival,
            GameMode::Survival => GameMode::Creative,
        };
        info!("Game mode: {:?}", *mode);
    }
}

/// Place the camera at the player's eyes. Physics runs at 60 steps per
/// second but the screen may refresh at 120+, so we blend between the last
/// two physics positions according to how far we are into the next step.
fn follow_camera(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    mut player: Single<&mut Player>,
    mut camera: Single<&mut Transform, With<PlayerCamera>>,
) {
    // Fade out any correction offset smoothly.
    let fade = (-SMOOTHING_RATE * time.delta_secs()).exp();
    player.visual_offset *= fade;
    let t = fixed.overstep_fraction();
    let feet = player.prev_position.lerp(player.state.position, t) + player.visual_offset;
    camera.translation = feet + Vec3::Y * physics::EYE_HEIGHT;
    camera.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
}

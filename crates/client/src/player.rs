//! The local player: physics on a fixed timestep, mouse look, flying, and
//! a camera that follows the player's eyes.

use std::f32::consts::FRAC_PI_2;

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::physics::{self, PlayerInput, PlayerState, TICK_RATE};
use ourcraft_common::protocol::{Channel, ClientMessage, NetPlayerState, ServerMessage};

use crate::input::Actions;
use crate::interaction::PlayerInventory;
use crate::net::{Connection, FromServer, NetReceiveSet};
use crate::world::{Blocks, ChunkViewer, ClientWorld};

/// Position updates sent to the server per second.
const STATE_SEND_RATE: f32 = 20.0;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(TICK_RATE))
            .init_resource::<GameMode>()
            .add_systems(Startup, spawn_player)
            .add_systems(PreUpdate, handle_welcome.after(NetReceiveSet))
            .add_systems(FixedUpdate, player_physics.in_set(PlayerPhysicsSet))
            .add_systems(
                Update,
                (look, toggle_fly, toggle_game_mode, follow_camera)
                    .chain()
                    .in_set(PlayerUpdateSet),
            )
            .add_systems(
                Update,
                (send_player_state, send_inventory).run_if(resource_exists::<Connection>),
            );
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

/// Tell the server where we are, 20 times a second.
fn send_player_state(
    time: Res<Time>,
    player: Single<&Player>,
    mut conn: ResMut<Connection>,
    mut timer: Local<f32>,
    mut tick: Local<u32>,
) {
    *timer += time.delta_secs();
    if *timer < 1.0 / STATE_SEND_RATE || player.frozen || !conn.is_welcomed() {
        return;
    }
    *timer = 0.0;
    *tick += 1;
    let msg = ClientMessage::PlayerState {
        tick: *tick,
        state: player.net_state(),
    };
    conn.send(Channel::Unreliable, &msg);
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

/// Advance the player's physics by one fixed step.
fn player_physics(
    actions: Res<Actions>,
    touch: Res<crate::touch::TouchControls>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    mut player: Single<&mut Player>,
) {
    let input = PlayerInput {
        movement: actions.movement,
        jump: actions.jump,
        descend: actions.descend,
        sprint: actions.sprint,
        yaw: player.yaw,
        auto_jump: touch.enabled,
    };
    if player.frozen {
        return;
    }
    player.prev_position = player.state.position;
    let player = &mut **player;
    physics::step(&mut player.state, &input, &world.0, &blocks.0);
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
fn toggle_game_mode(keys: Res<ButtonInput<KeyCode>>, mut mode: ResMut<GameMode>) {
    if keys.just_pressed(KeyCode::KeyG) {
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
    fixed: Res<Time<Fixed>>,
    player: Single<&Player>,
    mut camera: Single<&mut Transform, With<PlayerCamera>>,
) {
    let t = fixed.overstep_fraction();
    let feet = player.prev_position.lerp(player.state.position, t);
    camera.translation = feet + Vec3::Y * physics::EYE_HEIGHT;
    camera.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
}

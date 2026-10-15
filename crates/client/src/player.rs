//! The local player: physics on a fixed timestep, mouse look, flying, and
//! a camera that follows the player's eyes.

use std::f32::consts::FRAC_PI_2;

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use ourcraft_common::physics::{self, PlayerInput, PlayerState, TICK_RATE};

use crate::input::Actions;
use crate::world::{Blocks, ChunkViewer, ClientWorld, TerrainGenerator};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_hz(TICK_RATE))
            .init_resource::<GameMode>()
            .add_systems(Startup, spawn_player)
            .add_systems(FixedUpdate, player_physics.in_set(PlayerPhysicsSet))
            .add_systems(
                Update,
                (look, toggle_fly, toggle_game_mode, follow_camera)
                    .chain()
                    .in_set(PlayerUpdateSet),
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
}

impl Player {
    pub fn new(feet: Vec3) -> Self {
        Self {
            state: PlayerState::new(feet),
            prev_position: feet,
            yaw: 0.0,
            pitch: 0.0,
            last_jump_press: f32::MAX,
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

fn spawn_player(
    mut commands: Commands,
    generator: Res<TerrainGenerator>,
    storage: Res<crate::persistence::Storage>,
    mut inventory: ResMut<crate::interaction::PlayerInventory>,
    mut mode: ResMut<GameMode>,
) {
    *mode = if storage.meta.creative {
        GameMode::Creative
    } else {
        GameMode::Survival
    };
    let player = match &storage.player {
        // Back where we left off.
        Some(save) => {
            let mut p = Player::new(Vec3::from_array(save.position));
            p.yaw = save.yaw;
            p.pitch = save.pitch;
            p.state.flying = save.flying;
            inventory.0 = save.inventory(storage.remap.as_ref());
            p
        }
        // New world: find a nice spot.
        None => {
            let (x, ground, z) = generator.0.find_spawn();
            let mut p = Player::new(Vec3::new(
                x as f32 + 0.5,
                ground as f32 + 1.0,
                z as f32 + 0.5,
            ));
            p.yaw = -std::f32::consts::FRAC_PI_4;
            p.pitch = -0.35; // look slightly down at the ground ahead
            p
        }
    };
    let feet = player.state.position;
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

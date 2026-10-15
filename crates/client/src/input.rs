//! Turns raw input (keyboard, mouse, gamepad, and touch in M4) into abstract
//! actions.
//!
//! Gameplay code only ever reads [`Actions`], never the keyboard directly.
//! That keeps it the same on every device: adding touch controls for the iPad
//! later means writing one more input source here, not touching gameplay.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Actions>()
            .init_resource::<InputSettings>()
            // PreUpdate runs before Update, so every gameplay system this
            // frame sees the same, fully-built actions.
            .add_systems(
                PreUpdate,
                (clear_actions, (keyboard_mouse_actions, gamepad_actions))
                    .chain()
                    .after(bevy::input::InputSystems),
            )
            .add_systems(Update, grab_cursor);
    }
}

/// What the player wants to do this frame.
#[derive(Resource, Default, Debug)]
pub struct Actions {
    /// Desired movement relative to where the camera faces:
    /// `x` = strafe right, `y` = up, `z` = forward. Each in `-1..=1`.
    pub movement: Vec3,
    /// Camera rotation this frame in radians: `x` = yaw (left/right),
    /// `y` = pitch (up/down). Positive turns right / looks up.
    pub look: Vec2,
    /// Move faster.
    pub sprint: bool,
}

#[derive(Resource, Debug)]
pub struct InputSettings {
    /// Radians per pixel of mouse movement.
    pub mouse_sensitivity: f32,
    /// Radians per second at full stick deflection.
    pub stick_look_speed: f32,
    /// Stick values below this are ignored (sticks never rest at exactly 0).
    pub stick_deadzone: f32,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.0025,
            stick_look_speed: 3.0,
            stick_deadzone: 0.15,
        }
    }
}

fn clear_actions(mut actions: ResMut<Actions>) {
    *actions = Actions::default();
}

fn keyboard_mouse_actions(
    keys: Res<ButtonInput<KeyCode>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions>,
    settings: Res<InputSettings>,
    mut actions: ResMut<Actions>,
) {
    let axis =
        |neg: KeyCode, pos: KeyCode| (keys.pressed(pos) as i32 - keys.pressed(neg) as i32) as f32;
    actions.movement += Vec3::new(
        axis(KeyCode::KeyA, KeyCode::KeyD),
        axis(KeyCode::ShiftLeft, KeyCode::Space),
        axis(KeyCode::KeyS, KeyCode::KeyW),
    );
    actions.sprint |= keys.pressed(KeyCode::ControlLeft);

    // Only look around while the mouse is captured, so the camera doesn't
    // spin when you move the pointer to another window.
    if cursor.grab_mode != CursorGrabMode::None {
        // Mouse motion is already "per frame" (it's the distance moved since
        // last frame), so unlike keys and sticks we don't multiply by time.
        let delta = mouse_motion.delta * settings.mouse_sensitivity;
        actions.look += Vec2::new(delta.x, -delta.y);
    }
}

fn gamepad_actions(
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    settings: Res<InputSettings>,
    mut actions: ResMut<Actions>,
) {
    let dz = |v: f32| {
        if v.abs() < settings.stick_deadzone {
            0.0
        } else {
            v
        }
    };
    for pad in &gamepads {
        let get = |a: GamepadAxis| dz(pad.get(a).unwrap_or(0.0));
        let up = pad.pressed(GamepadButton::South) as i32 as f32
            - pad.pressed(GamepadButton::East) as i32 as f32;
        actions.movement += Vec3::new(
            get(GamepadAxis::LeftStickX),
            up,
            get(GamepadAxis::LeftStickY),
        );
        actions.sprint |= pad.pressed(GamepadButton::LeftThumb);
        // Sticks report a position, not a distance, so scale by frame time.
        actions.look += Vec2::new(get(GamepadAxis::RightStickX), get(GamepadAxis::RightStickY))
            * settings.stick_look_speed
            * time.delta_secs();
    }
    actions.movement = actions.movement.clamp(Vec3::splat(-1.0), Vec3::splat(1.0));
}

/// Click to capture the mouse; Esc to release it.
fn grab_cursor(
    mut cursor: Single<&mut CursorOptions>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        cursor.visible = false;
        // `Locked` keeps the pointer in place; some platforms (X11) only
        // support `Confined`, which still works for mouse look.
        cursor.grab_mode = if cfg!(target_os = "linux") {
            CursorGrabMode::Confined
        } else {
            CursorGrabMode::Locked
        };
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.visible = true;
        cursor.grab_mode = CursorGrabMode::None;
    }
}

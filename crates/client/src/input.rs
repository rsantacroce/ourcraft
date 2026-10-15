//! Turns raw input (keyboard, mouse, gamepad, and touch in M4) into abstract
//! actions.
//!
//! Gameplay code only ever reads [`Actions`], never the keyboard directly.
//! That keeps it the same on every device: adding touch controls for the iPad
//! means writing one more input source here, not touching gameplay.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Actions>()
            .init_resource::<InputSettings>()
            .init_resource::<UiFocus>()
            // PreUpdate runs before Update, so every gameplay system this
            // frame sees the same, fully-built actions.
            .add_systems(
                PreUpdate,
                (clear_actions, (keyboard_mouse_actions, gamepad_actions))
                    .chain()
                    .in_set(ActionsSet)
                    .after(bevy::input::InputSystems),
            )
            .add_systems(Update, grab_cursor);
    }
}

/// Systems that fill in [`Actions`]. Other input sources (touch) add
/// themselves to this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionsSet;

/// What the player wants to do this frame.
#[derive(Resource, Default, Debug, Clone)]
pub struct Actions {
    /// Desired movement relative to where the player faces:
    /// `x` = strafe right, `y` = forward. Each in `-1..=1`.
    pub movement: Vec2,
    /// Camera rotation this frame in radians: `x` = yaw (left/right),
    /// `y` = pitch (up/down). Positive turns right / looks up.
    pub look: Vec2,
    /// Held: jump, swim up or fly up.
    pub jump: bool,
    /// Pressed this frame (for double-tap detection).
    pub jump_pressed: bool,
    /// Held: fly or swim down.
    pub descend: bool,
    pub sprint: bool,
    /// Held: break the targeted block (repeats while held).
    pub break_block: bool,
    /// Held: place a block (repeats while held).
    pub place_block: bool,
    /// Pressed: copy the targeted block into the hotbar (creative).
    pub pick_block: bool,
    pub toggle_fly: bool,
    pub toggle_inventory: bool,
    /// Pressed a number key: select that hotbar slot.
    pub hotbar_select: Option<usize>,
    /// Scroll through the hotbar: positive = next slot.
    pub hotbar_scroll: i32,
    /// Open (or close) the pause menu (touch Menu button; Esc is read
    /// directly by the menu).
    pub open_menu: bool,
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

/// Whether a menu or screen has the focus, in which case gameplay input
/// (looking, breaking blocks…) is ignored and the mouse is free.
#[derive(Resource, Default, Debug)]
pub struct UiFocus {
    pub inventory_open: bool,
    pub menu_open: bool,
    pub chat_open: bool,
}

impl UiFocus {
    pub fn gameplay_blocked(&self) -> bool {
        self.inventory_open || self.menu_open || self.chat_open
    }
}

fn clear_actions(mut actions: ResMut<Actions>) {
    *actions = Actions::default();
}

const HOTBAR_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

fn keyboard_mouse_actions(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    cursor: Option<Single<&CursorOptions>>,
    settings: Res<InputSettings>,
    focus: Res<UiFocus>,
    mut actions: ResMut<Actions>,
    mut was_captured: Local<bool>,
    mut ignore_buttons: Local<bool>,
) {
    if focus.chat_open {
        return; // typing in chat: keys are letters, not controls
    }
    actions.toggle_inventory |= keys.just_pressed(KeyCode::KeyE);
    if focus.gameplay_blocked() {
        return;
    }

    let axis =
        |neg: KeyCode, pos: KeyCode| (keys.pressed(pos) as i32 - keys.pressed(neg) as i32) as f32;
    actions.movement += Vec2::new(
        axis(KeyCode::KeyA, KeyCode::KeyD),
        axis(KeyCode::KeyS, KeyCode::KeyW),
    );
    actions.jump |= keys.pressed(KeyCode::Space);
    actions.jump_pressed |= keys.just_pressed(KeyCode::Space);
    actions.descend |= keys.pressed(KeyCode::ShiftLeft);
    actions.sprint |= keys.pressed(KeyCode::ControlLeft);
    actions.toggle_fly |= keys.just_pressed(KeyCode::KeyF);
    for (i, key) in HOTBAR_KEYS.iter().enumerate() {
        if keys.just_pressed(*key) {
            actions.hotbar_select = Some(i);
        }
    }
    // Scrolling down moves to the next slot, like most games. (Careful:
    // `0.0f32.signum()` is 1.0, so check for zero first.)
    if scroll.delta.y != 0.0 {
        actions.hotbar_scroll -= scroll.delta.y.signum() as i32;
    }

    // Mouse look and clicks only count while the mouse is captured, so the
    // click that captures it doesn't also break a block.
    let captured = cursor.is_some_and(|c| c.grab_mode != CursorGrabMode::None);
    // The click that captures the mouse shouldn't also break a block or
    // fire: ignore the buttons until they've been let go.
    if captured && !*was_captured {
        *ignore_buttons = true;
    }
    *was_captured = captured;
    if *ignore_buttons && !mouse.pressed(MouseButton::Left) && !mouse.pressed(MouseButton::Right) {
        *ignore_buttons = false;
    }
    if captured && *ignore_buttons {
        let delta = mouse_motion.delta * settings.mouse_sensitivity;
        actions.look += Vec2::new(delta.x, -delta.y);
    } else if captured {
        // Mouse motion is already "per frame" (it's the distance moved since
        // last frame), so unlike keys and sticks we don't multiply by time.
        let delta = mouse_motion.delta * settings.mouse_sensitivity;
        actions.look += Vec2::new(delta.x, -delta.y);
        actions.break_block |= mouse.pressed(MouseButton::Left);
        actions.place_block |= mouse.pressed(MouseButton::Right);
        actions.pick_block |= mouse.just_pressed(MouseButton::Middle);
    }
}

fn gamepad_actions(
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    settings: Res<InputSettings>,
    focus: Res<UiFocus>,
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
        actions.toggle_inventory |= pad.just_pressed(GamepadButton::North);
        if focus.gameplay_blocked() {
            continue;
        }
        let get = |a: GamepadAxis| dz(pad.get(a).unwrap_or(0.0));
        actions.movement += Vec2::new(get(GamepadAxis::LeftStickX), get(GamepadAxis::LeftStickY));
        actions.jump |= pad.pressed(GamepadButton::South);
        actions.jump_pressed |= pad.just_pressed(GamepadButton::South);
        actions.descend |= pad.pressed(GamepadButton::East);
        actions.sprint |= pad.pressed(GamepadButton::LeftThumb);
        actions.toggle_fly |= pad.just_pressed(GamepadButton::West);
        actions.break_block |= pad.pressed(GamepadButton::RightTrigger2);
        actions.place_block |= pad.pressed(GamepadButton::LeftTrigger2);
        actions.pick_block |= pad.just_pressed(GamepadButton::DPadUp);
        actions.hotbar_scroll += pad.just_pressed(GamepadButton::RightTrigger) as i32
            - pad.just_pressed(GamepadButton::LeftTrigger) as i32;
        // Sticks report a position, not a distance, so scale by frame time.
        actions.look += Vec2::new(get(GamepadAxis::RightStickX), get(GamepadAxis::RightStickY))
            * settings.stick_look_speed
            * time.delta_secs();
    }
    actions.movement = actions.movement.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
}

/// Click to capture the mouse; Esc to release it. Opening a screen releases
/// it too.
fn grab_cursor(
    cursor: Option<Single<&mut CursorOptions>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<UiFocus>,
) {
    let Some(mut cursor) = cursor else { return };
    let release = keys.just_pressed(KeyCode::Escape) || focus.gameplay_blocked();
    if release {
        if cursor.grab_mode != CursorGrabMode::None {
            cursor.visible = true;
            cursor.grab_mode = CursorGrabMode::None;
        }
    } else if mouse.just_pressed(MouseButton::Left) {
        cursor.visible = false;
        // `Locked` keeps the pointer in place; some platforms (X11) only
        // support `Confined`, which still works for mouse look.
        cursor.grab_mode = if cfg!(target_os = "linux") {
            CursorGrabMode::Confined
        } else {
            CursorGrabMode::Locked
        };
    }
}

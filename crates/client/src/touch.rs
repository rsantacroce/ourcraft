//! Touch controls for tablets and phones.
//!
//! - **Left side:** a floating joystick. Put your thumb down anywhere on the
//!   left and drag to walk.
//! - **Right side:** drag to look around. A quick *tap* places a block at the
//!   crosshair; *press and hold* breaks it.
//! - **Buttons:** jump, down (fly/swim down), fly toggle and inventory.
//! - **Hotbar:** tap a slot to select it.
//!
//! All of this just fills in [`Actions`], exactly like the keyboard does, so
//! gameplay code doesn't know or care which device you're using.

use bevy::input::touch::Touches;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use ourcraft_common::inventory::HOTBAR_SLOTS;

use crate::input::{Actions, ActionsSet, UiFocus};
use crate::interaction::PlayerInventory;
use crate::player::GameMode;

pub struct TouchPlugin;

impl Plugin for TouchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TouchControls>()
            .init_resource::<TouchState>()
            .add_systems(Startup, spawn_touch_ui)
            .add_systems(
                PreUpdate,
                (detect_touch, touch_actions)
                    .chain()
                    .in_set(ActionsSet)
                    .after(bevy::input::InputSystems),
            )
            .add_systems(Update, update_touch_ui);
    }
}

/// Whether touch controls are shown and active.
#[derive(Resource, Debug)]
pub struct TouchControls {
    pub enabled: bool,
    /// Radians per logical pixel dragged.
    pub look_sensitivity: f32,
}

impl Default for TouchControls {
    fn default() -> Self {
        Self {
            // Always on for phones and tablets; on desktop they switch on the
            // first time a touchscreen is used.
            enabled: cfg!(any(target_os = "ios", target_os = "android")),
            look_sensitivity: 0.006,
        }
    }
}

/// What a finger is doing, decided when it first touches the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Role {
    Joystick {
        origin: Vec2,
    },
    Look,
    Button(TouchButton),
    /// Touching the hotbar or a menu; Bevy UI handles it.
    Ui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TouchButton {
    Jump,
    Down,
    Fly,
    Inventory,
    /// Opens the pause menu (top left).
    Menu,
    /// Hold to eat (only shown with food in hand, in survival).
    Eat,
    /// Switch camera view (top, next to the bag).
    View,
}

const JOYSTICK_RADIUS: f32 = 70.0;
/// Fingers moving less than this many pixels count as "not moving" for taps.
const TAP_SLOP: f32 = 14.0;
const TAP_MAX_SECS: f32 = 0.25;
const HOLD_SECS: f32 = 0.35;

#[derive(Debug, Clone, Copy)]
struct Finger {
    role: Role,
    start_time: f32,
    /// Largest distance from where the finger started.
    max_travel: f32,
}

#[derive(Resource, Default, Debug)]
struct TouchState {
    fingers: Vec<(u64, Finger)>,
    /// Current joystick vector, for drawing the knob.
    joystick: Option<(Vec2, Vec2)>,
    /// Holding food in survival: the Eat button is showing.
    can_eat: bool,
}

/// Button positions, as (centre offset from the bottom-right corner, radius)
/// in logical pixels. Shared by hit-testing and drawing.
fn button_layout(button: TouchButton) -> (Vec2, f32) {
    match button {
        TouchButton::Jump => (Vec2::new(-95.0, -130.0), 52.0),
        TouchButton::Down => (Vec2::new(-215.0, -215.0), 40.0),
        TouchButton::Fly => (Vec2::new(-95.0, -225.0), 34.0),
        TouchButton::Inventory => (Vec2::new(-60.0, 0.0), 32.0), // placed at the top
        TouchButton::Menu => (Vec2::ZERO, 30.0),                 // top left
        TouchButton::Eat => (Vec2::new(-215.0, -105.0), 36.0),
        TouchButton::View => (Vec2::new(-140.0, 0.0), 28.0), // placed at the top
    }
}

const BUTTONS: [TouchButton; 7] = [
    TouchButton::Jump,
    TouchButton::Down,
    TouchButton::Fly,
    TouchButton::Inventory,
    TouchButton::Menu,
    TouchButton::Eat,
    TouchButton::View,
];

/// Screen-space centre of a button for a window of `size`.
fn button_center(button: TouchButton, size: Vec2) -> Vec2 {
    let (offset, _) = button_layout(button);
    if button == TouchButton::Inventory || button == TouchButton::View {
        Vec2::new(size.x + offset.x, 60.0)
    } else if button == TouchButton::Menu {
        Vec2::new(56.0, 56.0)
    } else {
        size + offset
    }
}

/// Logical-pixel rectangle covered by the hotbar (see `inventory_ui`).
fn hotbar_rect(size: Vec2) -> Rect {
    let width = HOTBAR_SLOTS as f32 * 56.0;
    Rect::new(
        size.x / 2.0 - width / 2.0,
        size.y - 12.0 - 52.0,
        size.x / 2.0 + width / 2.0,
        size.y - 12.0,
    )
}

/// Turn touch controls on as soon as a touchscreen is used.
fn detect_touch(touches: Res<Touches>, mut controls: ResMut<TouchControls>) {
    if !controls.enabled && touches.iter_just_pressed().next().is_some() {
        controls.enabled = true;
    }
}

fn touch_actions(
    time: Res<Time>,
    touches: Res<Touches>,
    controls: Res<TouchControls>,
    focus: Res<UiFocus>,
    window: Single<&Window, With<PrimaryWindow>>,
    mode: Res<GameMode>,
    mut state: ResMut<TouchState>,
    mut actions: ResMut<Actions>,
    mut inventory: ResMut<PlayerInventory>,
) {
    if !controls.enabled {
        return;
    }
    let now = time.elapsed_secs();
    let size = window.size();

    // New fingers: decide what each one is for.
    for touch in touches.iter_just_pressed() {
        let p = touch.position();
        let on_bag = p.distance(button_center(TouchButton::Inventory, size))
            <= button_layout(TouchButton::Inventory).1 * 1.2;
        let role = if on_bag && !focus.menu_open {
            // The bag button works even while the inventory is open, to close it.
            Role::Button(TouchButton::Inventory)
        } else if focus.gameplay_blocked() {
            Role::Ui
        } else if let Some(b) = BUTTONS.iter().copied().find(|b| {
            let visible = match b {
                TouchButton::Fly => *mode == GameMode::Creative,
                TouchButton::Eat => state.can_eat,
                _ => true,
            };
            visible && p.distance(button_center(*b, size)) <= button_layout(*b).1 * 1.2
        }) {
            Role::Button(b)
        } else if hotbar_rect(size).contains(p) {
            let slot = ((p.x - hotbar_rect(size).min.x) / 56.0) as usize;
            inventory.select(slot);
            Role::Ui
        } else if p.x < size.x * 0.4 {
            Role::Joystick { origin: p }
        } else {
            Role::Look
        };
        if let Role::Button(b) = role {
            match b {
                TouchButton::Jump => actions.jump_pressed = true,
                TouchButton::Fly => actions.toggle_fly = true,
                TouchButton::Inventory => actions.toggle_inventory = true,
                TouchButton::Menu => actions.open_menu = true,
                TouchButton::View => actions.cycle_view = true,
                TouchButton::Down | TouchButton::Eat => {}
            }
        }
        state.fingers.push((
            touch.id(),
            Finger {
                role,
                start_time: now,
                max_travel: 0.0,
            },
        ));
    }

    // Lifted fingers: a quick, still tap on the look area places a block.
    for touch in touches.iter_just_released() {
        if let Some(i) = state.fingers.iter().position(|(id, _)| *id == touch.id()) {
            let (_, f) = state.fingers.remove(i);
            let quick = now - f.start_time < TAP_MAX_SECS;
            if f.role == Role::Look && quick && f.max_travel < TAP_SLOP && !focus.gameplay_blocked()
            {
                actions.place_block = true;
            }
        }
    }
    for touch in touches.iter_just_canceled() {
        state.fingers.retain(|(id, _)| *id != touch.id());
    }

    // Fingers still down.
    state.joystick = None;
    let fingers = state.fingers.clone();
    for (id, mut finger) in fingers {
        let Some(touch) = touches.get_pressed(id) else {
            continue;
        };
        finger.max_travel = finger
            .max_travel
            .max(touch.position().distance(touch.start_position()));
        match finger.role {
            Role::Joystick { origin } => {
                let offset = (touch.position() - origin).clamp_length_max(JOYSTICK_RADIUS);
                // Screen y grows downwards, so dragging up means forward.
                actions.movement += Vec2::new(offset.x, -offset.y) / JOYSTICK_RADIUS;
                // Push the stick all the way to run.
                actions.sprint |= offset.length() > JOYSTICK_RADIUS * 0.95;
                state.joystick = Some((origin, origin + offset));
            }
            Role::Look => {
                let d = touch.delta() * controls.look_sensitivity;
                actions.look += Vec2::new(d.x, -d.y);
                let held = now - finger.start_time;
                if held > HOLD_SECS && finger.max_travel < TAP_SLOP {
                    actions.break_block = true;
                }
            }
            Role::Button(TouchButton::Jump) => actions.jump = true,
            Role::Button(TouchButton::Down) => actions.descend = true,
            Role::Button(TouchButton::Eat) => actions.eat = true,
            Role::Button(_) | Role::Ui => {}
        }
        if let Some(slot) = state.fingers.iter_mut().find(|(i, _)| *i == id) {
            slot.1 = finger;
        }
    }
    actions.movement = actions.movement.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
}

#[derive(Component)]
struct TouchUi;

#[derive(Component)]
struct TouchButtonUi(TouchButton);

#[derive(Component)]
struct JoystickBase;

#[derive(Component)]
struct JoystickKnob;

fn circle(radius: f32, color: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: px(radius * 2.0),
            height: px(radius * 2.0),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::MAX,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(color),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.5)),
        Pickable::IGNORE,
    )
}

fn spawn_touch_ui(mut commands: Commands) {
    let root = commands
        .spawn((
            Name::new("Touch controls"),
            TouchUi,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .id();
    commands.entity(root).with_children(|ui| {
        for b in BUTTONS {
            let (_, r) = button_layout(b);
            let label = match b {
                TouchButton::Jump => "Jump",
                TouchButton::Down => "Down",
                TouchButton::Fly => "Fly",
                TouchButton::Inventory => "Bag",
                TouchButton::Menu => "Menu",
                TouchButton::Eat => "Eat",
                TouchButton::View => "View",
            };
            ui.spawn((
                circle(r, Color::srgba(0.0, 0.0, 0.0, 0.3)),
                TouchButtonUi(b),
            ))
            .with_child((
                Text::new(label),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                Pickable::IGNORE,
            ));
        }
        ui.spawn((
            circle(JOYSTICK_RADIUS, Color::srgba(0.0, 0.0, 0.0, 0.25)),
            JoystickBase,
        ));
        ui.spawn((
            circle(28.0, Color::srgba(1.0, 1.0, 1.0, 0.45)),
            JoystickKnob,
        ));
    });
}

#[allow(clippy::too_many_arguments)]
fn update_touch_ui(
    controls: Res<TouchControls>,
    mut state: ResMut<TouchState>,
    focus: Res<UiFocus>,
    mode: Res<GameMode>,
    (vitals, items, inventory): (
        Res<crate::vitals::Vitals>,
        Res<crate::items::Items>,
        Res<PlayerInventory>,
    ),
    window: Single<&Window, With<PrimaryWindow>>,
    mut root: Single<&mut Visibility, With<TouchUi>>,
    mut buttons: Query<(&TouchButtonUi, &mut Node, &mut Visibility), Without<TouchUi>>,
    mut base: Single<
        &mut Node,
        (
            With<JoystickBase>,
            Without<JoystickKnob>,
            Without<TouchButtonUi>,
        ),
    >,
    mut knob: Single<
        &mut Node,
        (
            With<JoystickKnob>,
            Without<JoystickBase>,
            Without<TouchButtonUi>,
        ),
    >,
) {
    let held = inventory.held().and_then(|i| items.get(i));
    state.can_eat = vitals.shown() && held.is_some_and(|d| d.food > 0 || d.heals > 0)
        || held.is_some_and(|d| ourcraft_common::magic::potion_effect(&d.name).is_some());
    let show = controls.enabled && !focus.menu_open;
    **root = if show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if !show {
        return;
    }
    let size = window.size();
    for (b, mut node, mut vis) in &mut buttons {
        let (_, r) = button_layout(b.0);
        let c = button_center(b.0, size);
        node.left = px(c.x - r);
        node.top = px(c.y - r);
        let visible = match b.0 {
            TouchButton::Fly => *mode == GameMode::Creative && !focus.inventory_open,
            TouchButton::Inventory => true,
            TouchButton::Eat => state.can_eat && !focus.inventory_open,
            _ => !focus.inventory_open,
        };
        *vis = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    // The joystick rests at a default spot until a thumb lands somewhere.
    let rest = Vec2::new(130.0, size.y - 150.0);
    let (origin, tip) = state.joystick.unwrap_or((rest, rest));
    base.left = px(origin.x - JOYSTICK_RADIUS);
    base.top = px(origin.y - JOYSTICK_RADIUS);
    knob.left = px(tip.x - 28.0);
    knob.top = px(tip.y - 28.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_do_not_overlap_each_other_or_the_hotbar() {
        // Check on a small phone screen in landscape.
        let size = Vec2::new(740.0, 360.0);
        for (i, a) in BUTTONS.iter().enumerate() {
            for b in &BUTTONS[i + 1..] {
                let d = button_center(*a, size).distance(button_center(*b, size));
                assert!(
                    d > button_layout(*a).1 + button_layout(*b).1,
                    "{a:?} overlaps {b:?}"
                );
            }
            let c = button_center(*a, size);
            let r = button_layout(*a).1;
            let hb = hotbar_rect(size);
            let overlaps_hotbar = c.x + r > hb.min.x && c.x - r < hb.max.x && c.y + r > hb.min.y;
            assert!(!overlaps_hotbar, "{a:?} overlaps the hotbar");
        }
    }
}

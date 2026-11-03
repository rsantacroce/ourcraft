//! Menus: the title screen, world list and "new world" form, joining a
//! friend (local network list or an address), settings, and the pause menu.
//!
//! Each screen is rebuilt from scratch whenever something it shows changes.
//! That's the simplest way to do UI and fast enough for menus. Buttons carry
//! a [`Action`] that a single system carries out.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use ourcraft_common::protocol::{DEFAULT_PORT, sanitize_name};
use std::time::SystemTime;

use bevy::ui_widgets::{ControlOrientation, ScrollArea, Scrollbar, ScrollbarThumb};
use ourcraft_common::save::{WorldMeta, list_worlds};

use crate::audio::PlaySound;
use crate::input::{Actions, UiFocus};
use crate::lan::LanBrowser;
use crate::net::{Connection, SessionEnded, SinglePlayer, Status};
use crate::paths::{folder_name, saves_dir};
use crate::settings::{ClientSettings, GraphicsSettings};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Screen>()
            .init_resource::<Forms>()
            .init_resource::<WorldListScroll>()
            .add_systems(Startup, start_screen)
            .add_systems(PreUpdate, open_pause_menu.after(crate::input::ActionsSet))
            .add_systems(
                Update,
                (
                    watch_connection,
                    type_into_fields,
                    button_actions,
                    button_hover,
                    rebuild_menu,
                    remember_world_scroll,
                    report_text_focus,
                )
                    .chain(),
            );
    }
}

/// Tell the on-screen keyboard whether a field is being typed into.
fn report_text_focus(
    forms: Res<Forms>,
    screen: Res<Screen>,
    mut text: ResMut<crate::soft_keyboard::TextFocus>,
) {
    let typing = forms.focused.is_some() && *screen != Screen::None;
    if text.menu_field != typing {
        text.menu_field = typing;
    }
}

/// Which menu is showing.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub enum Screen {
    #[default]
    Title,
    Worlds,
    NewWorld,
    Join,
    Settings,
    /// Making your own skin (from Settings).
    SkinMaker,
    Pause,
    /// Lost the connection (with the reason).
    Disconnected(String),
    /// In game, no menu.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    WorldName,
    Seed,
    Address,
    PlayerName,
}

/// What's typed into the forms.
#[derive(Resource, Debug)]
struct Forms {
    world_name: String,
    seed: String,
    /// Which of [`GAME_MODES`] a new world gets.
    game_mode: usize,
    survival: bool,
    lan: bool,
    address: String,
    focused: Option<Field>,
    /// Shown at the top of a screen (e.g. "Couldn't connect…").
    message: Option<String>,
    /// Where Settings goes back to.
    settings_from: Screen,
    /// How the world list is ordered.
    sort: WorldSort,
    /// The world (folder) whose Delete was pressed once: asking "really?".
    confirm_delete: Option<String>,
}

/// Orders for the world list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum WorldSort {
    #[default]
    LastPlayed,
    Name,
    Newest,
}

impl WorldSort {
    fn next(self) -> Self {
        match self {
            Self::LastPlayed => Self::Name,
            Self::Name => Self::Newest,
            Self::Newest => Self::LastPlayed,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::LastPlayed => "last played",
            Self::Name => "name",
            Self::Newest => "newest",
        }
    }
}

/// How far the world list is scrolled, kept while the menu is rebuilt.
#[derive(Resource, Default)]
struct WorldListScroll(f32);

/// The scrolling list of worlds.
#[derive(Component)]
struct WorldList;

/// A red button (deleting).
#[derive(Component)]
struct Danger;

impl Default for Forms {
    fn default() -> Self {
        Self {
            world_name: String::new(),
            seed: String::new(),
            game_mode: 0,
            survival: false,
            lan: false,
            address: format!("localhost:{DEFAULT_PORT}"),
            focused: None,
            message: None,
            settings_from: Screen::Title,
            sort: WorldSort::default(),
            confirm_delete: None,
        }
    }
}

#[derive(Component, Debug, Clone, PartialEq)]
enum Action {
    Go(Screen),
    Play {
        folder: String,
        name: String,
    },
    NewWorld,
    CreateWorld,
    CycleSort,
    /// Delete pressed: ask first.
    AskDelete(String),
    /// "Yes, delete it."
    Delete(String),
    KeepWorld,
    CycleGameMode,
    ToggleSurvival,
    ToggleLan,
    RandomName,
    Focus(Field),
    JoinAddress,
    JoinLan(String),
    Resume,
    QuitToTitle,
    QuitGame,
    OpenSettings,
    CloseSettings,
    RenderDistance(i32),
    Sensitivity(i32),
    Volume(i32),
    CycleTouch,
    ToggleFilter,
    ToggleDebug,
    ToggleMusic,
    Skin(i32),
    OpenSkinMaker,
    /// Change one of the skin maker's choices (which, and which way).
    MakerPart(usize, i32),
    MakerRandom,
    TogglePick,
    ToggleWheel,
}

/// The games a new world can be: (mode name, what the button says).
const GAME_MODES: [(Option<&str>, &str); 3] = [
    (None, "Build (free play)"),
    (Some("team_shooter"), "Splat Squad (team flag game)"),
    (Some("build_contest"), "Build Contest (vote!)"),
];

/// A game mode's short name, for lists.
fn game_mode_name(mode: &str) -> &'static str {
    match mode {
        "team_shooter" => "Splat Squad",
        "build_contest" => "Build Contest",
        _ => "game mode",
    }
}

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuButton;

/// Start in game if the command line chose a world, otherwise at the title.
fn start_screen(mut screen: ResMut<Screen>, options: Option<Res<crate::net::LaunchOptions>>) {
    let options = options
        .map(|o| o.clone())
        .unwrap_or_else(crate::net::LaunchOptions::from_args);
    *screen = match options {
        crate::net::LaunchOptions::None => Screen::Title,
        _ => Screen::None,
    };
}

/// Esc (or the touch Menu button) opens the pause menu in game, and
/// closes it again.
fn open_pause_menu(
    keys: Res<ButtonInput<KeyCode>>,
    actions: Res<Actions>,
    focus: Res<UiFocus>,
    conn: Option<Res<Connection>>,
    mut screen: ResMut<Screen>,
) {
    let pressed = keys.just_pressed(KeyCode::Escape) || actions.open_menu;
    if !pressed || conn.is_none() || focus.chat_open || focus.inventory_open {
        return;
    }
    match *screen {
        Screen::None => *screen = Screen::Pause,
        Screen::Pause => *screen = Screen::None,
        _ => {}
    }
}

/// Show a "disconnected" screen when the connection drops.
fn watch_connection(conn: Option<Res<Connection>>, mut screen: ResMut<Screen>) {
    let Some(conn) = conn else { return };
    if let Status::Disconnected(reason) = &conn.status
        && matches!(*screen, Screen::None | Screen::Pause)
    {
        *screen = Screen::Disconnected(reason.clone());
    }
}

fn type_into_fields(
    mut keys: MessageReader<KeyboardInput>,
    mut forms: ResMut<Forms>,
    mut settings: ResMut<ClientSettings>,
    screen: Res<Screen>,
) {
    let Some(field) = forms.focused else {
        keys.clear();
        return;
    };
    if *screen == Screen::None {
        forms.focused = None;
        return;
    }
    for ev in keys.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        let text: &mut String = match field {
            Field::WorldName => &mut forms.world_name,
            Field::Seed => &mut forms.seed,
            Field::Address => &mut forms.address,
            Field::PlayerName => &mut settings.name,
        };
        match &crate::soft_keyboard::key_of(ev) {
            Key::Backspace => {
                text.pop();
            }
            Key::Enter | Key::Escape | Key::Tab => {
                forms.focused = None;
                if field == Field::PlayerName {
                    settings.name = sanitize_name(&settings.name);
                }
                return;
            }
            Key::Space => text.push(' '),
            Key::Character(s) if text.chars().count() < 40 => {
                let allowed: String = s
                    .chars()
                    .filter(|c| match field {
                        Field::Seed => c.is_ascii_digit(),
                        Field::Address => !c.is_whitespace(),
                        _ => c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ',
                    })
                    .collect();
                text.push_str(&allowed);
            }
            _ => {}
        }
    }
}

/// Fun names for new worlds.
fn random_world_name(seed: u64) -> String {
    const A: [&str; 10] = [
        "Sunny", "Bouncy", "Secret", "Giant", "Sleepy", "Shiny", "Mossy", "Windy", "Cosy", "Wild",
    ];
    const B: [&str; 10] = [
        "Valley", "Island", "Hills", "Meadow", "Canyon", "Forest", "Lagoon", "Peaks", "Shore",
        "Glade",
    ];
    let h = ourcraft_common::worldgen::splitmix64(seed);
    format!("{} {}", A[(h % 10) as usize], B[((h / 10) % 10) as usize])
}

/// A folder name that isn't taken yet.
fn free_folder(name: &str) -> String {
    let base = folder_name(name);
    let dir = saves_dir();
    if !dir.join(&base).exists() {
        return base;
    }
    (2..)
        .map(|i| format!("{base}_{i}"))
        .find(|f| !dir.join(f).exists())
        .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn button_actions(
    mut commands: Commands,
    buttons: Query<(&Interaction, &Action), Changed<Interaction>>,
    time: Res<Time<Real>>,
    mut screen: ResMut<Screen>,
    mut forms: ResMut<Forms>,
    mut settings: ResMut<ClientSettings>,
    conn: Option<Res<Connection>>,
    mut ended: MessageWriter<SessionEnded>,
    mut sounds: MessageWriter<PlaySound>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        sounds.write(PlaySound::ui("click"));
        let name = settings.name.clone();
        match action.clone() {
            Action::Go(s) => {
                forms.message = None;
                forms.focused = None;
                forms.confirm_delete = None;
                *screen = s;
            }
            Action::Play {
                folder,
                name: world,
            } => {
                let opts = SinglePlayer {
                    folder,
                    world_name: world,
                    seed: None,
                    lan: forms.lan,
                    mode: None,
                    creative: true,
                };
                commands.insert_resource(Connection::single_player(&opts, &name));
                *screen = Screen::None;
            }
            Action::NewWorld => {
                forms.world_name = random_world_name(time.elapsed().as_nanos() as u64);
                forms.seed.clear();
                *screen = Screen::NewWorld;
            }
            Action::RandomName => {
                forms.world_name = random_world_name(time.elapsed().as_nanos() as u64);
            }
            Action::CreateWorld => {
                let world = if forms.world_name.trim().is_empty() {
                    random_world_name(time.elapsed().as_nanos() as u64)
                } else {
                    forms.world_name.trim().to_owned()
                };
                let opts = SinglePlayer {
                    folder: free_folder(&world),
                    world_name: world,
                    seed: forms.seed.parse().ok(),
                    lan: forms.lan,
                    mode: GAME_MODES[forms.game_mode].0.map(str::to_owned),
                    // Game modes are played in creative.
                    creative: !forms.survival || forms.game_mode != 0,
                };
                commands.insert_resource(Connection::single_player(&opts, &name));
                forms.focused = None;
                *screen = Screen::None;
            }
            Action::CycleSort => forms.sort = forms.sort.next(),
            Action::AskDelete(folder) => forms.confirm_delete = Some(folder),
            Action::KeepWorld => forms.confirm_delete = None,
            Action::Delete(folder) => {
                forms.confirm_delete = None;
                forms.message = delete_world(&folder).err();
            }
            Action::CycleGameMode => forms.game_mode = (forms.game_mode + 1) % GAME_MODES.len(),
            Action::ToggleSurvival => forms.survival = !forms.survival,
            Action::ToggleLan => forms.lan = !forms.lan,
            Action::Focus(f) => forms.focused = Some(f),
            Action::JoinAddress | Action::JoinLan(_) => {
                let address = match action {
                    Action::JoinLan(a) => a.clone(),
                    _ => forms.address.trim().to_owned(),
                };
                match Connection::join(&address, &name) {
                    Ok(c) => {
                        commands.insert_resource(c);
                        forms.message = None;
                        *screen = Screen::None;
                    }
                    Err(e) => forms.message = Some(e),
                }
            }
            Action::Resume => *screen = Screen::None,
            Action::QuitToTitle => {
                // Dropping the connection saves and stops a single-player
                // world.
                if conn.is_some() {
                    commands.remove_resource::<Connection>();
                    ended.write(SessionEnded);
                }
                *screen = Screen::Title;
            }
            Action::QuitGame => {
                exit.write(AppExit::Success);
            }
            Action::OpenSettings => {
                forms.settings_from = screen.clone();
                *screen = Screen::Settings;
            }
            Action::CloseSettings => {
                settings.name = sanitize_name(&settings.name);
                forms.focused = None;
                *screen = forms.settings_from.clone();
            }
            Action::RenderDistance(d) => {
                settings.render_distance = (settings.render_distance + d).clamp(
                    GraphicsSettings::MIN_RENDER_DISTANCE,
                    GraphicsSettings::MAX_RENDER_DISTANCE,
                );
            }
            Action::Sensitivity(d) => {
                settings.mouse_sensitivity =
                    ((settings.mouse_sensitivity + d as f32 * 0.1) * 10.0).round() / 10.0;
                settings.mouse_sensitivity = settings.mouse_sensitivity.clamp(0.2, 3.0);
            }
            Action::Volume(d) => {
                settings.volume = ((settings.volume + d as f32 * 0.1) * 10.0).round() / 10.0;
                settings.volume = settings.volume.clamp(0.0, 1.0);
            }
            Action::CycleTouch => {
                settings.touch_controls = match settings.touch_controls {
                    None => Some(true),
                    Some(true) => Some(false),
                    Some(false) => None,
                }
            }
            Action::ToggleFilter => settings.chat_filter = !settings.chat_filter,
            Action::ToggleMusic => settings.music = !settings.music,
            Action::ToggleDebug => settings.show_debug = !settings.show_debug,
            Action::Skin(d) => {
                settings.skin = crate::skins::cycle_skin(&settings.skin, d, &settings.made_skin)
            }
            Action::OpenSkinMaker => {
                // Wear the made skin while making it.
                settings.skin = settings.made_skin.clone();
                *screen = Screen::SkinMaker;
            }
            Action::MakerPart(part, d) => {
                use crate::render::skin_textures::{
                    MAKER_PARTS, made_skin_choices, made_skin_name,
                };
                let mut choices = made_skin_choices(&settings.made_skin).unwrap_or([0; 9]);
                let n = MAKER_PARTS[part].1.len() as i32;
                choices[part] = (choices[part] as i32 + d).rem_euclid(n) as u8;
                settings.made_skin = made_skin_name(choices);
                settings.skin = settings.made_skin.clone();
            }
            Action::MakerRandom => {
                use crate::render::skin_textures::{MAKER_PARTS, made_skin_name};
                let mut r = ourcraft_common::worldgen::splitmix64(
                    time.elapsed().as_nanos() as u64 ^ 0x736b_696e,
                );
                let choices = std::array::from_fn(|i| {
                    r = ourcraft_common::worldgen::splitmix64(r);
                    // Hats now and then, not every time.
                    let n = if i == 8 && !r.is_multiple_of(3) {
                        1
                    } else {
                        MAKER_PARTS[i].1.len() as u64
                    };
                    ((r >> 16) % n) as u8
                });
                settings.made_skin = made_skin_name(choices);
                settings.skin = settings.made_skin.clone();
            }
            Action::TogglePick => settings.middle_click_pick = !settings.middle_click_pick,
            Action::ToggleWheel => settings.wheel_hotbar = !settings.wheel_hotbar,
        }
    }
}

const BUTTON: Color = Color::srgb(0.24, 0.42, 0.28);
const BUTTON_HOVER: Color = Color::srgb(0.32, 0.55, 0.36);
const BUTTON_PRESSED: Color = Color::srgb(0.18, 0.32, 0.2);
const DANGER: Color = Color::srgb(0.6, 0.22, 0.2);
const DANGER_HOVER: Color = Color::srgb(0.78, 0.3, 0.26);
const DANGER_PRESSED: Color = Color::srgb(0.45, 0.16, 0.14);

fn button_hover(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor, Has<Danger>),
        (Changed<Interaction>, With<MenuButton>),
    >,
) {
    for (interaction, mut bg, danger) in &mut buttons {
        bg.0 = match (interaction, danger) {
            (Interaction::Pressed, false) => BUTTON_PRESSED,
            (Interaction::Hovered, false) => BUTTON_HOVER,
            (Interaction::None, false) => BUTTON,
            (Interaction::Pressed, true) => DANGER_PRESSED,
            (Interaction::Hovered, true) => DANGER_HOVER,
            (Interaction::None, true) => DANGER,
        };
    }
}

/// Delete a saved world's folder (after the player said yes).
fn delete_world(folder: &str) -> Result<(), String> {
    // Only ever a plain folder name straight inside the saves folder.
    let plain =
        !folder.is_empty() && folder != "." && folder != ".." && !folder.contains(['/', '\\', ':']);
    let dir = saves_dir().join(folder);
    if !plain || !dir.join("world.dat").is_file() {
        return Err(format!("Couldn't delete \"{folder}\": it isn't a world."));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("Couldn't delete the world: {e}"))
}

/// When a world was last saved (played), and when it was made.
fn world_times(folder: &str) -> (Option<SystemTime>, Option<SystemTime>) {
    let dir = saves_dir().join(folder);
    let saved = std::fs::metadata(dir.join("world.dat"))
        .and_then(|m| m.modified())
        .ok();
    let made = std::fs::metadata(&dir)
        .and_then(|m| m.created().or_else(|_| m.modified()))
        .ok();
    (saved, made)
}

/// "today", "yesterday", "3 days ago"…
fn days_ago(t: SystemTime) -> String {
    let days = SystemTime::now()
        .duration_since(t)
        .map_or(0, |d| d.as_secs() / 86_400);
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..=13 => format!("{days} days ago"),
        14..=59 => format!("{} weeks ago", days / 7),
        _ => format!("{} months ago", days / 30),
    }
}

/// Keep the world list's scroll position across menu rebuilds.
fn remember_world_scroll(
    lists: Query<&ScrollPosition, (With<WorldList>, Changed<ScrollPosition>)>,
    mut saved: ResMut<WorldListScroll>,
) {
    for pos in &lists {
        saved.0 = pos.y;
    }
}

fn button(parent: &mut ChildSpawnerCommands, label: impl Into<String>, action: Action) {
    button_sized(parent, label, action, 320.0);
}

fn button_sized(
    parent: &mut ChildSpawnerCommands,
    label: impl Into<String>,
    action: Action,
    width: f32,
) {
    button_node(parent, label, action, width);
}

/// A button; returns its entity so callers can restyle it.
fn button_node(
    parent: &mut ChildSpawnerCommands,
    label: impl Into<String>,
    action: Action,
    width: f32,
) -> Entity {
    parent
        .spawn((
            Button,
            MenuButton,
            action,
            Node {
                width: px(width),
                min_height: px(46),
                padding: UiRect::axes(px(12), px(8)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(BUTTON),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.35)),
        ))
        .with_child((
            Text::new(label),
            TextFont::from_font_size(19.0),
            TextColor(Color::WHITE),
            Pickable::IGNORE,
        ))
        .id()
}

fn text_field(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    value: &str,
    field: Field,
    focused: bool,
) {
    parent.spawn((
        Text::new(label),
        TextFont::from_font_size(15.0),
        TextColor(Color::srgb(0.8, 0.85, 0.9)),
    ));
    let shown = if focused {
        format!("{value}_")
    } else if value.is_empty() {
        "(click to type)".into()
    } else {
        value.to_owned()
    };
    parent
        .spawn((
            Button,
            Action::Focus(field),
            Node {
                width: px(320),
                min_height: px(40),
                padding: UiRect::axes(px(10), px(6)),
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            BorderColor::all(if focused {
                Color::srgb(1.0, 0.85, 0.3)
            } else {
                Color::srgba(1.0, 1.0, 1.0, 0.3)
            }),
        ))
        .with_child((
            Text::new(shown),
            TextFont::from_font_size(18.0),
            TextColor(Color::WHITE),
            Pickable::IGNORE,
        ));
}

/// A setting with − and + buttons either side.
fn setting_row(ui: &mut ChildSpawnerCommands, label: String, minus: Action, plus: Action) {
    setting_row_sized(ui, label, minus, plus, 300.0);
}

fn setting_row_sized(
    ui: &mut ChildSpawnerCommands,
    label: String,
    minus: Action,
    plus: Action,
    width: f32,
) {
    ui.spawn(Node {
        column_gap: px(8),
        align_items: AlignItems::Center,
        ..default()
    })
    .with_children(|r| {
        button_sized(r, "-", minus, 50.0);
        r.spawn(Node {
            width: px(width),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_child((
            Text::new(label),
            TextFont::from_font_size(18.0),
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
        ));
        button_sized(r, "+", plus, 50.0);
    });
}

/// The skin you're wearing, from the front.
fn skin_preview(ui: &mut ChildSpawnerCommands, preview: &crate::skins::SkinPreview, scale: f32) {
    ui.spawn((
        ImageNode::new(preview.0.clone()),
        Node {
            width: px(16.0 * scale),
            height: px(32.0 * scale),
            ..default()
        },
        Pickable::IGNORE,
    ));
}

fn heading(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((
        Text::new(text),
        TextFont::from_font_size(34.0),
        TextColor(Color::WHITE),
        TextShadow {
            offset: Vec2::splat(3.0),
            color: Color::srgba(0.0, 0.0, 0.0, 0.7),
        },
        Node {
            margin: UiRect::bottom(px(8)),
            ..default()
        },
    ));
}

fn note(parent: &mut ChildSpawnerCommands, text: impl Into<String>, color: Color) {
    parent.spawn((
        Text::new(text),
        TextFont::from_font_size(15.0),
        TextColor(color),
    ));
}

/// The worlds, in a box that scrolls (mouse wheel, or drag the bar), each
/// with its Delete button.
fn world_list(
    ui: &mut ChildSpawnerCommands,
    worlds: Vec<(String, WorldMeta, Option<SystemTime>, Option<SystemTime>)>,
    forms: &Forms,
    scroll: f32,
) {
    ui.spawn(Node {
        display: Display::Grid,
        width: px(530),
        max_width: percent(96),
        // About six worlds tall, but never more than half the screen.
        height: px(330),
        max_height: vh(50),
        grid_template_columns: vec![RepeatedGridTrack::flex(1, 1.0), RepeatedGridTrack::auto(1)],
        grid_template_rows: vec![RepeatedGridTrack::flex(1, 1.0)],
        column_gap: px(6),
        ..default()
    })
    .with_children(|frame| {
        let list = frame
            .spawn((
                WorldList,
                ScrollArea,
                ScrollPosition(Vec2::new(0.0, scroll)),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    padding: UiRect::all(px(6)),
                    overflow: Overflow::scroll_y(),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.22)),
            ))
            .with_children(|list| {
                for (folder, meta, saved, _) in worlds {
                    let mode = match meta.game_mode.as_deref() {
                        Some(m) => game_mode_name(m),
                        None if meta.creative => "creative",
                        None => "survival",
                    };
                    let when = saved.map(days_ago).unwrap_or_default();
                    list.spawn(Node {
                        column_gap: px(6),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        if forms.confirm_delete.as_deref() == Some(folder.as_str()) {
                            row.spawn((
                                Text::new(format!("Delete \"{}\" for good?", meta.name)),
                                TextFont::from_font_size(16.0),
                                TextColor(Color::WHITE),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                            ));
                            let yes = button_node(row, "Delete", Action::Delete(folder), 100.0);
                            row.commands()
                                .entity(yes)
                                .insert((Danger, BackgroundColor(DANGER)));
                            button_node(row, "Keep", Action::KeepWorld, 90.0);
                        } else {
                            let play = button_node(
                                row,
                                format!("{}\n{mode}, played {when}", meta.name),
                                Action::Play {
                                    folder: folder.clone(),
                                    name: meta.name,
                                },
                                400.0,
                            );
                            row.commands().entity(play).insert(Node {
                                flex_grow: 1.0,
                                min_height: px(54),
                                padding: UiRect::axes(px(12), px(6)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(2)),
                                border_radius: BorderRadius::all(px(8)),
                                ..default()
                            });
                            let delete =
                                button_node(row, "Delete", Action::AskDelete(folder), 80.0);
                            row.commands()
                                .entity(delete)
                                .insert((Danger, BackgroundColor(DANGER)));
                        }
                    });
                }
            })
            .id();
        frame
            .spawn((
                Node {
                    width: px(12),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.2)),
                Scrollbar::new(list, ControlOrientation::Vertical, 30.0),
            ))
            .with_child((
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                ScrollbarThumb {
                    border_radius: BorderRadius::all(px(6)),
                    border: UiRect::ZERO,
                },
            ));
    });
}

fn onoff(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_menu(
    mut commands: Commands,
    screen: Res<Screen>,
    forms: Res<Forms>,
    settings: Res<ClientSettings>,
    my_skin: Res<crate::skins::MySkin>,
    mut lan: ResMut<LanBrowser>,
    mut focus: ResMut<UiFocus>,
    existing: Query<Entity, With<MenuRoot>>,
    scroll: Res<WorldListScroll>,
    preview: Option<Res<crate::skins::SkinPreview>>,
) {
    // The local network list only listens while the Join screen is open.
    let want_lan = *screen == Screen::Join;
    if lan.active != want_lan {
        lan.active = want_lan;
    }
    let menu_open = *screen != Screen::None;
    if focus.menu_open != menu_open {
        focus.menu_open = menu_open;
    }
    if !screen.is_changed()
        && !forms.is_changed()
        && !settings.is_changed()
        && !lan.is_changed()
        && !my_skin.is_changed()
    {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !menu_open {
        return;
    }

    // In game (pause), the world stays visible behind a dark veil; on the
    // title screens a sky-blue background.
    let in_game = matches!(*screen, Screen::Pause | Screen::Disconnected(_))
        || (matches!(*screen, Screen::Settings | Screen::SkinMaker)
            && forms.settings_from == Screen::Pause);
    let bg = if in_game {
        Color::srgba(0.0, 0.0, 0.0, 0.55)
    } else {
        Color::srgb(0.36, 0.6, 0.86)
    };
    let mobile = cfg!(any(target_os = "ios", target_os = "android"));
    let red = Color::srgb(1.0, 0.55, 0.5);
    let grey = Color::srgb(0.8, 0.85, 0.9);

    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(10),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(bg),
            GlobalZIndex(10),
        ))
        .with_children(|ui| {
            if let Some(msg) = &forms.message {
                note(ui, msg.clone(), red);
            }
            match &*screen {
                Screen::Title => {
                    ui.spawn((
                        Text::new("OURCRAFT"),
                        TextFont::from_font_size(72.0),
                        TextColor(Color::srgb(1.0, 0.95, 0.75)),
                        TextShadow {
                            offset: Vec2::splat(5.0),
                            color: Color::srgb(0.25, 0.18, 0.1),
                        },
                    ));
                    note(ui, "Build, explore and play together", Color::WHITE);
                    ui.spawn(Node {
                        height: px(16),
                        ..default()
                    });
                    button(ui, "Play", Action::Go(Screen::Worlds));
                    button(ui, "Join a friend", Action::Go(Screen::Join));
                    button(ui, "Settings", Action::OpenSettings);
                    if !mobile {
                        button(ui, "Quit", Action::QuitGame);
                    }
                    note(ui, format!("Playing as {}", settings.name), Color::WHITE);
                }
                Screen::Worlds => {
                    heading(ui, "Your worlds");
                    ui.spawn(Node {
                        column_gap: px(10),
                        ..default()
                    })
                    .with_children(|row| {
                        button_sized(row, "+ New world", Action::NewWorld, 250.0);
                        button_sized(
                            row,
                            format!("Sort: {}", forms.sort.label()),
                            Action::CycleSort,
                            250.0,
                        );
                    });
                    let mut worlds: Vec<_> = list_worlds(&saves_dir())
                        .into_iter()
                        .map(|(folder, meta)| {
                            let (saved, made) = world_times(&folder);
                            (folder, meta, saved, made)
                        })
                        .collect();
                    match forms.sort {
                        WorldSort::LastPlayed => worlds.sort_by_key(|w| std::cmp::Reverse(w.2)),
                        WorldSort::Name => worlds.sort_by_key(|w| w.1.name.to_lowercase()),
                        WorldSort::Newest => worlds.sort_by_key(|w| std::cmp::Reverse(w.3)),
                    }
                    if worlds.is_empty() {
                        note(ui, "No worlds yet. Make one!", grey);
                    } else {
                        world_list(ui, worlds, &forms, scroll.0);
                    }
                    button_sized(
                        ui,
                        format!("Friends on my network can join: {}", onoff(forms.lan)),
                        Action::ToggleLan,
                        510.0,
                    );
                    button(ui, "Back", Action::Go(Screen::Title));
                }
                Screen::NewWorld => {
                    heading(ui, "New world");
                    let f = forms.focused;
                    text_field(
                        ui,
                        "Name",
                        &forms.world_name,
                        Field::WorldName,
                        f == Some(Field::WorldName),
                    );
                    button(ui, "Pick a random name", Action::RandomName);
                    text_field(
                        ui,
                        "Seed (optional number)",
                        &forms.seed,
                        Field::Seed,
                        f == Some(Field::Seed),
                    );
                    let mode = format!("Game: {}", GAME_MODES[forms.game_mode].1);
                    button_sized(ui, mode, Action::CycleGameMode, 420.0);
                    if forms.game_mode == 0 {
                        let survival = if forms.survival {
                            "Survival: collect what you break"
                        } else {
                            "Creative: fly, endless blocks"
                        };
                        button_sized(ui, survival, Action::ToggleSurvival, 420.0);
                    }
                    button_sized(
                        ui,
                        format!("Friends on my network can join: {}", onoff(forms.lan)),
                        Action::ToggleLan,
                        480.0,
                    );
                    button(ui, "Create and play", Action::CreateWorld);
                    button(ui, "Back", Action::Go(Screen::Worlds));
                }
                Screen::Join => {
                    heading(ui, "Join a friend");
                    note(ui, "Games on your network:", Color::WHITE);
                    let servers = lan.list();
                    if let Some(problem) = &lan.problem {
                        note(ui, problem.clone(), red);
                    } else if servers.is_empty() {
                        note(
                            ui,
                            "Looking...  (your friend needs \"Friends on my network can join\" on)",
                            grey,
                        );
                    }
                    for s in servers {
                        let mode = s
                            .beacon
                            .game_mode
                            .as_deref()
                            .map(|m| format!(" - {}", game_mode_name(m)))
                            .unwrap_or_default();
                        button_sized(
                            ui,
                            format!(
                                "{}{mode}  ({}/{} players)",
                                s.beacon.name, s.beacon.players, s.beacon.max_players
                            ),
                            Action::JoinLan(s.address.to_string()),
                            460.0,
                        );
                    }
                    ui.spawn(Node {
                        height: px(10),
                        ..default()
                    });
                    let f = forms.focused;
                    text_field(
                        ui,
                        "Or type an address (host or host:port)",
                        &forms.address,
                        Field::Address,
                        f == Some(Field::Address),
                    );
                    button(ui, "Join", Action::JoinAddress);
                    button(ui, "Back", Action::Go(Screen::Title));
                }
                Screen::Settings => {
                    heading(ui, "Settings");
                    let f = forms.focused;
                    text_field(
                        ui,
                        "Your name",
                        &settings.name,
                        Field::PlayerName,
                        f == Some(Field::PlayerName),
                    );
                    let row = setting_row;
                    // Two columns where there's room (one on narrow screens),
                    // so everything fits without scrolling.
                    let columns = Node {
                        flex_wrap: FlexWrap::Wrap,
                        justify_content: JustifyContent::Center,
                        column_gap: px(16),
                        row_gap: px(10),
                        max_width: px(880),
                        ..default()
                    };
                    let touch = match settings.touch_controls {
                        None => "automatic",
                        Some(true) => "on",
                        Some(false) => "off",
                    };
                    ui.spawn(columns.clone()).with_children(|ui| {
                        row(
                            ui,
                            format!("Skin: {}", crate::skins::skin_label(&settings.skin)),
                            Action::Skin(-1),
                            Action::Skin(1),
                        );
                        row(
                            ui,
                            format!("View distance: {} chunks", settings.render_distance),
                            Action::RenderDistance(-1),
                            Action::RenderDistance(1),
                        );
                        row(
                            ui,
                            format!("Look sensitivity: {:.1}", settings.mouse_sensitivity),
                            Action::Sensitivity(-1),
                            Action::Sensitivity(1),
                        );
                        row(
                            ui,
                            format!("Sound volume: {:.0}%", settings.volume * 100.0),
                            Action::Volume(-1),
                            Action::Volume(1),
                        );
                    });
                    ui.spawn(Node {
                        column_gap: px(16),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|ui| {
                        if let Some(p) = &preview {
                            skin_preview(ui, p, 2.0);
                        }
                        button(ui, "Make your own skin", Action::OpenSkinMaker);
                    });
                    if let Some(problem) = &my_skin.problem {
                        note(ui, problem.clone(), red);
                    } else if !mobile {
                        note(
                            ui,
                            format!(
                                "Your own skins: 64x64 PNG files in {}",
                                crate::skins::skins_dir().display()
                            ),
                            grey,
                        );
                    }
                    ui.spawn(columns).with_children(|ui| {
                        let toggles = [
                            (format!("Touch controls: {touch}"), Action::CycleTouch),
                            (
                                format!("Hide rude words in chat: {}", onoff(settings.chat_filter)),
                                Action::ToggleFilter,
                            ),
                            (
                                format!("Music: {}", onoff(settings.music)),
                                Action::ToggleMusic,
                            ),
                            (
                                format!(
                                    "Show info text (FPS, position): {}",
                                    onoff(settings.show_debug)
                                ),
                                Action::ToggleDebug,
                            ),
                            (
                                format!(
                                    "Middle click picks block: {}",
                                    onoff(settings.middle_click_pick)
                                ),
                                Action::TogglePick,
                            ),
                            (
                                format!(
                                    "Mouse wheel changes slot: {}",
                                    onoff(settings.wheel_hotbar)
                                ),
                                Action::ToggleWheel,
                            ),
                        ];
                        for (label, action) in toggles {
                            button_sized(ui, label, action, 420.0);
                        }
                    });
                    button(ui, "Done", Action::CloseSettings);
                }
                Screen::SkinMaker => {
                    use crate::render::skin_textures::{MAKER_PARTS, made_skin_choices};
                    heading(ui, "Make your own skin");
                    let choices = made_skin_choices(&settings.made_skin).unwrap_or([0; 9]);
                    ui.spawn(Node {
                        column_gap: px(24),
                        align_items: AlignItems::Center,
                        flex_wrap: FlexWrap::Wrap,
                        justify_content: JustifyContent::Center,
                        ..default()
                    })
                    .with_children(|ui| {
                        if let Some(p) = &preview {
                            skin_preview(ui, p, 5.0);
                        }
                        // Two columns of choices, so it all fits on a phone.
                        ui.spawn(Node {
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(16),
                            row_gap: px(6),
                            max_width: px(730),
                            ..default()
                        })
                        .with_children(|ui| {
                            for (i, (part, options)) in MAKER_PARTS.iter().enumerate() {
                                setting_row_sized(
                                    ui,
                                    format!("{part}: {}", options[choices[i] as usize]),
                                    Action::MakerPart(i, -1),
                                    Action::MakerPart(i, 1),
                                    240.0,
                                );
                            }
                        });
                    });
                    ui.spawn(Node {
                        column_gap: px(12),
                        ..default()
                    })
                    .with_children(|ui| {
                        button_sized(ui, "Surprise me!", Action::MakerRandom, 200.0);
                        button_sized(ui, "Done", Action::Go(Screen::Settings), 200.0);
                    });
                }
                Screen::Pause => {
                    heading(ui, "Paused");
                    button(ui, "Back to game", Action::Resume);
                    button(ui, "Settings", Action::OpenSettings);
                    button(ui, "Save and quit to title", Action::QuitToTitle);
                }
                Screen::Disconnected(reason) => {
                    heading(ui, "Disconnected");
                    note(ui, reason.clone(), Color::WHITE);
                    button(ui, "Back to title", Action::QuitToTitle);
                }
                Screen::None => {}
            }
        });

    // On phones and tablets the on-screen keyboard covers the bottom half,
    // where the field being typed in may be: show it again up top.
    if let (true, Some(field)) = (mobile, forms.focused) {
        let (label, value) = match field {
            Field::WorldName => ("Name", forms.world_name.as_str()),
            Field::Seed => ("Seed", forms.seed.as_str()),
            Field::Address => ("Address", forms.address.as_str()),
            Field::PlayerName => ("Your name", settings.name.as_str()),
        };
        commands.spawn((
            MenuRoot,
            Text::new(format!("{label}: {value}_")),
            TextFont::from_font_size(20.0),
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                top: px(12),
                left: px(16),
                right: px(16),
                padding: UiRect::axes(px(12), px(8)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.1, 0.12, 0.2, 0.95)),
            BorderColor::all(Color::srgb(1.0, 0.85, 0.3)),
            GlobalZIndex(11),
            Pickable::IGNORE,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_names_look_nice() {
        let n = random_world_name(42);
        assert!(n.contains(' ') && n.len() > 5);
        assert_eq!(n, random_world_name(42));
    }
}

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
use ourcraft_common::save::list_worlds;

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
                )
                    .chain(),
            );
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
    splat_squad: bool,
    survival: bool,
    lan: bool,
    address: String,
    focused: Option<Field>,
    /// Shown at the top of a screen (e.g. "Couldn't connect…").
    message: Option<String>,
    /// Where Settings goes back to.
    settings_from: Screen,
}

impl Default for Forms {
    fn default() -> Self {
        Self {
            world_name: String::new(),
            seed: String::new(),
            splat_squad: false,
            survival: false,
            lan: false,
            address: format!("localhost:{DEFAULT_PORT}"),
            focused: None,
            message: None,
            settings_from: Screen::Title,
        }
    }
}

#[derive(Component, Debug, Clone, PartialEq)]
enum Action {
    Go(Screen),
    Play { folder: String, name: String },
    NewWorld,
    CreateWorld,
    ToggleSplat,
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
        match &ev.logical_key {
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
                    mode: forms.splat_squad.then(|| "team_shooter".to_owned()),
                    creative: !forms.survival,
                };
                commands.insert_resource(Connection::single_player(&opts, &name));
                forms.focused = None;
                *screen = Screen::None;
            }
            Action::ToggleSplat => forms.splat_squad = !forms.splat_squad,
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
            Action::ToggleDebug => settings.show_debug = !settings.show_debug,
        }
    }
}

const BUTTON: Color = Color::srgb(0.24, 0.42, 0.28);
const BUTTON_HOVER: Color = Color::srgb(0.32, 0.55, 0.36);
const BUTTON_PRESSED: Color = Color::srgb(0.18, 0.32, 0.2);

fn button_hover(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<MenuButton>),
    >,
) {
    for (interaction, mut bg) in &mut buttons {
        bg.0 = match interaction {
            Interaction::Pressed => BUTTON_PRESSED,
            Interaction::Hovered => BUTTON_HOVER,
            Interaction::None => BUTTON,
        };
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
        ));
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

fn onoff(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_menu(
    mut commands: Commands,
    screen: Res<Screen>,
    forms: Res<Forms>,
    settings: Res<ClientSettings>,
    mut lan: ResMut<LanBrowser>,
    mut focus: ResMut<UiFocus>,
    existing: Query<Entity, With<MenuRoot>>,
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
    if !screen.is_changed() && !forms.is_changed() && !settings.is_changed() && !lan.is_changed() {
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
        || (*screen == Screen::Settings && forms.settings_from == Screen::Pause);
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
                    note(ui, format!("Playing as {}", settings.name), grey);
                }
                Screen::Worlds => {
                    heading(ui, "Your worlds");
                    button(ui, "+ New world", Action::NewWorld);
                    let worlds = list_worlds(&saves_dir());
                    if worlds.is_empty() {
                        note(ui, "No worlds yet. Make one!", grey);
                    }
                    for (folder, meta) in worlds.into_iter().take(12) {
                        let mode = match meta.game_mode.as_deref() {
                            Some("team_shooter") => " (Splat Squad)",
                            Some(_) => " (game mode)",
                            None if meta.creative => "",
                            None => " (survival)",
                        };
                        button(
                            ui,
                            format!("{}{mode}", meta.name),
                            Action::Play {
                                folder,
                                name: meta.name,
                            },
                        );
                    }
                    button_sized(
                        ui,
                        format!("Friends on my network can join: {}", onoff(forms.lan)),
                        Action::ToggleLan,
                        420.0,
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
                    let mode = if forms.splat_squad {
                        "Game: Splat Squad (team flag game)"
                    } else {
                        "Game: Build (free play)"
                    };
                    button_sized(ui, mode, Action::ToggleSplat, 420.0);
                    if !forms.splat_squad {
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
                        420.0,
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
                            "Looking…  (your friend needs \"Friends on my network can join\" on)",
                            grey,
                        );
                    }
                    for s in servers {
                        let mode = if s.beacon.game_mode.as_deref() == Some("team_shooter") {
                            " - Splat Squad"
                        } else {
                            ""
                        };
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
                    let row = |ui: &mut ChildSpawnerCommands,
                               label: String,
                               minus: Action,
                               plus: Action| {
                        ui.spawn(Node {
                            column_gap: px(8),
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|r| {
                            button_sized(r, "-", minus, 50.0);
                            r.spawn((
                                Text::new(label),
                                TextFont::from_font_size(18.0),
                                TextColor(Color::WHITE),
                                Node {
                                    width: px(300),
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                            ));
                            button_sized(r, "+", plus, 50.0);
                        });
                    };
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
                    let touch = match settings.touch_controls {
                        None => "automatic",
                        Some(true) => "on",
                        Some(false) => "off",
                    };
                    button_sized(
                        ui,
                        format!("Touch controls: {touch}"),
                        Action::CycleTouch,
                        420.0,
                    );
                    button_sized(
                        ui,
                        format!("Hide rude words in chat: {}", onoff(settings.chat_filter)),
                        Action::ToggleFilter,
                        420.0,
                    );
                    button_sized(
                        ui,
                        format!(
                            "Show info text (FPS, position): {}",
                            onoff(settings.show_debug)
                        ),
                        Action::ToggleDebug,
                        420.0,
                    );
                    button(ui, "Done", Action::CloseSettings);
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

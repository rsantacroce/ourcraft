//! The Ourcraft client: rendering, input and (for now) the whole game.

pub mod audio;
pub mod chat;
pub mod entities;
pub mod gamemode;

pub mod hud;
pub mod input;
pub mod interaction;
pub mod inventory_ui;
pub mod items;
pub mod lan;
pub mod menu;
pub mod meshing;
pub mod net;
pub mod paths;
pub mod player;
pub mod profanity;
pub mod remote_players;
pub mod render;
pub mod settings;
pub mod sky;
pub mod survival;
pub mod touch;
pub mod world;

use bevy::prelude::*;

/// Everything the client adds on top of Bevy's default plugins.
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.53, 0.75, 0.96)))
            .add_plugins((
                input::InputPlugin,
                touch::TouchPlugin,
                settings::SettingsPlugin,
                render::VoxelRenderPlugin,
                world::WorldPlugin,
                net::NetPlugin,
                remote_players::RemotePlayersPlugin,
                player::PlayerPlugin,
                interaction::InteractionPlugin,
                inventory_ui::InventoryUiPlugin,
                sky::SkyPlugin,
                hud::HudPlugin,
                chat::ChatPlugin,
                gamemode::GameModePlugin,
            ))
            // (Bevy accepts at most 15 plugins per call.)
            .add_plugins((
                menu::MenuPlugin,
                audio::SoundPlugin,
                lan::LanPlugin,
                entities::EntitiesPlugin,
                items::ItemsPlugin,
                survival::SurvivalPlugin,
            ));
    }
}

/// The main window. Desktop gets a normal resizable window; phones and
/// tablets get the whole screen.
fn primary_window() -> Window {
    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        use bevy::window::{ScreenEdge, WindowMode};
        Window {
            title: "Ourcraft".into(),
            resizable: false,
            mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
            // iOS: hide the home bar and status bar, and make swipes from
            // the bottom edge go to the game first.
            prefers_home_indicator_hidden: true,
            prefers_status_bar_hidden: true,
            preferred_screen_edges_deferring_system_gestures: ScreenEdge::Bottom,
            ..default()
        }
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        use bevy::window::{PresentMode, WindowResolution};
        Window {
            title: "Ourcraft".into(),
            resolution: WindowResolution::new(1280, 720),
            present_mode: PresentMode::AutoVsync,
            ..default()
        }
    }
}

/// Build and run the game. Shared by the desktop binary and the mobile
/// launchers.
pub fn run() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(primary_window()),
            ..default()
        }))
        .add_plugins(ClientPlugin)
        .run();
}

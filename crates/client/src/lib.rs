//! The Ourcraft client: rendering, input and (for now) the whole game.

pub mod hud;
pub mod input;
pub mod interaction;
pub mod inventory_ui;
pub mod meshing;
pub mod player;
pub mod render;
pub mod settings;
pub mod sky;
pub mod world;

use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};

/// Everything the client adds on top of Bevy's default plugins.
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.53, 0.75, 0.96)))
            .add_plugins((
                input::InputPlugin,
                settings::SettingsPlugin,
                render::VoxelRenderPlugin,
                world::WorldPlugin,
                player::PlayerPlugin,
                interaction::InteractionPlugin,
                inventory_ui::InventoryUiPlugin,
                sky::SkyPlugin,
                hud::HudPlugin,
            ));
    }
}

/// Build and run the game. Shared by the desktop binary and (from M4) the
/// mobile launchers.
pub fn run() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Ourcraft".into(),
                resolution: WindowResolution::new(1280, 720),
                // Vsync without the input lag of plain Fifo where supported.
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(ClientPlugin)
        .run();
}

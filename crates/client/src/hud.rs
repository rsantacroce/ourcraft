//! On-screen text: controls help, FPS and (F3) debug information.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::settings::GraphicsSettings;
use crate::world::{ChunkStreaming, ChunkViewer, ClientWorld};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, (toggle_debug, update_debug_text));
    }
}

#[derive(Component)]
struct DebugText;

#[derive(Component)]
struct HelpText;

fn spawn_hud(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(10),
            left: px(10),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        })
        .with_children(|parent| {
            parent.spawn((
                Text::new(""),
                TextFont::from_font_size(16.0),
                TextColor(Color::WHITE),
                TextShadow {
                    offset: Vec2::splat(1.5),
                    color: Color::linear_rgba(0.0, 0.0, 0.0, 0.8),
                },
                DebugText,
            ));
            parent.spawn((
                Text::new(
                    "Click: capture mouse   Esc: release   F3: debug info\n\
                     WASD: move   Space/Shift: up/down   Ctrl: fast\n\
                     [ and ]: render distance",
                ),
                TextFont::from_font_size(14.0),
                TextColor(Color::WHITE),
                TextShadow {
                    offset: Vec2::splat(1.5),
                    color: Color::linear_rgba(0.0, 0.0, 0.0, 0.8),
                },
                HelpText,
            ));
        });
}

fn toggle_debug(
    keys: Res<ButtonInput<KeyCode>>,
    mut texts: Query<&mut Visibility, Or<(With<DebugText>, With<HelpText>)>>,
) {
    if keys.just_pressed(KeyCode::F3) {
        for mut v in &mut texts {
            v.toggle_visible_hidden();
        }
    }
}

fn update_debug_text(
    diagnostics: Res<DiagnosticsStore>,
    viewer: Single<&Transform, With<ChunkViewer>>,
    world: Res<ClientWorld>,
    streaming: Res<ChunkStreaming>,
    settings: Res<GraphicsSettings>,
    mut text: Single<&mut Text, With<DebugText>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let p = viewer.translation;
    text.0 = format!(
        "FPS: {fps:.0}\n\
         Position: {:.1} / {:.1} / {:.1}\n\
         Chunks: {} loaded, {} meshes, {} generating, {} to mesh\n\
         Render distance: {}",
        p.x,
        p.y,
        p.z,
        world.chunk_count(),
        streaming.mesh_count(),
        streaming.pending_generation(),
        streaming.pending_meshes(),
        settings.render_distance,
    );
}

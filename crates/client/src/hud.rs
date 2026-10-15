//! On-screen text: controls help, FPS and (F3) debug information.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::interaction::Target;
use crate::player::{GameMode, Player};
use crate::settings::GraphicsSettings;
use crate::sky::TimeOfDay;
use crate::world::{Blocks, ChunkStreaming, ClientWorld};

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
                    "Click: capture mouse   Esc: release   F3: hide this text\n\
                     WASD: move   Space: jump (double-tap: fly)   Shift: down   Ctrl: run\n\
                     Left click: break   Right click: place   Middle click: pick block\n\
                     1-9 / wheel: hotbar   E: inventory   F: fly   G: creative/survival\n\
                     T: skip an hour   [ and ]: render distance",
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
    player: Single<&Player>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    streaming: Res<ChunkStreaming>,
    settings: Res<GraphicsSettings>,
    tod: Res<TimeOfDay>,
    mode: Res<GameMode>,
    target: Res<Target>,
    mut text: Single<&mut Text, With<DebugText>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let p = player.state.position;
    let (hh, mm) = tod.clock();
    let looking_at = target
        .0
        .map(|t| {
            format!(
                "{} at {} {} {}",
                blocks.0.get(t.id).display_name,
                t.block.x,
                t.block.y,
                t.block.z
            )
        })
        .unwrap_or_else(|| "nothing".into());
    let flying = if player.state.flying { ", flying" } else { "" };
    text.0 = format!(
        "FPS: {fps:.0}   Time: {hh:02}:{mm:02}   Mode: {:?}{flying}\n\
         Position: {:.1} / {:.1} / {:.1}\n\
         Looking at: {looking_at}\n\
         Chunks: {} loaded, {} meshes, {} generating, {} meshing\n\
         Render distance: {}",
        *mode,
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

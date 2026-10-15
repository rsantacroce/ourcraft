//! On-screen text: controls help and an FPS counter.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_hud)
            .add_systems(Update, update_fps);
    }
}

#[derive(Component)]
struct FpsText;

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
                Text::new("FPS: --"),
                TextFont::from_font_size(18.0),
                TextColor(Color::WHITE),
                TextShadow::default(),
                FpsText,
            ));
            parent.spawn((
                Text::new(
                    "Click: capture mouse   Esc: release\n\
                     WASD: move   Space/Shift: up/down   Ctrl: fast\n\
                     Gamepad: sticks to move/look, A/B up/down",
                ),
                TextFont::from_font_size(14.0),
                TextColor(Color::WHITE),
                TextShadow::default(),
            ));
        });
}

fn update_fps(diagnostics: Res<DiagnosticsStore>, mut text: Single<&mut Text, With<FpsText>>) {
    if let Some(fps) = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
    {
        text.0 = format!("FPS: {fps:.0}");
    }
}

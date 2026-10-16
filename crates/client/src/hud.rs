//! On-screen text: controls help, FPS and (F3) debug information.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::interaction::Target;
use crate::net::{Connection, Status};
use crate::player::{GameMode, Player};
use crate::remote_players::RemotePlayers;
use crate::settings::GraphicsSettings;
use crate::sky::TimeOfDay;
use crate::world::{Blocks, ChunkStreaming, ClientWorld};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_hud)
            .add_systems(
                Update,
                (toggle_debug, update_debug_text, update_status, show_effects),
            );
    }
}

#[derive(Component)]
struct DebugText;

#[derive(Component)]
struct HelpText;

/// The potions working right now, top right.
#[derive(Component)]
struct EffectsText;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            right: px(16),
            ..default()
        },
        Text::new(""),
        TextFont::from_font_size(18.0),
        TextColor(Color::srgb(0.95, 0.85, 1.0)),
        TextShadow {
            offset: Vec2::splat(1.5),
            color: Color::linear_rgba(0.0, 0.0, 0.0, 0.8),
        },
        EffectsText,
    ));
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
                     1-9 / wheel: hotbar   E: inventory and crafting   Q: drop   F: fly   G: creative/survival\n\
                     Right click doors, chests, signs, beds, furnaces   Hold right click to eat   T: skip an hour   [ and ]: render distance   Enter: chat   F7: simulate lag",
                ),
                TextFont::from_font_size(14.0),
                TextColor(Color::WHITE),
                TextShadow {
                    offset: Vec2::splat(1.5),
                    color: Color::linear_rgba(0.0, 0.0, 0.0, 0.8),
                },
                HelpText,
                // No keyboard on phones and tablets.
                if cfg!(any(target_os = "ios", target_os = "android")) {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                },
            ));
        });
}

fn show_effects(
    effects: Res<crate::player::Effects>,
    mut text: Single<&mut Text, With<EffectsText>>,
) {
    let names = ["Hop", "Glow", "Swim"];
    let lines: Vec<String> = names
        .iter()
        .zip(effects.0)
        .filter(|(_, s)| *s > 0.0)
        .map(|(n, s)| {
            let s = s.ceil() as u32;
            format!("{n} potion {}:{:02}", s / 60, s % 60)
        })
        .collect();
    let joined = lines.join("\n");
    if text.0 != joined {
        text.0 = joined;
    }
}

#[derive(Component)]
struct StatusText;

/// Big centred text while connecting or after a disconnect.
fn update_status(
    mut commands: Commands,
    conn: Option<Res<Connection>>,
    existing: Query<Entity, With<StatusText>>,
    mut shown: Local<Option<String>>,
) {
    let text = match conn.as_deref().map(|c| (&c.status, c)) {
        Some((Status::Connecting, c)) => Some(format!("Connecting to {}...", c.target)),

        _ => None,
    };
    if *shown == text {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    if let Some(t) = &text {
        commands.spawn((
            StatusText,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.07, 0.12, 0.85)),
            children![(
                Text::new(t.clone()),
                TextFont::from_font_size(28.0),
                TextColor(Color::WHITE),
            )],
        ));
    }
    *shown = text;
}

/// F3 (or the setting) shows or hides the info text.
fn toggle_debug(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<crate::input::UiFocus>,
    mut settings: ResMut<crate::settings::ClientSettings>,
    mut debug: Query<&mut Visibility, (With<DebugText>, Without<HelpText>)>,
    mut help: Query<&mut Visibility, (With<HelpText>, Without<DebugText>)>,
) {
    if keys.just_pressed(KeyCode::F3) && !focus.chat_open {
        settings.show_debug = !settings.show_debug;
    }
    if !settings.is_changed() {
        return;
    }
    let vis = if settings.show_debug {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut debug {
        *v = vis;
    }
    // The keyboard help only makes sense with a keyboard.
    let mobile = cfg!(any(target_os = "ios", target_os = "android"));
    for mut v in &mut help {
        *v = if mobile { Visibility::Hidden } else { vis };
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
    conn: Option<Res<Connection>>,
    remote: Res<RemotePlayers>,
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
         Chunks: {} loaded, {} meshes, {} decoding, {} meshing\n\
         Render distance: {}\n\
         {}{}\n\
         Prediction: {} unconfirmed inputs, {} corrections, {} snaps (last {:.2} blocks)",
        *mode,
        p.x,
        p.y,
        p.z,
        world.chunk_count(),
        streaming.mesh_count(),
        streaming.pending_decodes(),
        streaming.pending_meshes(),
        settings.render_distance,
        match conn.as_deref() {
            Some(c) if c.is_single_player() => "Single player".to_owned(),
            Some(c) => format!(
                "Server: {}   Ping: {:.0} ms   Others online: {}",
                c.target,
                c.rtt_ms(),
                remote.count()
            ),
            None => "Not connected".to_owned(),
        },
        match conn.as_deref() {
            Some(c) if c.lag.is_on() => format!(
                "\nLag simulation: +{} ms, +/-{} ms, {}% loss",
                c.lag.latency_ms, c.lag.jitter_ms, c.lag.loss_percent
            ),
            _ => String::new(),
        },
        player.history.len(),
        player.stats.corrections,
        player.stats.snaps,
        player.stats.last_error,
    );
}

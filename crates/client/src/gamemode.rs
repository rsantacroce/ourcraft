//! What game-mode mods can show and change on the client: your rules
//! (speed, flying, building), a health bar, a HUD panel, big titles, and a
//! hotbar of tools instead of blocks.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::protocol::ServerMessage;
use ourcraft_mod_api::{HudLine, PlayerRules, ToolSpec};

use crate::net::{FromServer, NetReceiveSet};

pub struct GameModePlugin;

impl Plugin for GameModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Rules>()
            .init_resource::<Health>()
            .init_resource::<HudPanel>()
            .init_resource::<TitleState>()
            .init_resource::<ToolHotbar>()
            .init_resource::<ToolSpecs>()
            .add_systems(Startup, spawn_ui)
            .add_systems(PreUpdate, handle_messages.after(NetReceiveSet))
            .add_systems(Update, (update_health_bar, update_hud_panel, update_title))
            .add_systems(
                PreUpdate,
                reset.run_if(on_message::<crate::net::SessionEnded>),
            );
    }
}

/// Our rules from the game mode (defaults: anything goes).
#[derive(Resource, Default, Debug, Clone)]
pub struct Rules(pub PlayerRules);

#[derive(Resource, Default, Debug)]
pub struct Health {
    pub health: f32,
    /// 0 = no health bar.
    pub max: f32,
}

#[derive(Resource, Default, Debug)]
pub struct HudPanel(pub Vec<HudLine>);

#[derive(Resource, Default, Debug)]
pub struct TitleState {
    pub text: String,
    /// Seconds left on screen.
    pub remaining: f32,
}

/// Tools given by the game mode. While non-empty, the hotbar shows these
/// instead of blocks, and clicking uses them.
#[derive(Resource, Default, Debug)]
pub struct ToolHotbar(pub Vec<String>);

impl ToolHotbar {
    pub fn active(&self) -> bool {
        !self.0.is_empty()
    }
}

/// Tool definitions from the server's content pack, with their icons.
#[derive(Resource, Default)]
pub struct ToolSpecs {
    pub specs: HashMap<String, ToolSpec>,
    pub icons: HashMap<String, Handle<Image>>,
}

fn handle_messages(
    mut messages: MessageReader<FromServer>,
    mut rules: ResMut<Rules>,
    mut health: ResMut<Health>,
    mut hud: ResMut<HudPanel>,
    mut title: ResMut<TitleState>,
    mut tools: ResMut<ToolHotbar>,
    mut specs: ResMut<ToolSpecs>,
    mut images: ResMut<Assets<Image>>,
) {
    for FromServer(msg) in messages.read() {
        match msg {
            ServerMessage::Welcome { content, .. } => {
                // A fresh join: forget the previous server's game mode.
                *rules = Rules::default();
                *health = Health::default();
                hud.0.clear();
                tools.0.clear();
                specs.specs.clear();
                specs.icons.clear();
                for t in &content.tools {
                    let mut image = Image::new(
                        Extent3d {
                            width: 16,
                            height: 16,
                            depth_or_array_layers: 1,
                        },
                        TextureDimension::D2,
                        t.icon.clone(),
                        TextureFormat::Rgba8UnormSrgb,
                        RenderAssetUsages::RENDER_WORLD,
                    );
                    image.sampler = ImageSampler::nearest();
                    if t.icon.len() == 16 * 16 * 4 {
                        specs.icons.insert(t.name.clone(), images.add(image));
                    }
                    specs.specs.insert(t.name.clone(), t.clone());
                }
            }
            ServerMessage::Rules(r) => rules.0 = r.clone(),
            ServerMessage::Health { health: h, max } => {
                health.health = *h;
                health.max = *max;
            }
            ServerMessage::Hud(lines) => hud.0 = lines.clone(),
            ServerMessage::Title { text, seconds } => {
                title.text = text.clone();
                title.remaining = *seconds;
            }
            ServerMessage::Tools(list) => tools.0 = list.clone(),
            _ => {}
        }
    }
}

fn reset(
    mut rules: ResMut<Rules>,
    mut health: ResMut<Health>,
    mut hud: ResMut<HudPanel>,
    mut title: ResMut<TitleState>,
    mut tools: ResMut<ToolHotbar>,
) {
    *rules = Rules::default();
    *health = Health::default();
    hud.0.clear();
    *title = TitleState::default();
    tools.0.clear();
}

#[derive(Component)]
struct HealthBarFill;

#[derive(Component)]
struct HealthBar;

#[derive(Component)]
struct HudPanelText;

#[derive(Component)]
struct TitleText;

fn spawn_ui(mut commands: Commands) {
    // Health bar: just above the hotbar.
    commands
        .spawn((
            HealthBar,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(92),
                left: percent(50),
                width: px(240),
                height: px(14),
                margin: UiRect::left(px(-120)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(7)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            Visibility::Hidden,
        ))
        .with_child((
            HealthBarFill,
            Node {
                width: percent(100),
                height: percent(100),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.3, 0.85, 0.4)),
        ));

    // HUD panel: top right.
    commands.spawn((
        HudPanelText,
        Text::new(""),
        TextFont::from_font_size(17.0),
        TextLayout::justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            // Leave room for the touch "Bag" button in the corner.
            right: px(100),
            padding: UiRect::all(px(8)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
    ));

    // Title: centre of the screen.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            top: percent(28),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_child((
            TitleText,
            Text::new(""),
            TextFont::from_font_size(44.0),
            TextColor(Color::WHITE),
            TextShadow {
                offset: Vec2::splat(3.0),
                color: Color::srgba(0.0, 0.0, 0.0, 0.8),
            },
        ));
}

fn update_health_bar(
    health: Res<Health>,
    mut bar: Single<&mut Visibility, With<HealthBar>>,
    mut fill: Single<(&mut Node, &mut BackgroundColor), With<HealthBarFill>>,
) {
    if !health.is_changed() {
        return;
    }
    **bar = if health.max > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    let frac = if health.max > 0.0 {
        (health.health / health.max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (node, color) = &mut *fill;
    node.width = percent(frac * 100.0);
    // Green when healthy, through yellow, to red when nearly out.
    color.0 = Color::hsl(120.0 * frac, 0.75, 0.5);
}

fn update_hud_panel(
    hud: Res<HudPanel>,
    panel: Single<(Entity, &mut BackgroundColor), With<HudPanelText>>,
    mut writer: TextUiWriter,
    mut commands: Commands,
) {
    if !hud.is_changed() {
        return;
    }
    let (entity, mut bg) = panel.into_inner();
    bg.0 = if hud.0.is_empty() {
        Color::NONE
    } else {
        Color::srgba(0.0, 0.0, 0.0, 0.45)
    };
    // One text span per line, each in its own colour.
    commands.entity(entity).despawn_children();
    *writer.text(entity, 0) = String::new();
    commands.entity(entity).with_children(|t| {
        for (i, line) in hud.0.iter().enumerate() {
            let [r, g, b, a] = line.color;
            let text = if i + 1 < hud.0.len() {
                format!("{}\n", line.text)
            } else {
                line.text.clone()
            };
            t.spawn((
                TextSpan::new(text),
                TextFont::from_font_size(17.0),
                TextColor(Color::srgba_u8(r, g, b, a)),
            ));
        }
    });
}

fn update_title(
    time: Res<Time>,
    mut title: ResMut<TitleState>,
    mut text: Single<(&mut Text, &mut TextColor), With<TitleText>>,
) {
    if title.remaining > 0.0 {
        title.remaining -= time.delta_secs();
    }
    let (t, color) = &mut *text;
    if title.remaining <= 0.0 {
        if !t.0.is_empty() {
            t.0.clear();
        }
        return;
    }
    if t.0 != title.text {
        t.0 = title.text.clone();
    }
    // Fade out during the last half second.
    color.0 = Color::srgba(1.0, 1.0, 1.0, (title.remaining * 2.0).min(1.0));
}

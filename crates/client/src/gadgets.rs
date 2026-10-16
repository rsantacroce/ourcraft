//! Gadgets you hold (milestone 20): the compass points home, the clock
//! tells the time of day, the map shows the land around you, and the
//! spyglass zooms in while you hold "place".

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::block::BlockId;
use ourcraft_common::protocol::ServerMessage;

use crate::input::{Actions, UiFocus};
use crate::interaction::PlayerInventory;
use crate::items::Items;
use crate::net::{FromServer, NetReceiveSet};
use crate::player::{Player, PlayerCamera};
use crate::world::{Blocks, ChunkStreaming, ClientWorld};

/// Blocks across the map (one pixel each).
const MAP_SIZE: u32 = 96;
const NORMAL_FOV: f32 = 70.0;
const ZOOM_FOV: f32 = 14.0;

pub struct GadgetsPlugin;

impl Plugin for GadgetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Home>()
            .add_systems(Startup, spawn_panel)
            .add_systems(PreUpdate, hear_home.after(NetReceiveSet))
            .add_systems(Update, (show_gadgets, spyglass));
    }
}

/// Where the compass points (from the server).
#[derive(Resource, Default)]
struct Home(Option<IVec3>);

#[derive(Component)]
struct GadgetPanel;
#[derive(Component)]
struct Needle;
#[derive(Component)]
struct ClockText;
#[derive(Component)]
struct MapImage;

fn hear_home(mut messages: MessageReader<FromServer>, mut home: ResMut<Home>) {
    for FromServer(msg) in messages.read() {
        if let ServerMessage::Home { pos } = msg {
            home.0 = Some(IVec3::from_array(*pos));
        }
    }
}

fn image(pixels: Vec<u8>, size: u32) -> Image {
    let mut img = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    img.sampler = ImageSampler::nearest();
    img
}

/// A compass needle pointing up: red tip, white tail.
fn needle_pixels() -> Vec<u8> {
    let n = 32usize;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 - 15.5, y as f32 - 15.5);
            // A diamond shape, long up and down.
            let inside = fx.abs() * 4.0 + fy.abs() * 0.6 <= 10.0;
            let ring = ((fx * fx + fy * fy).sqrt() - 15.0).abs() < 1.0;
            let px: [u8; 4] = if inside {
                if fy < 0.0 {
                    [220, 40, 40, 255]
                } else {
                    [240, 240, 240, 255]
                }
            } else if ring {
                [60, 60, 70, 255]
            } else {
                [0, 0, 0, 0]
            };
            out[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&px);
        }
    }
    out
}

fn spawn_panel(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let needle = images.add(image(needle_pixels(), 32));
    let map = images.add(image(vec![0; (MAP_SIZE * MAP_SIZE * 4) as usize], MAP_SIZE));
    commands
        .spawn((
            GadgetPanel,
            Node {
                position_type: PositionType::Absolute,
                top: px(12),
                left: percent(50),
                margin: UiRect::left(px(-60)),
                width: px(120),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(4),
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                Needle,
                ImageNode::new(needle),
                Node {
                    width: px(64),
                    height: px(64),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                Visibility::Hidden,
            ));
            p.spawn((
                ClockText,
                Text::new(""),
                TextFont::from_font_size(16.0),
                TextShadow::default(),
                Visibility::Hidden,
            ));
            p.spawn((
                MapImage,
                ImageNode::new(map),
                Node {
                    width: px(MAP_SIZE as f32 * 1.5),
                    height: px(MAP_SIZE as f32 * 1.5),
                    border: UiRect::all(px(3)),
                    ..default()
                },
                BorderColor::all(Color::srgb(0.45, 0.32, 0.18)),
                Visibility::Hidden,
            ));
        });
}

/// Which way the compass needle turns (radians, clockwise on screen) for
/// someone at `from` facing `yaw`, with home at `home`.
fn needle_angle(from: Vec3, yaw: f32, home: Vec3) -> f32 {
    let d = home - from;
    // The yaw that would face straight at home (forward is (-sin, -cos)).
    let to = (-d.x).atan2(-d.z);
    let turn = to - yaw;
    // Turning left (a bigger yaw) is anticlockwise.
    -turn
}

/// The time of day in words.
fn time_words(fraction: f32) -> &'static str {
    match fraction {
        f if !(0.22..0.78).contains(&f) => "Night",
        f if f < 0.4 => "Morning",
        f if f < 0.6 => "Midday",
        _ => "Evening",
    }
}

#[allow(clippy::too_many_arguments)]
fn show_gadgets(
    time: Res<Time>,
    inventory: Res<PlayerInventory>,
    items: Res<Items>,
    focus: Res<UiFocus>,
    home: Res<Home>,
    tod: Res<crate::sky::TimeOfDay>,
    player: Single<&Player>,
    (world, streaming, blocks, textures): (
        Res<ClientWorld>,
        Res<ChunkStreaming>,
        Res<Blocks>,
        Option<Res<crate::render::ModTextures>>,
    ),
    mut panel: Single<&mut Visibility, With<GadgetPanel>>,
    mut parts: Query<
        (
            &mut Visibility,
            Option<&mut UiTransform>,
            Option<&mut Text>,
            Option<&ImageNode>,
            Has<Needle>,
            Has<ClockText>,
            Has<MapImage>,
        ),
        Without<GadgetPanel>,
    >,
    mut images: ResMut<Assets<Image>>,
    mut colours: Local<HashMap<BlockId, [u8; 4]>>,
    mut redraw: Local<f32>,
) {
    let held = inventory.held().map(|h| items.name(h)).unwrap_or("");
    let shown =
        !focus.gameplay_blocked() && matches!(held, "core:compass" | "core:clock" | "core:map");
    **panel = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if !shown {
        *redraw = 0.0;
        return;
    }
    *redraw -= time.delta_secs();
    let draw_map = held == "core:map" && *redraw <= 0.0;
    if draw_map {
        *redraw = 0.5;
    }
    let me = player.state.position;
    for (mut vis, transform, text, image_node, needle, clock, map) in &mut parts {
        let on = (needle && held == "core:compass")
            || (clock && held == "core:clock")
            || (map && held == "core:map");
        *vis = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !on {
            continue;
        }
        if needle && let (Some(mut t), Some(h)) = (transform, home.0) {
            let target = h.as_vec3() + Vec3::splat(0.5);
            t.rotation = Rot2::radians(needle_angle(me, player.yaw, target));
        }
        if clock && let Some(mut t) = text {
            t.0 = time_words(tod.fraction).to_owned();
        }
        if map
            && draw_map
            && let Some(img) = image_node
        {
            let Some(mut image) = images.get_mut(&img.image) else {
                continue;
            };
            let Some(data) = image.data.as_mut() else {
                continue;
            };
            let half = MAP_SIZE as i32 / 2;
            let (cx, cz) = (me.x.floor() as i32, me.z.floor() as i32);
            for py in 0..MAP_SIZE as i32 {
                for px_ in 0..MAP_SIZE as i32 {
                    let (x, z) = (cx + px_ - half, cz + py - half);
                    let rgba = match streaming.top(x, z) {
                        Some(top) if top >= 0 => {
                            let b = world.0.block(x, top as i32, z);
                            let c = *colours.entry(b).or_insert_with(|| {
                                average_colour(&blocks.0, textures.as_deref(), b)
                            });
                            // Higher ground a little lighter.
                            let lift = ((top as i32 - 64) * 2).clamp(-40, 40);
                            let f = |v: u8| (v as i32 + lift).clamp(0, 255) as u8;
                            [f(c[0]), f(c[1]), f(c[2]), 255]
                        }
                        // Not explored (not loaded): parchment.
                        _ => [214, 196, 150, 255],
                    };
                    let i = ((py as u32 * MAP_SIZE + px_ as u32) * 4) as usize;
                    data[i..i + 4].copy_from_slice(&rgba);
                }
            }
            // You: a red dot in the middle.
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = (((half + dy) as u32 * MAP_SIZE + (half + dx) as u32) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[220, 30, 30, 255]);
            }
        }
    }
}

/// A block's colour on the map: the average of its top texture.
fn average_colour(
    blocks: &ourcraft_common::block::BlockRegistry,
    textures: Option<&crate::render::ModTextures>,
    b: BlockId,
) -> [u8; 4] {
    let Some(t) = blocks.get(b).textures.as_ref() else {
        return [214, 196, 150, 255];
    };
    let pixels = match textures {
        Some(m) => m.pixels(&t.top),
        None => crate::render::textures::generate_texture(&t.top),
    };
    let (mut sum, mut n) = ([0u32; 3], 0u32);
    for p in pixels.chunks(4).filter(|p| p[3] > 0) {
        for i in 0..3 {
            sum[i] += p[i] as u32;
        }
        n += 1;
    }
    let n = n.max(1);
    [
        (sum[0] / n) as u8,
        (sum[1] / n) as u8,
        (sum[2] / n) as u8,
        255,
    ]
}

/// Holding "place" with a spyglass zooms in.
fn spyglass(
    time: Res<Time>,
    actions: Res<Actions>,
    inventory: Res<PlayerInventory>,
    items: Res<Items>,
    focus: Res<UiFocus>,
    mut camera: Query<&mut Projection, With<PlayerCamera>>,
) {
    let looking = actions.place_block
        && !focus.gameplay_blocked()
        && inventory
            .held()
            .is_some_and(|h| items.name(h) == "core:spyglass");
    let want = if looking { ZOOM_FOV } else { NORMAL_FOV }.to_radians();
    for mut projection in &mut camera {
        if let Projection::Perspective(p) = &mut *projection {
            let step = 3.0 * time.delta_secs();
            p.fov += (want - p.fov).clamp(-step, step);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_needle_points_home() {
        // Facing north (-z), home straight ahead: no turn.
        assert!(needle_angle(Vec3::ZERO, 0.0, Vec3::new(0.0, 0.0, -10.0)).abs() < 1e-5);
        // Home to the east (+x, our right): the needle turns clockwise.
        let a = needle_angle(Vec3::ZERO, 0.0, Vec3::new(10.0, 0.0, 0.0));
        assert!((a - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "{a}");
        assert_eq!(time_words(0.5), "Midday");
        assert_eq!(time_words(0.0), "Night");
    }
}

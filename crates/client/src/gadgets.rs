//! Gadgets you hold (milestone 20): the compass points home, the clock
//! tells the time of day, the map shows the land around you, and the
//! spyglass zooms in while you hold "place".
//!
//! The map remembers the land you've seen (every column that was loaded),
//! saved per world, so it fills in as you explore.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::block::BlockId;
use ourcraft_common::protocol::ServerMessage;
use ourcraft_common::world::ColumnPos;

use crate::input::{Actions, UiFocus};
use crate::interaction::PlayerInventory;
use crate::items::Items;
use crate::net::{Connection, FromServer, NetReceiveSet};
use crate::player::{Player, PlayerCamera};
use crate::world::{Blocks, ChunkStreaming, ClientWorld};

/// Blocks across the map (one pixel each).
const MAP_SIZE: u32 = 96;
const NORMAL_FOV: f32 = 70.0;
const ZOOM_FOV: f32 = 14.0;
/// Blocks per pixel at each zoom (right click with the map to change).
const MAP_ZOOMS: [i32; 3] = [1, 2, 4];
/// Parchment: land not seen yet.
const PARCHMENT: [u8; 3] = [214, 196, 150];
/// Open sky (in the Cloud Isles, where there's no land below).
const SKY_BLUE: [u8; 3] = [196, 206, 245];
/// The most columns a map remembers (about 3 MB on disk).
const MAP_MEMORY_MAX: usize = 4096;

pub struct GadgetsPlugin;

impl Plugin for GadgetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Home>()
            .init_resource::<MapMemory>()
            .add_systems(Startup, spawn_panel)
            .add_systems(PreUpdate, hear_home.after(NetReceiveSet))
            .add_systems(
                Update,
                (open_map_memory, remember_land, show_gadgets, spyglass).chain(),
            )
            .add_systems(Last, save_map_memory.run_if(on_message::<AppExit>));
    }
}

/// Where the compass points (from the server).
#[derive(Resource, Default)]
struct Home(Option<IVec3>);

/// The land seen in this world: one colour per block, a column at a time.
#[derive(Resource, Default)]
pub struct MapMemory {
    file: Option<std::path::PathBuf>,
    columns: HashMap<ColumnPos, Box<[[u8; 3]; 256]>>,
    /// The order columns were first seen (the oldest are forgotten first).
    order: std::collections::VecDeque<ColumnPos>,
    unsaved: bool,
    since_save: f32,
}

impl MapMemory {
    /// What the map shows at `x, z`, if that land has been seen.
    pub fn colour(&self, x: i32, z: i32) -> Option<[u8; 3]> {
        let c = self.columns.get(&ColumnPos::from_block(x, z))?;
        Some(c[(x.rem_euclid(16) + z.rem_euclid(16) * 16) as usize])
    }

    pub fn remember(&mut self, pos: ColumnPos, colours: [[u8; 3]; 256]) {
        match self.columns.get_mut(&pos) {
            Some(old) if **old == colours => return,
            Some(old) => **old = colours,
            None => {
                self.columns.insert(pos, Box::new(colours));
                self.order.push_back(pos);
                while self.order.len() > MAP_MEMORY_MAX {
                    if let Some(gone) = self.order.pop_front() {
                        self.columns.remove(&gone);
                    }
                }
            }
        }
        self.unsaved = true;
    }

    /// Columns as bytes: x, z (little-endian i32s), then 768 colour bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.order.len() * 776 + 4);
        out.extend_from_slice(b"MAP1");
        for pos in &self.order {
            let Some(c) = self.columns.get(pos) else {
                continue;
            };
            out.extend_from_slice(&pos.x.to_le_bytes());
            out.extend_from_slice(&pos.z.to_le_bytes());
            out.extend(c.iter().flatten());
        }
        out
    }

    /// Read what `to_bytes` wrote (anything broken reads as nothing).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut m = Self::default();
        let Some(rest) = bytes.strip_prefix(b"MAP1") else {
            return m;
        };
        for entry in rest.chunks_exact(776) {
            let x = i32::from_le_bytes(entry[0..4].try_into().unwrap());
            let z = i32::from_le_bytes(entry[4..8].try_into().unwrap());
            let mut c = [[0u8; 3]; 256];
            for (i, px) in c.iter_mut().enumerate() {
                px.copy_from_slice(&entry[8 + i * 3..11 + i * 3]);
            }
            m.remember(ColumnPos::new(x, z), c);
        }
        m.unsaved = false;
        m
    }
}

/// Load the world's map memory when we join it.
fn open_map_memory(conn: Option<Res<Connection>>, mut memory: ResMut<MapMemory>) {
    let file = conn.map(|c| c.map_file.clone());
    if memory.file == file {
        return;
    }
    // Leaving a world: keep what was seen there first.
    write_map_memory(&mut memory);
    let mut fresh = file
        .as_ref()
        .and_then(|f| std::fs::read(f).ok())
        .map(|b| MapMemory::from_bytes(&b))
        .unwrap_or_default();
    fresh.file = file;
    *memory = fresh;
}

fn save_map_memory(mut memory: ResMut<MapMemory>) {
    write_map_memory(&mut memory);
}

fn write_map_memory(memory: &mut MapMemory) {
    if !memory.unsaved {
        return;
    }
    let Some(file) = memory.file.clone() else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = file.with_extension("tmp");
    match std::fs::write(&tmp, memory.to_bytes()).and_then(|_| std::fs::rename(&tmp, &file)) {
        Ok(()) => memory.unsaved = false,
        Err(e) => warn!("Couldn't save the map: {e}"),
    }
}

/// What the map shows for the top block `b` at height `top`.
fn land_colour(c: [u8; 4], top: i16) -> [u8; 3] {
    // Higher ground a little lighter.
    let lift = ((top as i32 - 64) * 2).clamp(-40, 40);
    let f = |v: u8| (v as i32 + lift).clamp(0, 255) as u8;
    [f(c[0]), f(c[1]), f(c[2])]
}

/// Every so often, remember the loaded land (new columns first; the ones
/// nearby again, since things get built and dug).
#[allow(clippy::too_many_arguments)]
fn remember_land(
    time: Res<Time>,
    world: Res<ClientWorld>,
    streaming: Res<ChunkStreaming>,
    blocks: Res<Blocks>,
    textures: Option<Res<crate::render::ModTextures>>,
    dimension: Res<crate::net::CurrentDimension>,
    mut memory: ResMut<MapMemory>,
    mut colours: Local<HashMap<BlockId, [u8; 4]>>,
    mut wait: Local<f32>,
) {
    // The map only remembers the overworld (the Cloud Isles are mostly
    // empty sky, and would draw over it).
    let home = dimension.0 == ourcraft_common::dimension::Dimension::Overworld;
    if memory.file.is_none() || !home {
        return;
    }
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    *wait = 2.0;
    let near = streaming.center;
    let columns: Vec<ColumnPos> = streaming
        .loaded_columns()
        .filter(|c| {
            !memory.columns.contains_key(c)
                || ((c.x - near.x).abs() <= 2 && (c.z - near.z).abs() <= 2)
        })
        .collect();
    for col in columns {
        let mut out = [PARCHMENT; 256];
        let mut complete = true;
        for (i, px) in out.iter_mut().enumerate() {
            let (x, z) = (col.x * 16 + (i % 16) as i32, col.z * 16 + (i / 16) as i32);
            match streaming.top(x, z) {
                Some(top) if top >= 0 => {
                    let b = world.0.block(x, top as i32, z);
                    let c = *colours
                        .entry(b)
                        .or_insert_with(|| average_colour(&blocks.0, textures.as_deref(), b));
                    *px = land_colour(c, top);
                }
                Some(_) => {}
                None => complete = false,
            }
        }
        if complete {
            memory.remember(col, out);
        }
    }
    memory.since_save += 2.0;
    if memory.since_save >= 30.0 {
        memory.since_save = 0.0;
        write_map_memory(&mut memory);
    }
}

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
    (world, streaming, blocks, textures, dimension): (
        Res<ClientWorld>,
        Res<ChunkStreaming>,
        Res<Blocks>,
        Option<Res<crate::render::ModTextures>>,
        Res<crate::net::CurrentDimension>,
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
        (
            Without<GadgetPanel>,
            Or<(With<Needle>, With<ClockText>, With<MapImage>)>,
        ),
    >,
    mut images: ResMut<Assets<Image>>,
    (memory, actions, mut colours, mut redraw, mut zoom): (
        Res<MapMemory>,
        Res<Actions>,
        Local<HashMap<BlockId, [u8; 4]>>,
        Local<f32>,
        Local<(usize, bool)>,
    ),
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
    // "Place" with a map zooms out (and back in after the last step).
    let (level, was_down) = &mut *zoom;
    if held == "core:map" && actions.place_block && !*was_down {
        *level = (*level + 1) % MAP_ZOOMS.len();
        *redraw = 0.0;
    }
    *was_down = actions.place_block;
    let step = MAP_ZOOMS[*level];
    let draw_map = held == "core:map" && *redraw <= 0.0;
    if draw_map {
        *redraw = 0.5;
    }
    let me = player.state.position;
    let in_overworld = dimension.0 == ourcraft_common::dimension::Dimension::Overworld;
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
                    let (x, z) = (cx + (px_ - half) * step, cz + (py - half) * step);
                    let rgb = match streaming.top(x, z) {
                        Some(top) if top >= 0 => {
                            let b = world.0.block(x, top as i32, z);
                            let c = *colours.entry(b).or_insert_with(|| {
                                average_colour(&blocks.0, textures.as_deref(), b)
                            });
                            land_colour(c, top)
                        }
                        // Empty sky in the Cloud Isles.
                        Some(_) if !in_overworld => SKY_BLUE,
                        // Not loaded now: what we saw before, or parchment.
                        _ if in_overworld => memory.colour(x, z).unwrap_or(PARCHMENT),
                        _ => PARCHMENT,
                    };
                    let rgba = [rgb[0], rgb[1], rgb[2], 255];
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
    fn the_map_remembers() {
        let mut m = MapMemory::default();
        let mut c = [PARCHMENT; 256];
        c[17] = [1, 2, 3];
        m.remember(ColumnPos::new(-2, 5), c);
        let back = MapMemory::from_bytes(&m.to_bytes());
        assert_eq!(back.colour(-32 + 1, 80 + 1), Some([1, 2, 3]));
        assert_eq!(back.colour(-32, 80), Some(PARCHMENT));
        assert_eq!(back.colour(0, 0), None);
        // Only so much is kept: the oldest is forgotten.
        for i in 0..MAP_MEMORY_MAX as i32 {
            m.remember(ColumnPos::new(i, 100), c);
        }
        assert_eq!(m.colour(-32, 80), None);
        assert_eq!(MapMemory::from_bytes(b"junk").columns.len(), 0);
    }

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

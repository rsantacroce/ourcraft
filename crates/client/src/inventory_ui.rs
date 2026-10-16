//! The crosshair, hotbar and inventory screens.
//!
//! Screens (all opened with the inventory focus):
//! - **Inventory** (E): your items plus a 2×2 crafting grid; in creative,
//!   a palette of every item instead.
//! - **Crafting table** (use a crafting table): a 3×3 grid.
//! - **Furnace** (use a furnace): its three slots, kept by the server.
//!
//! Slots work like in every block game: click to pick a stack up onto the
//! mouse pointer, click again to put it down (or swap, or merge). Right
//! click picks up half, or puts down one.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::container::{ContainerKind, FURNACE_FUEL, FURNACE_INPUT, FURNACE_OUTPUT};
use ourcraft_common::inventory::{HOTBAR_SLOTS, INVENTORY_SLOTS, ItemStack, click_slot};
use ourcraft_common::item::{ItemId, ItemKind, ItemRegistry};
use ourcraft_common::protocol::{Channel, ClientMessage, NetContainer, NetStack};

use crate::input::{Actions, UiFocus};
use crate::interaction::PlayerInventory;
use crate::items::{Items, Recipes};
use crate::player::GameMode;
use crate::render::textures::TEXTURE_SIZE;
use crate::world::Blocks;

pub struct InventoryUiPlugin;

impl Plugin for InventoryUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenScreen>()
            .init_resource::<Cursor>()
            .init_resource::<PaletteTab>()
            .init_resource::<CraftGrid>()
            .init_resource::<ContainerView>()
            .add_systems(
                Startup,
                (
                    build_icons,
                    spawn_crosshair,
                    spawn_hotbar,
                    spawn_cursor_icon,
                )
                    .chain(),
            )
            // Mods can add blocks and items when we join a server: redraw.
            .add_systems(
                Update,
                build_icons.run_if(resource_changed::<Items>.or_eager(resource_changed::<Blocks>)),
            )
            .add_systems(
                Update,
                (
                    toggle_inventory_screen,
                    close_screen_cleanup,
                    slot_clicks,
                    rebuild_hotbar,
                    rebuild_screen,
                    move_cursor_icon,
                )
                    .chain(),
            );
    }
}

/// A picture of each item for the hotbar and inventory: a small isometric
/// cube for blocks, a flat icon for everything else.
#[derive(Resource, Default)]
pub struct ItemIcons(pub HashMap<ItemId, Handle<Image>>);

/// Which screen the inventory focus shows.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenScreen {
    #[default]
    Inventory,
    CraftingTable,
    /// A furnace or chest (kept by the server).
    Container(IVec3),
    /// Writing on a sign (see the `signs` module).
    Sign(IVec3),
}

impl OpenScreen {
    /// Size of the crafting grid on this screen (0 = none).
    fn grid_size(self) -> usize {
        match self {
            OpenScreen::Inventory => 2,
            OpenScreen::CraftingTable => 3,
            OpenScreen::Container(_) | OpenScreen::Sign(_) => 0,
        }
    }
}

/// The stack carried on the mouse pointer.
#[derive(Resource, Default, Debug)]
pub struct Cursor {
    pub stack: Option<ItemStack>,
    /// A furnace click is on its way to the server; wait for the answer.
    pub pending: bool,
}

/// The crafting grid's cells, row by row (2×2 uses the first 4).
#[derive(Resource, Default, Debug)]
pub struct CraftGrid(pub [Option<ItemStack>; 9]);

/// The open furnace or chest, as the server last described it.
#[derive(Resource, Default, Debug)]
pub struct ContainerView(pub Option<NetContainer>);

const ICON: u32 = 32;

/// Draw an isometric cube from a block's top and side textures.
pub fn render_block_icon(top: &[u8], side: &[u8]) -> Vec<u8> {
    let t = TEXTURE_SIZE as f32;
    let mut out = vec![0u8; (ICON * ICON * 4) as usize];
    let sample = |tex: &[u8], a: f32, b: f32, shade: f32| -> [u8; 4] {
        let x = ((a * t) as usize).min(TEXTURE_SIZE as usize - 1);
        let y = ((b * t) as usize).min(TEXTURE_SIZE as usize - 1);
        let i = (y * TEXTURE_SIZE as usize + x) * 4;
        let f = |v: u8| (v as f32 * shade) as u8;
        [f(tex[i]), f(tex[i + 1]), f(tex[i + 2]), tex[i + 3]]
    };
    let unit = |v: f32| (0.0..1.0).contains(&v);
    for py in 0..ICON {
        for px in 0..ICON {
            let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
            // Three faces: a diamond on top, two parallelograms below it.
            let ta = ((x - 16.0) / 16.0 + y / 8.0) / 2.0;
            let tb = (y / 8.0 - (x - 16.0) / 16.0) / 2.0;
            let la = x / 16.0;
            let lb = (y - 8.0 - 8.0 * la) / 16.0;
            let ra = (x - 16.0) / 16.0;
            let rb = (y - 16.0 + 8.0 * ra) / 16.0;
            let px_color = if unit(ta) && unit(tb) {
                Some(sample(top, ta, tb, 1.0))
            } else if unit(la) && unit(lb) {
                Some(sample(side, la, lb, 0.8))
            } else if unit(ra) && unit(rb) {
                Some(sample(side, ra, rb, 0.62))
            } else {
                None
            };
            if let Some(c) = px_color {
                let i = ((py * ICON + px) * 4) as usize;
                out[i..i + 4].copy_from_slice(&c);
            }
        }
    }
    out
}

/// Draw the boxes of a shaped block (stairs, slabs, fences…) the same way
/// as [`render_block_icon`] draws a cube: seen from above one corner, top
/// faces brightest. Each pixel sends a ray into the scene and takes the
/// colour of the first box face it hits.
pub fn render_boxes_icon(top: &[u8], side: &[u8], boxes: &[([f32; 3], [f32; 3])]) -> Vec<u8> {
    let t = TEXTURE_SIZE as f32;
    let mut out = vec![0u8; (ICON * ICON * 4) as usize];
    let dir = Vec3::new(-1.0, -1.0, -1.0).normalize();
    // The cube icon's corners: its top corner is at the top middle, and it
    // is 32 pixels wide; work backwards from that.
    let c30 = 16.0;
    let s30 = 8.0;
    let sample = |tex: &[u8], a: f32, b: f32, shade: f32| -> [u8; 4] {
        let x = ((a.clamp(0.0, 0.999) * t) as usize).min(TEXTURE_SIZE as usize - 1);
        let y = ((b.clamp(0.0, 0.999) * t) as usize).min(TEXTURE_SIZE as usize - 1);
        let i = (y * TEXTURE_SIZE as usize + x) * 4;
        let f = |v: u8| (v as f32 * shade) as u8;
        [f(tex[i]), f(tex[i + 1]), f(tex[i + 2]), tex[i + 3]]
    };
    for py in 0..ICON {
        for px in 0..ICON {
            // Screen position → a point high above the scene on the ray.
            let sx = (px as f32 + 0.5 - 16.0) / c30;
            let sy = (py as f32 + 0.5) / s30;
            // sx = x - z, sy = x + z - 2y + 2 (from how the cube icon is laid out).
            let y0 = 3.0;
            let sum = sy + 2.0 * y0 - 2.0;
            let x0 = (sum + sx) / 2.0;
            let z0 = (sum - sx) / 2.0;
            let origin = Vec3::new(x0, y0, z0);
            let mut best: Option<(f32, usize, Vec3)> = None;
            for (min, max) in boxes {
                let (min, max) = (Vec3::from(*min), Vec3::from(*max));
                let (mut tn, mut tf, mut axis) = (f32::MIN, f32::MAX, 0);
                for a in 0..3 {
                    let (mut t1, mut t2) =
                        ((min[a] - origin[a]) / dir[a], (max[a] - origin[a]) / dir[a]);
                    if t1 > t2 {
                        std::mem::swap(&mut t1, &mut t2);
                    }
                    if t1 > tn {
                        tn = t1;
                        axis = a;
                    }
                    tf = tf.min(t2);
                }
                if tn <= tf && best.is_none_or(|b| tn < b.0) {
                    best = Some((tn, axis, origin + dir * tn));
                }
            }
            let Some((_, axis, p)) = best else { continue };
            // Same shading as the cube icon: +z faces lighter than +x faces.
            let color = match axis {
                1 => sample(top, p.x, p.z, 1.0),
                0 => sample(side, 1.0 - p.z, 1.0 - p.y, 0.62),
                _ => sample(side, p.x, 1.0 - p.y, 0.8),
            };
            if color[3] > 0 {
                let i = ((py * ICON + px) * 4) as usize;
                out[i..i + 4].copy_from_slice(&color);
            }
        }
    }
    out
}

/// Scale a 16×16 picture up to the icon size (each pixel becomes 2×2).
fn upscale(pixels: &[u8]) -> Vec<u8> {
    let n = TEXTURE_SIZE as usize;
    let mut out = vec![0u8; (ICON * ICON * 4) as usize];
    for y in 0..ICON as usize {
        for x in 0..ICON as usize {
            let src = ((y / 2) * n + x / 2) * 4;
            let dst = (y * ICON as usize + x) * 4;
            out[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
        }
    }
    out
}

fn icon_image(pixels: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: ICON,
            height: ICON,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// The picture of an item, as 32×32 RGBA.
pub fn item_icon_pixels(
    items: &ItemRegistry,
    blocks: &ourcraft_common::block::BlockRegistry,
    textures: &crate::render::ModTextures,
    id: ItemId,
) -> Option<Vec<u8>> {
    let def = items.get(id)?;
    match def.kind {
        // Blocks that aren't cubes (torches) get a flat picture.
        ItemKind::Block(b) if blocks.get(b).shape.is_cube() => {
            let tex = blocks.get(b).textures.as_ref()?;
            Some(render_block_icon(
                &textures.pixels(&tex.top),
                &textures.pixels(&tex.side),
            ))
        }
        // Plants (flowers, crops, grass): a flat picture of the plant.
        ItemKind::Block(b)
            if crate::render::item_icons::item_icon(def.name.trim_start_matches("core:"))
                .is_none()
                && blocks.stage(b).is_some_and(|s| {
                    !matches!(
                        s.kind,
                        ourcraft_common::farming::StageKind::Cake
                            | ourcraft_common::farming::StageKind::Composter
                    )
                }) =>
        {
            let tex = blocks.get(b).textures.as_ref()?;
            Some(upscale(&textures.pixels(&tex.side)))
        }
        // Other shapes: a 3D picture of the shape, unless there's a flat
        // picture for it (torches, doors, signs).
        ItemKind::Block(b)
            if crate::render::item_icons::item_icon(def.name.trim_start_matches("core:"))
                .is_none()
                && blocks.get(b).textures.is_some()
                && !blocks.is_climbable(b) =>
        {
            let tex = blocks.get(b).textures.as_ref()?;
            // Fences are shown joined up on both sides.
            let fence = |o: [i32; 3]| {
                if o[0] != 0 {
                    b
                } else {
                    ourcraft_common::block::BlockId::AIR
                }
            };
            let boxes = blocks.boxes_at(b, fence);
            Some(render_boxes_icon(
                &textures.pixels(&tex.top),
                &textures.pixels(&tex.side),
                &boxes,
            ))
        }
        _ => {
            let short = def.name.trim_start_matches("core:");
            let side = def
                .block()
                .and_then(|b| blocks.get(b).textures.as_ref())
                .map(|t| t.side.clone());
            let flat = crate::render::item_icons::item_icon(short)
                // A shaped block's side texture, or a mod's texture with the
                // item's name.
                .unwrap_or_else(|| textures.pixels(side.as_deref().unwrap_or(&def.name)));
            Some(upscale(&flat))
        }
    }
}

fn build_icons(
    mut commands: Commands,
    blocks: Res<Blocks>,
    items: Res<Items>,
    textures: Option<Res<crate::render::ModTextures>>,
    mut images: ResMut<Assets<Image>>,
) {
    let textures = textures.map(|t| t.clone()).unwrap_or_default();
    let mut icons = ItemIcons::default();
    for (id, _) in items.iter() {
        if let Some(px) = item_icon_pixels(&items, &blocks.0, &textures, id) {
            icons.0.insert(id, images.add(icon_image(px)));
        }
    }
    commands.insert_resource(icons);
}

fn spawn_crosshair(mut commands: Commands) {
    let bar = |w: f32, h: f32| {
        (
            Node {
                position_type: PositionType::Absolute,
                width: px(w),
                height: px(h),
                left: percent(50),
                top: percent(50),
                margin: UiRect {
                    left: px(-w / 2.0),
                    top: px(-h / 2.0),
                    ..default()
                },
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        )
    };
    commands.spawn((Name::new("Crosshair"), bar(18.0, 2.0)));
    commands.spawn((Name::new("Crosshair"), bar(2.0, 18.0)));
}

#[derive(Component)]
struct Hotbar;

/// A clickable slot.
#[derive(Component, Clone, Copy, Debug)]
enum SlotButton {
    /// One of the player's 36 inventory slots.
    Inventory(usize),
    /// A cell of the crafting grid.
    Craft(usize),
    /// What the crafting grid makes.
    CraftResult,
    /// A slot of the open furnace.
    Container(usize),
    /// An item in the creative palette.
    Palette(ItemId),
    /// A tab of the creative palette.
    Tab(PaletteKind),
    /// The next (+1) or previous (-1) page of the palette tab.
    Page(i8),
}

/// The creative palette's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaletteKind {
    #[default]
    Building,
    Nature,
    Food,
    Tools,
    Items,
}

impl PaletteKind {
    const ALL: [PaletteKind; 5] = [
        PaletteKind::Building,
        PaletteKind::Nature,
        PaletteKind::Food,
        PaletteKind::Tools,
        PaletteKind::Items,
    ];

    fn label(self) -> &'static str {
        match self {
            PaletteKind::Building => "Building",
            PaletteKind::Nature => "Nature",
            PaletteKind::Food => "Food",
            PaletteKind::Tools => "Tools",
            PaletteKind::Items => "Items",
        }
    }

    /// Which tab an item goes on.
    fn of(
        def: &ourcraft_common::item::ItemDef,
        blocks: &ourcraft_common::block::BlockRegistry,
    ) -> Self {
        const NATURE: [&str; 16] = [
            "core:grass",
            "core:dirt",
            "core:sand",
            "core:gravel",
            "core:snow",
            "core:cactus",
            "core:leaves",
            "core:log",
            "core:clay",
            "core:water",
            "core:lava",
            "core:coal_ore",
            "core:iron_ore",
            "core:gold_ore",
            "core:diamond_ore",
            "core:bedrock",
        ];
        const TOOLS: [&str; 4] = [
            "core:shears",
            "core:bucket",
            "core:water_bucket",
            "core:lava_bucket",
        ];
        let plant = def
            .block()
            .is_some_and(|b| blocks.sits_on_ground(b) || blocks.grows(b));
        if def.food > 0 || def.heals > 0 || def.name == "core:cake" {
            PaletteKind::Food
        } else if def.tool().is_some() || TOOLS.contains(&def.name.as_str()) {
            PaletteKind::Tools
        } else if plant
            || NATURE.contains(&def.name.as_str())
            || matches!(
                def.name.as_str(),
                "core:pumpkin" | "core:melon" | "core:sugar_cane"
            )
        {
            PaletteKind::Nature
        } else if def.block().is_some() {
            PaletteKind::Building
        } else {
            PaletteKind::Items
        }
    }
}

/// The palette tab being looked at, and its page.
#[derive(Resource, Default, Debug)]
pub struct PaletteTab {
    pub kind: PaletteKind,
    pub page: usize,
}

/// Palette slots per page (five rows of nine).
const PALETTE_PAGE: usize = 45;

#[derive(Component)]
struct InventoryScreen;

/// The stack following the mouse pointer.
#[derive(Component)]
struct CursorIcon;

const SLOT: f32 = 52.0;

fn spawn_hotbar(mut commands: Commands) {
    commands.spawn((
        Name::new("Hotbar"),
        Hotbar,
        Node {
            position_type: PositionType::Absolute,
            bottom: px(12),
            width: percent(100),
            justify_content: JustifyContent::Center,
            column_gap: px(4),
            ..default()
        },
    ));
}

fn spawn_cursor_icon(mut commands: Commands) {
    commands.spawn((
        Name::new("Cursor stack"),
        CursorIcon,
        Node {
            position_type: PositionType::Absolute,
            width: px(40),
            height: px(40),
            ..default()
        },
        ImageNode::default(),
        Visibility::Hidden,
        GlobalZIndex(100),
        Pickable::IGNORE,
    ));
}

/// The count and wear bar drawn over an item picture.
fn spawn_stack_extras(
    s: &mut ChildSpawnerCommands,
    stack: ItemStack,
    items: &ItemRegistry,
    show_count: bool,
) {
    if show_count && stack.count > 1 {
        s.spawn((
            Text::new(stack.count.to_string()),
            TextFont::from_font_size(14.0),
            TextColor(Color::WHITE),
            TextShadow {
                offset: Vec2::splat(1.0),
                color: Color::BLACK,
            },
            Node {
                position_type: PositionType::Absolute,
                right: px(3),
                bottom: px(1),
                ..default()
            },
            Pickable::IGNORE,
        ));
    }
    let durability = items.get(stack.item).map_or(0, |d| d.durability);
    if durability > 0 && stack.wear > 0 {
        let left = 1.0 - stack.wear as f32 / durability as f32;
        let color = Color::srgb(1.0 - left, 0.3 + 0.6 * left, 0.2);
        s.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(6),
                bottom: px(4),
                width: px(38),
                height: px(4),
                ..default()
            },
            BackgroundColor(Color::BLACK),
            Pickable::IGNORE,
        ))
        .with_child((
            Node {
                width: percent(left * 100.0),
                height: percent(100),
                ..default()
            },
            BackgroundColor(color),
            Pickable::IGNORE,
        ));
    }
}

/// Spawn one slot's UI: a frame, the item picture, its count and wear.
/// A tab (or page arrow) of the creative palette.
fn tab_button(label: &str, on: bool, button: SlotButton) -> impl Bundle {
    (
        Button,
        button,
        Node {
            padding: UiRect::axes(px(10), px(5)),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(if on {
            Color::srgb(0.38, 0.5, 0.75)
        } else {
            Color::srgb(0.28, 0.3, 0.36)
        }),
        children![(
            Text::new(label),
            TextFont::from_font_size(15.0),
            Pickable::IGNORE,
        )],
    )
}

fn spawn_slot(
    parent: &mut ChildSpawnerCommands,
    stack: Option<ItemStack>,
    selected: bool,
    show_count: bool,
    icons: &ItemIcons,
    items: &ItemRegistry,
    button: Option<SlotButton>,
) {
    let mut slot = parent.spawn((
        Node {
            width: px(SLOT),
            height: px(SLOT),
            border: UiRect::all(px(if selected { 3.0 } else { 2.0 })),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.1, 0.1, 0.12, 0.6)),
        BorderColor::all(if selected {
            Color::WHITE
        } else {
            Color::srgba(0.0, 0.0, 0.0, 0.6)
        }),
    ));
    if let Some(b) = button {
        slot.insert((Button, b));
    }
    slot.with_children(|s| {
        let Some(stack) = stack else { return };
        if let Some(icon) = icons.0.get(&stack.item) {
            s.spawn((
                ImageNode::new(icon.clone()),
                Node {
                    width: px(40),
                    height: px(40),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        }
        spawn_stack_extras(s, stack, items, show_count);
    });
}

#[allow(clippy::too_many_arguments)]
fn rebuild_hotbar(
    mut commands: Commands,
    inventory: Res<PlayerInventory>,
    mode: Res<GameMode>,
    icons: Res<ItemIcons>,
    items: Res<Items>,
    tools: Res<crate::gamemode::ToolHotbar>,
    specs: Res<crate::gamemode::ToolSpecs>,
    hotbar: Single<Entity, With<Hotbar>>,
) {
    if !inventory.is_changed() && !mode.is_changed() && !icons.is_changed() && !tools.is_changed() {
        return;
    }
    if tools.active() {
        spawn_tool_hotbar(&mut commands, *hotbar, &tools.0, &specs, inventory.selected);
        return;
    }
    let survival = *mode == GameMode::Survival;
    commands
        .entity(*hotbar)
        .despawn_children()
        .with_children(|bar| {
            for i in 0..HOTBAR_SLOTS {
                let selected = i == inventory.selected;
                spawn_slot(
                    bar,
                    inventory.slots[i],
                    selected,
                    survival,
                    &icons,
                    &items,
                    None,
                );
            }
            // Name of the selected item, above the hotbar.
            if let Some(stack) = inventory.selected_stack() {
                bar.spawn((
                    Text::new(items.display_name(stack.item)),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::WHITE),
                    TextShadow {
                        offset: Vec2::splat(1.5),
                        color: Color::BLACK,
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: px(SLOT + 8.0),
                        ..default()
                    },
                ));
            }
        });
}

/// The hotbar while a game mode has given us tools.
fn spawn_tool_hotbar(
    commands: &mut Commands,
    hotbar: Entity,
    tools: &[String],
    specs: &crate::gamemode::ToolSpecs,
    selected: usize,
) {
    commands
        .entity(hotbar)
        .despawn_children()
        .with_children(|bar| {
            for (i, tool) in tools.iter().enumerate().take(HOTBAR_SLOTS) {
                let chosen = i == selected;
                bar.spawn((
                    Node {
                        width: px(SLOT),
                        height: px(SLOT),
                        border: UiRect::all(px(if chosen { 3.0 } else { 2.0 })),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.1, 0.1, 0.12, 0.6)),
                    BorderColor::all(if chosen {
                        Color::WHITE
                    } else {
                        Color::srgba(0.0, 0.0, 0.0, 0.6)
                    }),
                ))
                .with_children(|s| {
                    if let Some(icon) = specs.icons.get(tool) {
                        s.spawn((
                            ImageNode::new(icon.clone()),
                            Node {
                                width: px(40),
                                height: px(40),
                                ..default()
                            },
                        ));
                    }
                });
            }
            let name = tools.get(selected).map(|t| {
                specs
                    .specs
                    .get(t)
                    .map_or(t.clone(), |s| s.display_name.clone())
            });
            if let Some(name) = name {
                bar.spawn((
                    Text::new(name),
                    TextFont::from_font_size(16.0),
                    TextColor(Color::WHITE),
                    TextShadow {
                        offset: Vec2::splat(1.5),
                        color: Color::BLACK,
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: px(SLOT + 8.0),
                        ..default()
                    },
                ));
            }
        });
}

fn toggle_inventory_screen(
    actions: Res<Actions>,
    keys: Res<ButtonInput<KeyCode>>,
    tools: Res<crate::gamemode::ToolHotbar>,
    mut focus: ResMut<UiFocus>,
    mut screen: ResMut<OpenScreen>,
) {
    // Game modes with tools have no inventory to open.
    if tools.active() && !focus.inventory_open {
        return;
    }
    // Typing on a sign uses the keyboard (E included); the sign editor
    // closes itself.
    if focus.menu_open
        || focus.chat_open
        || (focus.inventory_open && matches!(*screen, OpenScreen::Sign(_)))
    {
        return;
    }
    if actions.toggle_inventory {
        focus.inventory_open = !focus.inventory_open;
        if focus.inventory_open {
            *screen = OpenScreen::Inventory;
        }
    } else if focus.inventory_open && keys.just_pressed(KeyCode::Escape) {
        focus.inventory_open = false;
    }
}

/// When a screen closes: put the crafting grid and the carried stack back
/// in the inventory (throwing out what doesn't fit), and tell the server
/// we've left the furnace.
#[allow(clippy::too_many_arguments)]
fn close_screen_cleanup(
    focus: Res<UiFocus>,
    screen: Res<OpenScreen>,
    items: Res<Items>,
    mut inventory: ResMut<PlayerInventory>,
    mut cursor: ResMut<Cursor>,
    mut grid: ResMut<CraftGrid>,
    mut furnace: ResMut<ContainerView>,
    conn: Option<ResMut<crate::net::Connection>>,
    mut was: Local<Option<OpenScreen>>,
) {
    let now = focus.inventory_open.then_some(*screen);
    if *was == now {
        return;
    }
    let before = std::mem::replace(&mut *was, now);
    let Some(before) = before else { return };
    let mut leftovers = Vec::new();
    let mut give = |stack: ItemStack, inventory: &mut PlayerInventory| {
        let left = inventory.add_stack(&items, stack);
        if left > 0 {
            leftovers.push(ItemStack {
                count: left,
                ..stack
            });
        }
    };
    for cell in grid.0.iter_mut() {
        if let Some(s) = cell.take() {
            give(s, &mut inventory);
        }
    }
    if let Some(s) = cursor.stack.take() {
        give(s, &mut inventory);
    }
    if let Some(mut conn) = conn {
        if matches!(before, OpenScreen::Container(_)) {
            furnace.0 = None;
            conn.send(Channel::Reliable, &ClientMessage::CloseContainer);
        }
        for s in leftovers {
            conn.send(Channel::Reliable, &ClientMessage::DropItems(s.into()));
        }
    }
}

/// Every item, for the creative palette (air, and hidden variants such as
/// wall torches and flowing water, are left out).
fn palette(items: &ItemRegistry, blocks: &ourcraft_common::block::BlockRegistry) -> Vec<ItemId> {
    use ourcraft_common::block::RenderKind;
    items
        .iter()
        .filter(|(id, d)| match d.kind {
            // Seeds and the like plant a hidden block, but are items of
            // their own.
            ItemKind::Block(b) if id.0 != b.0 => true,
            ItemKind::Block(b) => {
                let def = blocks.get(b);
                // Water and lava sources are fine; their flowing bits are hidden.
                def.render != RenderKind::Invisible && !def.hidden
            }
            _ => true,
        })
        .map(|(id, _)| id)
        .collect()
}

/// The crafting grid's cells as items, and what they make.
fn craft_result(grid: &CraftGrid, size: usize, recipes: &Recipes) -> Option<ItemStack> {
    if size == 0 {
        return None;
    }
    let cells: Vec<Option<ItemId>> = grid.0[..size * size]
        .iter()
        .map(|c| c.map(|s| s.item))
        .collect();
    recipes
        .find(&cells, size)
        .map(|(item, count)| ItemStack::new(item, count))
}

#[allow(clippy::too_many_arguments)]
fn rebuild_screen(
    mut commands: Commands,
    focus: Res<UiFocus>,
    screen: Res<OpenScreen>,
    inventory: Res<PlayerInventory>,
    mode: Res<GameMode>,
    icons: Res<ItemIcons>,
    items: Res<Items>,
    recipes: Res<Recipes>,
    blocks: Res<Blocks>,
    grid: Res<CraftGrid>,
    cursor: Res<Cursor>,
    furnace: Res<ContainerView>,
    mut tab: ResMut<PaletteTab>,
    existing: Query<Entity, With<InventoryScreen>>,
) {
    let changed = focus.is_changed()
        || inventory.is_changed()
        || mode.is_changed()
        || screen.is_changed()
        || grid.is_changed()
        || cursor.is_changed()
        || furnace.is_changed()
        || tab.is_changed()
        || icons.is_changed();
    if !changed {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !focus.inventory_open || matches!(*screen, OpenScreen::Sign(_)) {
        return;
    }
    let creative = *mode == GameMode::Creative;
    let palette_screen = creative && *screen == OpenScreen::Inventory;
    let title = match *screen {
        _ if palette_screen => "Creative: click an item to put it in your selected hotbar slot",
        OpenScreen::Inventory => "Inventory",
        OpenScreen::CraftingTable => "Crafting Table",
        OpenScreen::Container(_) => match furnace.0.as_ref().map(|v| v.kind) {
            Some(ContainerKind::Chest) => "Chest",
            _ => "Furnace",
        },
        OpenScreen::Sign(_) => "Sign",
    };
    let row = || Node {
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(4),
        row_gap: px(4),
        max_width: px(9.0 * (SLOT + 4.0)),
        ..default()
    };
    let label = |text: &str| {
        (
            Text::new(text),
            TextFont::from_font_size(14.0),
            TextColor(Color::srgb(0.85, 0.85, 0.85)),
        )
    };

    commands
        .spawn((
            Name::new("Inventory screen"),
            InventoryScreen,
            // Above the help text and everything else.
            GlobalZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        ))
        .with_children(|screen_node| {
            screen_node
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10),
                        padding: UiRect::all(px(16)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.18, 0.2, 0.25)),
                ))
                .with_children(|panel| {
                    panel.spawn((Text::new(title), TextFont::from_font_size(18.0)));

                    // ---- Top part: crafting, furnace or palette ----
                    if palette_screen {
                        let all: Vec<ItemId> = palette(&items, &blocks.0)
                            .into_iter()
                            .filter(|i| {
                                items
                                    .get(*i)
                                    .is_some_and(|d| PaletteKind::of(d, &blocks.0) == tab.kind)
                            })
                            .collect();
                        let pages = all.len().div_ceil(PALETTE_PAGE).max(1);
                        let page = tab.page.min(pages - 1);
                        // Past the last page: stay on the last one.
                        tab.bypass_change_detection().page = page;
                        // Tabs along the top, and arrows for more pages.
                        panel
                            .spawn(Node {
                                column_gap: px(6),
                                ..default()
                            })
                            .with_children(|tabs| {
                                for kind in PaletteKind::ALL {
                                    let on = kind == tab.kind;
                                    tabs.spawn(tab_button(kind.label(), on, SlotButton::Tab(kind)));
                                }
                                if pages > 1 {
                                    tabs.spawn(tab_button("<", false, SlotButton::Page(-1)));
                                    tabs.spawn((
                                        Text::new(format!("{}/{pages}", page + 1)),
                                        TextFont::from_font_size(14.0),
                                    ));
                                    tabs.spawn(tab_button(">", false, SlotButton::Page(1)));
                                }
                            });
                        panel
                            .spawn(Node {
                                min_height: px(5.0 * (SLOT + 4.0)),
                                align_content: AlignContent::FlexStart,
                                ..row()
                            })
                            .with_children(|g| {
                                for item in all.iter().skip(page * PALETTE_PAGE).take(PALETTE_PAGE)
                                {
                                    let stack = Some(ItemStack::new(*item, 1));
                                    let b = Some(SlotButton::Palette(*item));
                                    spawn_slot(g, stack, false, false, &icons, &items, b);
                                }
                            });
                    } else if let OpenScreen::Container(_) = *screen {
                        match furnace.0.as_ref().map(|v| v.kind) {
                            Some(ContainerKind::Chest) => {
                                spawn_chest(panel, &furnace, &icons, &items)
                            }
                            _ => spawn_furnace(panel, &furnace, &icons, &items),
                        }
                    } else {
                        let size = screen.grid_size();
                        let result = craft_result(&grid, size, &recipes);
                        panel
                            .spawn(Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: px(16),
                                ..default()
                            })
                            .with_children(|craft| {
                                craft
                                    .spawn(Node {
                                        display: Display::Grid,
                                        grid_template_columns: RepeatedGridTrack::px(
                                            size as u16,
                                            SLOT,
                                        ),
                                        column_gap: px(4),
                                        row_gap: px(4),
                                        ..default()
                                    })
                                    .with_children(|g| {
                                        for i in 0..size * size {
                                            let b = Some(SlotButton::Craft(i));
                                            spawn_slot(
                                                g, grid.0[i], false, true, &icons, &items, b,
                                            );
                                        }
                                    });
                                craft.spawn((Text::new(">"), TextFont::from_font_size(28.0)));
                                spawn_slot(
                                    craft,
                                    result,
                                    false,
                                    true,
                                    &icons,
                                    &items,
                                    Some(SlotButton::CraftResult),
                                );
                            });
                    }

                    // ---- Bottom part: the player's items ----
                    if !palette_screen {
                        panel.spawn(label("Backpack"));
                        panel.spawn(row()).with_children(|g| {
                            for i in HOTBAR_SLOTS..INVENTORY_SLOTS {
                                let b = Some(SlotButton::Inventory(i));
                                spawn_slot(g, inventory.slots[i], false, true, &icons, &items, b);
                            }
                        });
                    }
                    panel.spawn(label("Hotbar"));
                    panel.spawn(row()).with_children(|g| {
                        for i in 0..HOTBAR_SLOTS {
                            let selected = i == inventory.selected;
                            let b = Some(SlotButton::Inventory(i));
                            spawn_slot(
                                g,
                                inventory.slots[i],
                                selected,
                                !creative,
                                &icons,
                                &items,
                                b,
                            );
                        }
                    });
                    panel.spawn((
                        Text::new(if cfg!(any(target_os = "ios", target_os = "android")) {
                            "Tap Bag to close"
                        } else if palette_screen {
                            "Press E to close"
                        } else {
                            "Click: pick up / put down.  Right click: half / one.  E: close"
                        }),
                        TextFont::from_font_size(13.0),
                        TextColor(Color::srgb(0.7, 0.7, 0.7)),
                    ));
                });
        });
}

/// A chest: three rows of nine.
fn spawn_chest(
    panel: &mut ChildSpawnerCommands,
    view: &ContainerView,
    icons: &ItemIcons,
    items: &ItemRegistry,
) {
    let Some(view) = &view.0 else { return };
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(4),
            row_gap: px(4),
            max_width: px(9.0 * (SLOT + 4.0)),
            ..default()
        })
        .with_children(|g| {
            for (i, slot) in view.slots.iter().enumerate() {
                let stack = slot.map(ItemStack::from);
                spawn_slot(
                    g,
                    stack,
                    false,
                    true,
                    icons,
                    items,
                    Some(SlotButton::Container(i)),
                );
            }
        });
}

/// The furnace: input above fuel, a flame and an arrow, the output.
fn spawn_furnace(
    panel: &mut ChildSpawnerCommands,
    furnace: &ContainerView,
    icons: &ItemIcons,
    items: &ItemRegistry,
) {
    let Some(view) = &furnace.0 else {
        panel.spawn((
            Text::new("Lighting the fire..."),
            TextFont::from_font_size(14.0),
        ));
        return;
    };
    let slot = |i: usize| view.slots.get(i).copied().flatten().map(ItemStack::from);
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(16),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(6),
                ..default()
            })
            .with_children(|col| {
                let b = Some(SlotButton::Container(FURNACE_INPUT));
                spawn_slot(col, slot(FURNACE_INPUT), false, true, icons, items, b);
                // The flame: how much fire is left.
                col.spawn((
                    Node {
                        width: px(14),
                        height: px(22),
                        flex_direction: FlexDirection::ColumnReverse,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.25, 0.22, 0.2)),
                ))
                .with_child((
                    Node {
                        width: percent(100),
                        height: percent(view.burn * 100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(1.0, 0.55, 0.1)),
                ));
                let b = Some(SlotButton::Container(FURNACE_FUEL));
                spawn_slot(col, slot(FURNACE_FUEL), false, true, icons, items, b);
            });
            // The arrow: how far the current item has cooked.
            row.spawn((
                Node {
                    width: px(60),
                    height: px(12),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.3, 0.3, 0.32)),
            ))
            .with_child((
                Node {
                    width: percent(view.smelt * 100.0),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
            ));
            let b = Some(SlotButton::Container(FURNACE_OUTPUT));
            spawn_slot(row, slot(FURNACE_OUTPUT), false, true, icons, items, b);
        });
    if view.kind == ContainerKind::Furnace {
        panel.spawn((
            Text::new("Top: what to cook (ore, sand, clay...).  Bottom: fuel (coal, wood...)."),
            TextFont::from_font_size(13.0),
            TextColor(Color::srgb(0.7, 0.7, 0.7)),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn slot_clicks(
    buttons: Query<(&Interaction, &SlotButton)>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    focus: Res<UiFocus>,
    screen: Res<OpenScreen>,
    items: Res<Items>,
    recipes: Res<Recipes>,
    mode: Res<GameMode>,
    mut inventory: ResMut<PlayerInventory>,
    mut cursor: ResMut<Cursor>,
    mut grid: ResMut<CraftGrid>,
    mut tab: ResMut<PaletteTab>,
    conn: Option<ResMut<crate::net::Connection>>,
) {
    if !focus.inventory_open {
        return;
    }
    let main = mouse.just_pressed(MouseButton::Left) || touches.any_just_pressed();
    let secondary = mouse.just_pressed(MouseButton::Right);
    if !main && !secondary {
        return;
    }
    let Some(button) = buttons
        .iter()
        .find(|(i, _)| **i != Interaction::None)
        .map(|(_, b)| *b)
    else {
        return;
    };
    let creative = *mode == GameMode::Creative;
    match button {
        SlotButton::Tab(kind) => {
            *tab = PaletteTab { kind, page: 0 };
        }
        SlotButton::Page(step) => {
            tab.page = tab.page.saturating_add_signed(step as isize);
        }
        SlotButton::Palette(item) => {
            let selected = inventory.selected;
            let count = items.max_stack(item);
            inventory.slots[selected] = Some(ItemStack::new(item, count));
        }
        SlotButton::Inventory(i) if creative && *screen == OpenScreen::Inventory => {
            // The creative palette screen: clicking the hotbar selects.
            inventory.select(i);
        }
        SlotButton::Inventory(i) => {
            let (slot, held) = click_slot(&items, inventory.slots[i], cursor.stack, secondary);
            inventory.slots[i] = slot;
            cursor.stack = held;
        }
        SlotButton::Craft(i) => {
            let (slot, held) = click_slot(&items, grid.0[i], cursor.stack, secondary);
            grid.0[i] = slot;
            cursor.stack = held;
        }
        SlotButton::CraftResult => {
            let size = screen.grid_size();
            let Some(result) = craft_result(&grid, size, &recipes) else {
                return;
            };
            // Take the result onto the cursor (adding to a matching stack).
            let new_cursor = match cursor.stack {
                None => result,
                Some(c)
                    if c.stacks_with(&result)
                        && c.count + result.count <= items.max_stack(c.item) =>
                {
                    ItemStack {
                        count: c.count + result.count,
                        ..c
                    }
                }
                Some(_) => return,
            };
            cursor.stack = Some(new_cursor);
            for cell in grid.0[..size * size].iter_mut() {
                if let Some(s) = cell {
                    s.count -= 1;
                    // Milk buckets give their buckets back.
                    let left = items
                        .get(s.item)
                        .and_then(|d| d.leaves.as_deref())
                        .and_then(|n| items.by_name(n));
                    if s.count == 0 {
                        *cell = left.map(|l| ItemStack::new(l, 1));
                    } else if let Some(l) = left {
                        inventory.add(&items, l, 1);
                    }
                }
            }
        }
        SlotButton::Container(i) => {
            if cursor.pending {
                return;
            }
            let Some(mut conn) = conn else { return };
            conn.send(
                Channel::Reliable,
                &ClientMessage::ContainerClick {
                    slot: i as u8,
                    cursor: cursor.stack.map(NetStack::from),
                    secondary,
                },
            );
            // The server now holds what we carried; it sends back the
            // new cursor.
            cursor.pending = true;
        }
    }
}

fn move_cursor_icon(
    cursor: Res<Cursor>,
    focus: Res<UiFocus>,
    icons: Res<ItemIcons>,
    items: Res<Items>,
    window: Single<&Window>,
    touches: Res<Touches>,
    mut commands: Commands,
    mut icon: Single<(Entity, &mut Node, &mut ImageNode, &mut Visibility), With<CursorIcon>>,
) {
    let (entity, node, image, visibility) = &mut *icon;
    let stack = cursor.stack.filter(|_| focus.inventory_open);
    let Some(stack) = stack else {
        **visibility = Visibility::Hidden;
        return;
    };
    let pointer = window
        .cursor_position()
        .or_else(|| touches.iter().next().map(|t| t.position()));
    if let Some(p) = pointer {
        node.left = px(p.x - 20.0);
        node.top = px(p.y - 20.0);
    }
    **visibility = Visibility::Visible;
    if cursor.is_changed() || icons.is_changed() {
        if let Some(h) = icons.0.get(&stack.item) {
            image.image = h.clone();
        }
        commands
            .entity(*entity)
            .despawn_children()
            .with_children(|s| spawn_stack_extras(s, stack, &items, true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::textures::generate_texture;

    #[test]
    fn icon_covers_a_cube_shape() {
        let px = render_block_icon(&generate_texture("stone"), &generate_texture("dirt"));
        let alpha = |x: u32, y: u32| px[((y * ICON + x) * 4 + 3) as usize];
        assert_eq!(alpha(16, 2), 255, "top face");
        assert_eq!(alpha(4, 20), 255, "left face");
        assert_eq!(alpha(28, 20), 255, "right face");
        assert_eq!(alpha(1, 1), 0, "corner is transparent");
    }

    #[test]
    fn a_box_icon_of_a_cube_looks_like_the_cube_icon() {
        let (top, side) = (generate_texture("stone"), generate_texture("dirt"));
        let cube = render_block_icon(&top, &side);
        let boxes = render_boxes_icon(&top, &side, &[([0.0; 3], [1.0; 3])]);
        let differ = cube
            .chunks(4)
            .zip(boxes.chunks(4))
            .filter(|(a, b)| a[3] != b[3])
            .count();
        // Only a few edge pixels may round differently.
        assert!(differ < 40, "{differ} pixels differ");
    }

    #[test]
    fn every_item_gets_a_picture() {
        let blocks = ourcraft_common::block::BlockRegistry::with_builtin_blocks();
        let items = ItemRegistry::build(&blocks, &[]);
        let tex = crate::render::ModTextures::default();
        for (id, def) in items.iter().skip(1) {
            if def
                .block()
                .is_some_and(|b| blocks.get(b).textures.is_none())
            {
                continue;
            }
            let px = item_icon_pixels(&items, &blocks, &tex, id).expect(&def.name);
            assert_eq!(px.len(), (ICON * ICON * 4) as usize);
        }
    }
}

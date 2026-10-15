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
            .init_resource::<CraftGrid>()
            .init_resource::<FurnaceView>()
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
    Furnace(IVec3),
}

impl OpenScreen {
    /// Size of the crafting grid on this screen (0 = none).
    fn grid_size(self) -> usize {
        match self {
            OpenScreen::Inventory => 2,
            OpenScreen::CraftingTable => 3,
            OpenScreen::Furnace(_) => 0,
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

/// The open furnace, as the server last described it.
#[derive(Resource, Default, Debug)]
pub struct FurnaceView(pub Option<NetContainer>);

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
        ItemKind::Block(b) => {
            let tex = blocks.get(b).textures.as_ref()?;
            Some(render_block_icon(
                &textures.pixels(&tex.top),
                &textures.pixels(&tex.side),
            ))
        }
        _ => {
            let short = def.name.trim_start_matches("core:");
            let flat = crate::render::item_icons::item_icon(short)
                // Mods can give an item a texture with the item's name.
                .unwrap_or_else(|| textures.pixels(&def.name));
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
}

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
    if focus.menu_open || focus.chat_open {
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
    mut furnace: ResMut<FurnaceView>,
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
        if matches!(before, OpenScreen::Furnace(_)) {
            furnace.0 = None;
            conn.send(Channel::Reliable, &ClientMessage::CloseContainer);
        }
        for s in leftovers {
            conn.send(Channel::Reliable, &ClientMessage::DropItems(s.into()));
        }
    }
}

/// Every item, for the creative palette (blocks you can't hold, like air
/// and water, are left out).
fn palette(items: &ItemRegistry, blocks: &ourcraft_common::block::BlockRegistry) -> Vec<ItemId> {
    use ourcraft_common::block::RenderKind;
    items
        .iter()
        .filter(|(_, d)| match d.kind {
            ItemKind::Block(b) => {
                let def = blocks.get(b);
                def.render != RenderKind::Invisible && !def.liquid
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
    furnace: Res<FurnaceView>,
    existing: Query<Entity, With<InventoryScreen>>,
) {
    let changed = focus.is_changed()
        || inventory.is_changed()
        || mode.is_changed()
        || screen.is_changed()
        || grid.is_changed()
        || cursor.is_changed()
        || furnace.is_changed()
        || icons.is_changed();
    if !changed {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !focus.inventory_open {
        return;
    }
    let creative = *mode == GameMode::Creative;
    let palette_screen = creative && *screen == OpenScreen::Inventory;
    let title = match *screen {
        _ if palette_screen => "Creative: click an item to put it in your selected hotbar slot",
        OpenScreen::Inventory => "Inventory",
        OpenScreen::CraftingTable => "Crafting Table",
        OpenScreen::Furnace(_) => "Furnace",
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
                        panel.spawn(row()).with_children(|g| {
                            for item in palette(&items, &blocks.0) {
                                let stack = Some(ItemStack::new(item, 1));
                                let b = Some(SlotButton::Palette(item));
                                spawn_slot(g, stack, false, false, &icons, &items, b);
                            }
                        });
                    } else if let OpenScreen::Furnace(_) = *screen {
                        spawn_furnace(panel, &furnace, &icons, &items);
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

/// The furnace: input above fuel, a flame and an arrow, the output.
fn spawn_furnace(
    panel: &mut ChildSpawnerCommands,
    furnace: &FurnaceView,
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
                    if s.count == 0 {
                        *cell = None;
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

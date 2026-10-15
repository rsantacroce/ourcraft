//! The crosshair, hotbar and inventory screen.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::block::{BlockId, BlockRegistry, RenderKind};
use ourcraft_common::inventory::{HOTBAR_SLOTS, INVENTORY_SLOTS, ItemStack};

use crate::input::{Actions, UiFocus};
use crate::interaction::PlayerInventory;
use crate::player::GameMode;
use crate::render::textures::TEXTURE_SIZE;
use crate::world::Blocks;

pub struct InventoryUiPlugin;

impl Plugin for InventoryUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            (build_icons, spawn_crosshair, spawn_hotbar).chain(),
        )
        // Mods can add blocks when we join a server: redraw the icons.
        .add_systems(Update, build_icons.run_if(resource_changed::<Blocks>))
        .add_systems(
            Update,
            (
                toggle_inventory_screen,
                inventory_clicks,
                rebuild_hotbar,
                rebuild_inventory_screen,
            )
                .chain(),
        );
    }
}

/// A small isometric picture of each block, for the hotbar and inventory.
#[derive(Resource, Default)]
pub struct BlockIcons(pub HashMap<BlockId, Handle<Image>>);

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

fn build_icons(
    mut commands: Commands,
    blocks: Res<Blocks>,
    textures: Option<Res<crate::render::ModTextures>>,
    mut images: ResMut<Assets<Image>>,
) {
    let textures = textures.map(|t| t.clone()).unwrap_or_default();
    let mut icons = BlockIcons::default();
    for (id, def) in blocks.0.iter() {
        let Some(tex) = &def.textures else { continue };
        let pixels = render_block_icon(&textures.pixels(&tex.top), &textures.pixels(&tex.side));
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
        icons.0.insert(id, images.add(image));
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

/// A clickable inventory slot.
#[derive(Component, Clone, Copy)]
enum SlotButton {
    /// One of the player's 36 inventory slots.
    Inventory(usize),
    /// A block in the creative palette.
    Palette(BlockId),
}

#[derive(Component)]
struct InventoryScreen;

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

/// Spawn one slot's UI: a frame, the block icon and (in survival) a count.
fn spawn_slot(
    parent: &mut ChildSpawnerCommands,
    stack: Option<ItemStack>,
    selected: bool,
    show_count: bool,
    icons: &BlockIcons,
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
        if let Some(icon) = icons.0.get(&stack.block) {
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
    });
}

fn rebuild_hotbar(
    mut commands: Commands,
    inventory: Res<PlayerInventory>,
    mode: Res<GameMode>,
    icons: Res<BlockIcons>,
    blocks: Res<Blocks>,
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
                spawn_slot(bar, inventory.slots[i], selected, survival, &icons, None);
            }
            // Name of the selected block, above the hotbar.
            if let Some(stack) = inventory.selected_stack() {
                bar.spawn((
                    Text::new(&blocks.0.get(stack.block).display_name),
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
) {
    // Game modes with tools have no block inventory to open.
    if tools.active() && !focus.inventory_open {
        return;
    }
    if focus.menu_open || focus.chat_open {
        return;
    }
    if actions.toggle_inventory {
        focus.inventory_open = !focus.inventory_open;
    } else if focus.inventory_open && keys.just_pressed(KeyCode::Escape) {
        focus.inventory_open = false;
    }
}

/// Blocks offered in the creative palette.
fn palette(reg: &BlockRegistry) -> Vec<BlockId> {
    reg.iter()
        .filter(|(_, d)| d.render != RenderKind::Invisible && !d.liquid)
        .map(|(id, _)| id)
        .collect()
}

fn rebuild_inventory_screen(
    mut commands: Commands,
    focus: Res<UiFocus>,
    inventory: Res<PlayerInventory>,
    mode: Res<GameMode>,
    icons: Res<BlockIcons>,
    blocks: Res<Blocks>,
    existing: Query<Entity, With<InventoryScreen>>,
) {
    if !focus.is_changed() && !inventory.is_changed() && !mode.is_changed() {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    if !focus.inventory_open {
        return;
    }
    let survival = *mode == GameMode::Survival;
    let title = if survival {
        "Inventory: click a slot to swap it with your selected hotbar slot"
    } else {
        "Creative: click a block to put it in your selected hotbar slot"
    };

    let row = || Node {
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(4),
        row_gap: px(4),
        max_width: px(9.0 * (SLOT + 4.0)),
        ..default()
    };

    commands
        .spawn((
            Name::new("Inventory screen"),
            InventoryScreen,
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
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(12),
                        padding: UiRect::all(px(16)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.18, 0.2, 0.25, 0.95)),
                ))
                .with_children(|panel| {
                    panel.spawn((Text::new(title), TextFont::from_font_size(16.0)));
                    panel.spawn(row()).with_children(|grid| {
                        if survival {
                            for i in HOTBAR_SLOTS..INVENTORY_SLOTS {
                                let b = Some(SlotButton::Inventory(i));
                                spawn_slot(grid, inventory.slots[i], false, true, &icons, b);
                            }
                        } else {
                            for block in palette(&blocks.0) {
                                let stack = Some(ItemStack { block, count: 1 });
                                let b = Some(SlotButton::Palette(block));
                                spawn_slot(grid, stack, false, false, &icons, b);
                            }
                        }
                    });
                    panel.spawn((Text::new("Hotbar"), TextFont::from_font_size(14.0)));
                    panel.spawn(row()).with_children(|grid| {
                        for i in 0..HOTBAR_SLOTS {
                            let selected = i == inventory.selected;
                            let b = Some(SlotButton::Inventory(i));
                            spawn_slot(grid, inventory.slots[i], selected, survival, &icons, b);
                        }
                    });
                    panel.spawn((
                        Text::new(if cfg!(any(target_os = "ios", target_os = "android")) {
                            "Tap Bag to close"
                        } else {
                            "Press E to close"
                        }),
                        TextFont::from_font_size(13.0),
                        TextColor(Color::srgb(0.7, 0.7, 0.7)),
                    ));
                });
        });
}

fn inventory_clicks(
    buttons: Query<(&Interaction, &SlotButton), Changed<Interaction>>,
    mut inventory: ResMut<PlayerInventory>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let selected = inventory.selected;
        match *button {
            SlotButton::Inventory(i) if i < HOTBAR_SLOTS => inventory.select(i),
            SlotButton::Inventory(i) => inventory.swap(i, selected),
            SlotButton::Palette(block) => {
                inventory.slots[selected] = Some(ItemStack { block, count: 64 });
            }
        }
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
}

//! Looking at, breaking and placing blocks, and using crafting tables and
//! furnaces.
//!
//! Interaction doesn't change the world directly. It sends a
//! [`BlockEditRequest`] message, and whoever owns the world applies it: in
//! single player that's a local system; from M6 it's the server, which can
//! refuse (say, if you're too far away). Keeping that split from the start
//! makes multiplayer a much smaller change.
//!
//! In survival, breaking takes time (depending on the block and the tool
//! in hand), shown as cracks spreading over the block. What it drops is up
//! to the server: it appears as an item to pick up.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::block::{BlockId, blocks};
use ourcraft_common::inventory::{Inventory, ItemStack};
use ourcraft_common::item::ItemId;
use ourcraft_common::physics::Aabb;
use ourcraft_common::protocol::{Channel, ClientMessage};
use ourcraft_common::raycast::{RayHit, raycast};

use crate::input::{Actions, UiFocus};
use crate::inventory_ui::OpenScreen;
use crate::items::Items;
use crate::player::{GameMode, Player, PlayerCamera};
use crate::world::{Blocks, ClientWorld};

/// How far away you can reach blocks.
pub const REACH: f32 = 6.0;
/// Seconds between repeated breaks/places while the button is held.
const REPEAT_DELAY: f32 = 0.22;
/// Crack pictures, from a scratch to nearly broken.
const CRACK_STAGES: usize = 6;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BlockEditRequest>()
            .init_resource::<Target>()
            .init_resource::<SourceTarget>()
            .init_resource::<TargetMob>()
            .init_resource::<BreakProgress>()
            .insert_resource(PlayerInventory(creative_hotbar()))
            .add_systems(Startup, setup_cracks)
            .add_systems(
                Update,
                (
                    update_target,
                    hotbar_selection,
                    use_blocks,
                    break_and_place,
                    use_tools,
                    drop_items,
                    draw_highlight,
                    draw_cracks,
                )
                    .chain()
                    .after(crate::player::PlayerUpdateSet),
            );
    }
}

/// "Please set this block." Sent by interaction, applied by the world owner.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEditRequest {
    pub pos: IVec3,
    pub block: BlockId,
    /// The item in hand (decides what a broken block drops).
    pub held: ItemId,
}

/// The block under the crosshair, if any.
#[derive(Resource, Default, Debug)]
pub struct Target(pub Option<RayHit>);

/// Like [`Target`], but still water and lava count too (for buckets).
#[derive(Resource, Default, Debug)]
pub struct SourceTarget(pub Option<RayHit>);

/// The animal under the crosshair (when it's nearer than any block), and
/// where its feet are.
#[derive(Resource, Default, Debug)]
pub struct TargetMob(pub Option<(crate::entities::Mob, Vec3)>);

/// The local player's inventory.
#[derive(Resource, Debug, Deref, DerefMut)]
pub struct PlayerInventory(pub Inventory);

/// How far along breaking the targeted block is (survival).
#[derive(Resource, Default, Debug)]
pub struct BreakProgress {
    pub pos: Option<IVec3>,
    /// Seconds spent so far, and seconds needed.
    pub time: f32,
    pub needed: f32,
}

impl BreakProgress {
    pub fn fraction(&self) -> f32 {
        if self.needed > 0.0 {
            (self.time / self.needed).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// A hotbar with a good selection of building blocks.
pub fn creative_hotbar() -> Inventory {
    let mut inv = Inventory::default();
    let picks = [
        blocks::GRASS,
        blocks::DIRT,
        blocks::STONE,
        blocks::COBBLESTONE,
        blocks::PLANKS,
        blocks::LOG,
        blocks::GLASS,
        blocks::BRICKS,
        blocks::LAMP,
    ];
    for (slot, block) in inv.slots.iter_mut().zip(picks) {
        *slot = Some(ItemStack::new(ItemId::from_block(block), 64));
    }
    inv
}

fn update_target(
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    camera: Single<&GlobalTransform, With<PlayerCamera>>,
    player: Single<&Player>,
    mobs: Query<(&crate::entities::Mob, &Transform)>,
    mut target: ResMut<Target>,
    mut source: ResMut<SourceTarget>,
    mut target_mob: ResMut<TargetMob>,
) {
    // Use the up-to-date look direction (the camera transform may lag a
    // frame behind the mouse).
    let origin = camera.translation();
    let dir = player.look_direction();
    target.0 = raycast(&world.0, &blocks.0, origin, dir, REACH);
    source.0 = ourcraft_common::raycast::raycast_sources(&world.0, &blocks.0, origin, dir, REACH)
        .filter(|h| blocks.0.is_fluid_source(h.id));
    let block_dist = target
        .0
        .and_then(|h| Aabb::block(h.block).ray_hit(origin, dir, REACH))
        .unwrap_or(REACH);
    // An animal in front of the block wins.
    target_mob.0 = mobs
        .iter()
        .filter_map(|(m, t)| {
            let feet = t.translation;
            let half = m.size[0] / 2.0;
            let aabb = Aabb {
                min: feet - Vec3::new(half, 0.0, half),
                max: feet + Vec3::new(half, m.size[1], half),
            };
            Some((aabb.ray_hit(origin, dir, block_dist)?, m, feet))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, m, feet)| (m.clone(), feet));
    if target_mob.0.is_some() {
        target.0 = None;
    }
}

fn hotbar_selection(actions: Res<Actions>, mut inventory: ResMut<PlayerInventory>) {
    if let Some(slot) = actions.hotbar_select {
        inventory.select(slot);
    }
    if actions.hotbar_scroll != 0 {
        inventory.scroll(actions.hotbar_scroll);
    }
}

/// Placing against a crafting table or furnace opens it instead (hold the
/// "down" key to place a block against it after all).
#[allow(clippy::too_many_arguments)]
fn use_blocks(
    actions: Res<Actions>,
    (target, source): (Res<Target>, Res<SourceTarget>),
    target_mob: Res<TargetMob>,
    defs: Res<crate::entities::MobDefs>,
    items: Res<Items>,
    mode: Res<GameMode>,
    mut inventory: ResMut<PlayerInventory>,
    rules: Res<crate::gamemode::Rules>,
    tools: Res<crate::gamemode::ToolHotbar>,
    mut focus: ResMut<UiFocus>,
    mut screen: ResMut<OpenScreen>,
    mut container: ResMut<crate::inventory_ui::ContainerView>,
    registry: Res<Blocks>,
    (conn, player): (Option<ResMut<crate::net::Connection>>, Single<&Player>),
    mut held_before: Local<bool>,
) {
    // Only on a fresh press, so holding "place" doesn't reopen the screen.
    let fresh = actions.place_block && !*held_before;
    *held_before = actions.place_block;
    if !fresh || tools.active() || !rules.0.can_build || actions.descend || focus.gameplay_blocked()
    {
        return;
    }
    if let Some((mob, _)) = &target_mob.0 {
        let Some(mut conn) = conn else { return };
        let held = inventory.held();
        conn.send(
            Channel::Reliable,
            &ClientMessage::UseEntity {
                id: mob.id,
                held: held.unwrap_or(ItemId::NONE).0,
            },
        );
        // Shearing wears the shears (the server drops the wool).
        let shearable = defs
            .0
            .get(&mob.kind)
            .is_some_and(|d| d.shears_into.is_some());
        let shears = held.is_some_and(|h| items.name(h) == "core:shears");
        let survival = *mode == GameMode::Survival;
        if shears && shearable && !mob.sheared && !mob.baby && survival {
            inventory.wear_selected(&items);
        }
        // A bucket on a cow: milk.
        let bucket = held.is_some_and(|h| items.name(h) == "core:bucket");
        if bucket
            && mob.kind == "core:cow"
            && !mob.baby
            && let Some(milk) = items.by_name("core:milk_bucket")
        {
            inventory.exchange_selected(&items, milk);
        }
        return;
    }
    let held = inventory.held().unwrap_or(ItemId::NONE);
    let survival = *mode == GameMode::Survival;
    // A bow shoots an arrow (in survival, if you have one).
    if items.name(held) == "core:bow" {
        let Some(mut conn) = conn else { return };
        let arrow = items.by_name("core:arrow");
        let has_arrow = arrow.is_some_and(|a| inventory.count(a) > 0);
        if survival && !has_arrow {
            return;
        }
        if survival && let Some(a) = arrow {
            take_one(&mut inventory, a);
            inventory.wear_selected(&items);
        }
        conn.send(
            Channel::Reliable,
            &ClientMessage::UseItem {
                held: held.0,
                direction: player.look_direction().to_array(),
            },
        );
        return;
    }
    // A fishing rod casts into the water you point at (or reels in).
    if items.name(held) == "core:fishing_rod" {
        let Some(mut conn) = conn else { return };
        let spot = source.0.or(target.0).map_or(IVec3::ZERO, |h| h.block);
        if survival {
            inventory.wear_selected(&items);
        }
        conn.send(
            Channel::Reliable,
            &ClientMessage::UseBlock {
                pos: spot.to_array(),
                held: held.0,
            },
        );
        return;
    }
    // Food goes on a campfire to cook.
    if let Some(hit) = target.0
        && registry.0.get(hit.id).name == "core:campfire"
        && ourcraft_common::adventure::campfire_result(&items, items.name(held)).is_some()
        && let Some(mut conn) = conn
    {
        conn.send(
            Channel::Reliable,
            &ClientMessage::UseBlock {
                pos: hit.block.to_array(),
                held: held.0,
            },
        );
        return;
    }
    // A mine cart goes on the rail you point at.
    if items.name(held) == "core:minecart"
        && let Some(hit) = target.0
        && registry.0.is_rail(hit.id)
        && let Some(mut conn) = conn
    {
        conn.send(
            Channel::Reliable,
            &ClientMessage::UseBlock {
                pos: hit.block.to_array(),
                held: held.0,
            },
        );
        return;
    }
    // A boat goes on the water (or the ground) you point at.
    if items.name(held).ends_with("_boat")
        && let Some(spot) = source.0.or(target.0)
        && let Some(mut conn) = conn
    {
        conn.send(
            Channel::Reliable,
            &ClientMessage::UseBlock {
                pos: spot.block.to_array(),
                held: held.0,
            },
        );
        return;
    }
    let Some(hit) = target.0 else { return };
    let Some(mut conn) = conn else { return };
    let pos = hit.block.to_array();
    // Cakes, composters, and bone meal on plants: the server does it.
    if used_in_place(&registry.0, hit.id, items.name(held)) {
        conn.send(
            Channel::Reliable,
            &ClientMessage::UseBlock { pos, held: held.0 },
        );
        return;
    }
    use ourcraft_common::shapes::VariantKind;
    let opened = match (usable(&registry.0, hit.id), registry.0.variant_kind(hit.id)) {
        (false, _) => return,
        (
            _,
            Some(
                VariantKind::Door
                | VariantKind::Trapdoor
                | VariantKind::FenceGate
                | VariantKind::Bed,
            ),
        ) => {
            conn.send(
                Channel::Reliable,
                &ClientMessage::UseBlock { pos, held: held.0 },
            );
            return;
        }
        (_, Some(VariantKind::Sign | VariantKind::WallSign)) => OpenScreen::Sign(hit.block),
        // Levers, buttons and note blocks: the server does it.
        (_, Some(VariantKind::Lever | VariantKind::Button)) => {
            conn.send(
                Channel::Reliable,
                &ClientMessage::UseBlock { pos, held: held.0 },
            );
            return;
        }
        _ if registry.0.get(hit.id).name == "core:note_block" => {
            conn.send(
                Channel::Reliable,
                &ClientMessage::UseBlock { pos, held: held.0 },
            );
            return;
        }
        _ if hit.id == blocks::CRAFTING_TABLE => OpenScreen::CraftingTable,
        // Campfires only take food, pots plants (see above).
        _ if matches!(
            registry.0.get(hit.id).name.as_str(),
            "core:campfire" | "core:flower_pot"
        ) =>
        {
            return;
        }
        _ => {
            // Furnaces and chests live on the server.
            container.0 = None;
            conn.send(Channel::Reliable, &ClientMessage::OpenContainer { pos });
            OpenScreen::Container(hit.block)
        }
    };
    *screen = opened;
    focus.inventory_open = true;
}

/// Use up one of an item, from wherever it is in the inventory.
fn take_one(inventory: &mut PlayerInventory, item: ItemId) {
    if let Some(slot) = inventory
        .slots
        .iter_mut()
        .find(|s| s.is_some_and(|s| s.item == item))
    {
        let s = slot.as_mut().expect("found");
        s.count -= 1;
        if s.count == 0 {
            *slot = None;
        }
    }
}

/// Using a block that changes right where it is: eating cake, filling a
/// composter, giving a plant (or grass) bone meal.
pub fn used_in_place(
    registry: &ourcraft_common::block::BlockRegistry,
    id: BlockId,
    held: &str,
) -> bool {
    use ourcraft_common::farming::StageKind;
    let stepped = registry
        .stage(id)
        .is_some_and(|s| matches!(s.kind, StageKind::Cake | StageKind::Composter));
    let feeds = held == "core:bone_meal" && (registry.grows(id) || id == blocks::GRASS);
    let name = registry.get(id).name.as_str();
    let potting = name == "core:flower_pot"
        && ourcraft_common::decoration::pottable()
            .iter()
            .any(|p| p == held);
    let frame = registry
        .get(id)
        .variant
        .as_ref()
        .is_some_and(|v| v.family == "core:item_frame");
    let honey = held == "core:bowl"
        && registry
            .stage(id)
            .is_some_and(|s| s.kind == StageKind::Hive && s.stage == s.max);
    stepped || feeds || potting || frame || honey
}

/// Blocks that do something when you use them (instead of having a block
/// placed against them).
pub fn usable(registry: &ourcraft_common::block::BlockRegistry, id: BlockId) -> bool {
    use ourcraft_common::farming::StageKind;
    use ourcraft_common::shapes::VariantKind as K;
    let stepped = registry
        .stage(id)
        .is_some_and(|s| matches!(s.kind, StageKind::Cake | StageKind::Composter));
    stepped
        || matches!(
            registry.get(id).name.as_str(),
            "core:campfire" | "core:barrel" | "core:flower_pot" | "core:note_block"
        )
        || registry
            .get(id)
            .variant
            .as_ref()
            .is_some_and(|v| v.family == "core:item_frame")
        || id == blocks::CRAFTING_TABLE
        || id == blocks::FURNACE
        || registry.get(id).name == "core:chest"
        || matches!(
            registry.variant_kind(id),
            Some(
                K::Door
                    | K::Trapdoor
                    | K::FenceGate
                    | K::Bed
                    | K::Sign
                    | K::WallSign
                    | K::Lever
                    | K::Button
            )
        )
}

#[allow(clippy::too_many_arguments)]
fn break_and_place(
    time: Res<Time>,
    actions: Res<Actions>,
    target: Res<Target>,
    source: Res<SourceTarget>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    items: Res<Items>,
    mode: Res<GameMode>,
    (rules, tools, focus): (
        Res<crate::gamemode::Rules>,
        Res<crate::gamemode::ToolHotbar>,
        Res<UiFocus>,
    ),
    player: Single<&Player>,
    mut inventory: ResMut<PlayerInventory>,
    mut progress: ResMut<BreakProgress>,
    mut edits: MessageWriter<BlockEditRequest>,
    mut sounds: MessageWriter<crate::audio::PlaySound>,
    mut cooldown: Local<f32>,
) {
    // In some game modes you use tools instead, or can't build at all.
    if tools.active() || !rules.0.can_build || focus.gameplay_blocked() {
        *progress = BreakProgress::default();
        return;
    }
    *cooldown -= time.delta_secs();
    let survival = *mode == GameMode::Survival;
    let target_pos = target.0.map(|h| h.block);
    if !actions.break_block || progress.pos != target_pos {
        *progress = BreakProgress::default();
    }
    let wants = actions.break_block || actions.place_block || actions.pick_block;
    if !wants {
        *cooldown = 0.0; // releasing the button allows an instant next click
        return;
    }
    let held = inventory.held();
    if actions.place_block && !actions.break_block && *cooldown <= 0.0 {
        let bucket = use_bucket(
            held.map(|h| items.name(h)).unwrap_or(""),
            target.0,
            source.0,
            &world,
            &blocks.0,
            &items,
            &mut inventory,
            survival,
        );
        let bucket = bucket.or_else(|| {
            // A hoe turns dirt and grass into farmland.
            let name = held.map(|h| items.name(h)).unwrap_or("");
            let hit = target.0?;
            let soil = hit.id == blocks::DIRT || hit.id == blocks::GRASS;
            let above = world.block(hit.block.x, hit.block.y + 1, hit.block.z);
            if !name.ends_with("_hoe") || !soil || !above.is_air() {
                return None;
            }
            if survival {
                inventory.wear_selected(&items);
            }
            Some((hit.block, blocks.0.by_name("core:farmland")?))
        });
        if let Some(edit) = bucket {
            edits.write(BlockEditRequest {
                pos: edit.0,
                block: edit.1,
                held: held.unwrap_or(ItemId::NONE),
            });
            let sound = if blocks.0.is_farmland(edit.1) {
                "step_grass"
            } else {
                "splat"
            };
            sounds.write(crate::audio::PlaySound {
                name: sound.into(),
                at: Some(edit.0.as_vec3() + Vec3::splat(0.5)),
            });
            *cooldown = REPEAT_DELAY;
            return;
        }
    }
    let Some(hit) = target.0 else { return };

    if actions.pick_block && !survival {
        let selected = inventory.selected;
        inventory.slots[selected] = Some(ItemStack::new(ItemId::from_block(hit.id), 64));
        return;
    }

    if actions.break_block {
        if !blocks.0.is_breakable(hit.id) {
            return;
        }
        if survival {
            // Keep hitting until it gives way.
            progress.pos = Some(hit.block);
            progress.needed = items.break_time(&blocks.0, hit.id, held);
            progress.time += time.delta_secs();
            if progress.time < progress.needed {
                return;
            }
            *progress = BreakProgress::default();
            if held.is_some_and(|h| items.get(h).is_some_and(|d| d.tool().is_some()))
                && inventory.wear_selected(&items)
            {
                sounds.write(crate::audio::PlaySound {
                    name: "break".into(),
                    at: None,
                });
            }
        } else if *cooldown > 0.0 {
            return;
        }
        edits.write(BlockEditRequest {
            pos: hit.block,
            block: BlockId::AIR,
            held: held.unwrap_or(ItemId::NONE),
        });
        *cooldown = REPEAT_DELAY;
    } else if actions.place_block && *cooldown <= 0.0 {
        // Using a door, chest, furnace… instead (see `use_blocks`).
        if usable(&blocks.0, hit.id) && !actions.descend {
            return;
        }
        let pos = hit.block + hit.normal;
        let Some(block) = inventory
            .selected_stack()
            .and_then(|s| items.get(s.item))
            .and_then(|d| d.block())
        else {
            return; // not holding a block
        };
        // Some blocks change with where they go and which way you look
        // (torches on walls, stairs, doors two blocks tall).
        let look = player.look_direction().to_array();
        let Some(placed) = blocks.0.placement(block, hit.normal.to_array(), look) else {
            return;
        };
        let mut parts = vec![(pos, placed.block)];
        if let Some((offset, extra)) = placed.extra {
            parts.push((pos + IVec3::from_array(offset), extra));
        }
        // Only into empty space (air, water or tall grass), never inside
        // the player, and plants only where they can grow.
        let me = Aabb::player(player.state.position);
        let fits = parts.iter().all(|(p, b)| {
            let existing = world.block(p.x, p.y, p.z);
            let replaceable =
                existing.is_air() || blocks.0.is_liquid(existing) || blocks.0.is_soft(existing);
            let grows_here = blocks
                .0
                .fits(*b, |o| world.block(p.x + o[0], p.y + o[1], p.z + o[2]));
            replaceable && grows_here && !(blocks.0.is_solid(*b) && me.intersects(&Aabb::block(*p)))
        });
        if fits {
            if survival {
                inventory.take_selected();
            }
            for (pos, block) in parts {
                edits.write(BlockEditRequest {
                    pos,
                    block,
                    held: held.unwrap_or(ItemId::NONE),
                });
            }
            *cooldown = REPEAT_DELAY;
        }
    }
}

/// Scooping up water or lava with a bucket, or pouring it out: the block
/// to change, if the held item is a bucket and there's something to do.
#[allow(clippy::too_many_arguments)]
fn use_bucket(
    held: &str,
    target: Option<RayHit>,
    source: Option<RayHit>,
    world: &ClientWorld,
    blocks: &ourcraft_common::block::BlockRegistry,
    items: &Items,
    inventory: &mut PlayerInventory,
    survival: bool,
) -> Option<(IVec3, BlockId)> {
    let filled = |family: &str| match family {
        "core:water" => Some("core:water_bucket"),
        "core:lava" => Some("core:lava_bucket"),
        _ => None,
    };
    if held == "core:bucket" {
        let hit = source?;
        let full = filled(&blocks.fluid(hit.id)?.family).and_then(|n| items.by_name(n))?;
        if survival && !inventory.exchange_selected(items, full) {
            return None;
        }
        return Some((hit.block, BlockId::AIR));
    }
    let family = match held {
        "core:water_bucket" => "core:water",
        "core:lava_bucket" => "core:lava",
        _ => return None,
    };
    let hit = target?;
    if usable(blocks, hit.id) {
        return None;
    }
    let pos = hit.block + hit.normal;
    let existing = world.block(pos.x, pos.y, pos.z);
    if !(existing.is_air() || blocks.is_replaceable(existing)) {
        return None;
    }
    let block = blocks.fluid_source(family)?;
    if survival {
        let empty = items.by_name("core:bucket")?;
        inventory.exchange_selected(items, empty);
    }
    Some((pos, block))
}

/// Holding a game-mode tool: clicking uses it (left = main, right =
/// secondary), repeating at the tool's own rate while held.
fn use_tools(
    time: Res<Time>,
    actions: Res<Actions>,
    tools: Res<crate::gamemode::ToolHotbar>,
    specs: Res<crate::gamemode::ToolSpecs>,
    inventory: Res<PlayerInventory>,
    player: Single<&Player>,
    conn: Option<ResMut<crate::net::Connection>>,
    mut cooldown: Local<f32>,
) {
    *cooldown -= time.delta_secs();
    let (Some(mut conn), true) = (conn, tools.active()) else {
        return;
    };
    if !(actions.break_block || actions.place_block) || *cooldown > 0.0 {
        return;
    }
    let Some(tool) = tools.0.get(inventory.selected) else {
        return;
    };
    *cooldown = specs.specs.get(tool).map_or(0.2, |s| s.cooldown.max(0.05));
    conn.send(
        Channel::Reliable,
        &ClientMessage::UseTool {
            tool: tool.clone(),
            direction: player.look_direction().to_array(),
            secondary: !actions.break_block,
        },
    );
}

/// Q throws one item from your hand; Ctrl+Q the whole stack.
fn drop_items(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<UiFocus>,
    tools: Res<crate::gamemode::ToolHotbar>,
    mut inventory: ResMut<PlayerInventory>,
    conn: Option<ResMut<crate::net::Connection>>,
) {
    if !keys.just_pressed(KeyCode::KeyQ) || focus.gameplay_blocked() || tools.active() {
        return;
    }
    let (Some(mut conn), Some(stack)) = (conn, inventory.selected_stack()) else {
        return;
    };
    let all = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let n = if all { stack.count } else { 1 };
    let selected = inventory.selected;
    inventory.slots[selected] = (stack.count > n).then_some(ItemStack {
        count: stack.count - n,
        ..stack
    });
    conn.send(
        Channel::Reliable,
        &ClientMessage::DropItems(ItemStack { count: n, ..stack }.into()),
    );
}

/// Outline the targeted block (or animal).
fn draw_highlight(
    target: Res<Target>,
    target_mob: Res<TargetMob>,
    tools: Res<crate::gamemode::ToolHotbar>,
    rules: Res<crate::gamemode::Rules>,
    mut gizmos: Gizmos,
) {
    if tools.active() || !rules.0.can_build {
        return;
    }
    if let Some((mob, feet)) = &target_mob.0 {
        let size = Vec3::new(mob.size[0], mob.size[1], mob.size[0]);
        gizmos.cube(
            Transform::from_translation(*feet + Vec3::Y * size.y / 2.0).with_scale(size * 1.02),
            Color::srgba(1.0, 1.0, 1.0, 0.8),
        );
    }
    if let Some(hit) = target.0 {
        let center = hit.block.as_vec3() + Vec3::splat(0.5);
        gizmos.cube(
            Transform::from_translation(center).with_scale(Vec3::splat(1.004)),
            Color::srgba(0.05, 0.05, 0.05, 0.9),
        );
    }
}

/// The crack overlay: a slightly oversized cube, one material per stage.
#[derive(Resource)]
struct Cracks {
    entity: Entity,
    stages: Vec<Handle<StandardMaterial>>,
}

/// Cracks spreading from the middle: stage `s` (0-based) of `CRACK_STAGES`.
fn crack_pixels(stage: usize) -> Vec<u8> {
    let n = 16usize;
    let mut out = vec![0u8; n * n * 4];
    // A few crooked lines from the centre outwards; later stages draw more
    // of each line, and more lines.
    let lines: [(i32, i32); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];
    let count = 2 + stage;
    let length = 3 + stage as i32;
    for (i, (dx, dy)) in lines.iter().enumerate().take(count.min(8)) {
        let (mut x, mut y) = (7 + (i as i32 % 2), 7 + (i as i32 / 4));
        for step in 0..length {
            // Wobble sideways now and then.
            let wobble = ((i as i32 * 7 + step * 3) % 5 == 0) as i32;
            x += dx + if *dy != 0 { wobble } else { 0 };
            y += dy + if *dx != 0 { wobble } else { 0 };
            if !(0..16).contains(&x) || !(0..16).contains(&y) {
                break;
            }
            let p = (y as usize * n + x as usize) * 4;
            out[p..p + 4].copy_from_slice(&[20, 20, 20, 200]);
        }
    }
    out
}

fn setup_cracks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let stages = (0..CRACK_STAGES)
        .map(|s| {
            let mut image = Image::new(
                Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                crack_pixels(s),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            image.sampler = ImageSampler::nearest();
            materials.add(StandardMaterial {
                base_color_texture: Some(images.add(image)),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })
        })
        .collect::<Vec<_>>();
    let entity = commands
        .spawn((
            Name::new("Cracks"),
            Mesh3d(meshes.add(Cuboid::new(1.006, 1.006, 1.006))),
            MeshMaterial3d(stages[0].clone()),
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    commands.insert_resource(Cracks { entity, stages });
}

fn draw_cracks(
    progress: Res<BreakProgress>,
    cracks: Res<Cracks>,
    mut query: Query<(
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Ok((mut transform, mut visibility, mut material)) = query.get_mut(cracks.entity) else {
        return;
    };
    let (Some(pos), f) = (progress.pos, progress.fraction()) else {
        *visibility = Visibility::Hidden;
        return;
    };
    if f <= 0.02 {
        *visibility = Visibility::Hidden;
        return;
    }
    let stage = ((f * CRACK_STAGES as f32) as usize).min(CRACK_STAGES - 1);
    transform.translation = pos.as_vec3() + Vec3::splat(0.5);
    *visibility = Visibility::Visible;
    if material.0 != cracks.stages[stage] {
        material.0 = cracks.stages[stage].clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cracks_grow() {
        let ink = |s| crack_pixels(s).chunks(4).filter(|p| p[3] > 0).count();
        assert!(ink(0) > 0);
        assert!(ink(CRACK_STAGES - 1) > ink(0));
    }
}

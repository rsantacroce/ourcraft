//! Looking at, breaking and placing blocks.
//!
//! Interaction doesn't change the world directly. It sends a
//! [`BlockEditRequest`] message, and whoever owns the world applies it: in
//! single player that's a local system; from M6 it's the server, which can
//! refuse (say, if you're too far away). Keeping that split from the start
//! makes multiplayer a much smaller change.

use bevy::prelude::*;
use ourcraft_common::block::{BlockId, blocks};
use ourcraft_common::inventory::{Inventory, ItemStack};
use ourcraft_common::physics::Aabb;
use ourcraft_common::raycast::{RayHit, raycast};

use crate::input::Actions;
use crate::player::{GameMode, Player, PlayerCamera};
use crate::world::{Blocks, ClientWorld};

/// How far away you can reach blocks.
pub const REACH: f32 = 6.0;
/// Seconds between repeated breaks/places while the button is held.
const REPEAT_DELAY: f32 = 0.22;

pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BlockEditRequest>()
            .init_resource::<Target>()
            .insert_resource(PlayerInventory(creative_hotbar()))
            .add_systems(
                Update,
                (
                    update_target,
                    hotbar_selection,
                    break_and_place,
                    use_tools,
                    draw_highlight,
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
}

/// The block under the crosshair, if any.
#[derive(Resource, Default, Debug)]
pub struct Target(pub Option<RayHit>);

/// The local player's inventory.
#[derive(Resource, Debug, Deref, DerefMut)]
pub struct PlayerInventory(pub Inventory);

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
        *slot = Some(ItemStack { block, count: 64 });
    }
    inv
}

fn update_target(
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    camera: Single<&GlobalTransform, With<PlayerCamera>>,
    player: Single<&Player>,
    mut target: ResMut<Target>,
) {
    // Use the up-to-date look direction (the camera transform may lag a
    // frame behind the mouse).
    let origin = camera.translation();
    target.0 = raycast(&world.0, &blocks.0, origin, player.look_direction(), REACH);
}

fn hotbar_selection(actions: Res<Actions>, mut inventory: ResMut<PlayerInventory>) {
    if let Some(slot) = actions.hotbar_select {
        inventory.select(slot);
    }
    if actions.hotbar_scroll != 0 {
        inventory.scroll(actions.hotbar_scroll);
    }
}

fn break_and_place(
    time: Res<Time>,
    actions: Res<Actions>,
    target: Res<Target>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    mode: Res<GameMode>,
    rules: Res<crate::gamemode::Rules>,
    tools: Res<crate::gamemode::ToolHotbar>,
    player: Single<&Player>,
    mut inventory: ResMut<PlayerInventory>,
    mut edits: MessageWriter<BlockEditRequest>,
    mut cooldown: Local<f32>,
) {
    // In some game modes you use tools instead, or can't build at all.
    if tools.active() || !rules.0.can_build {
        return;
    }
    *cooldown -= time.delta_secs();
    let wants = actions.break_block || actions.place_block || actions.pick_block;
    if !wants {
        *cooldown = 0.0; // releasing the button allows an instant next click
        return;
    }
    let Some(hit) = target.0 else { return };

    if actions.pick_block && *mode == GameMode::Creative {
        let selected = inventory.selected;
        inventory.slots[selected] = Some(ItemStack {
            block: hit.id,
            count: 64,
        });
        return;
    }
    if *cooldown > 0.0 {
        return;
    }

    if actions.break_block {
        if blocks.0.is_breakable(hit.id) {
            edits.write(BlockEditRequest {
                pos: hit.block,
                block: BlockId::AIR,
            });
            if *mode == GameMode::Survival
                && let Some(drop) = blocks.0.drop_for(hit.id)
            {
                inventory.add(drop, 1);
            }
            *cooldown = REPEAT_DELAY;
        }
    } else if actions.place_block {
        let pos = hit.block + hit.normal;
        let Some(stack) = inventory.selected_stack() else {
            return;
        };
        // Only place into empty space (air or water)…
        let existing = world.block(pos.x, pos.y, pos.z);
        let replaceable = existing.is_air() || blocks.0.is_liquid(existing);
        // …and never inside the player.
        let inside_player = blocks.0.is_solid(stack.block)
            && Aabb::player(player.state.position).intersects(&Aabb::block(pos));
        if replaceable && !inside_player {
            if *mode == GameMode::Survival {
                inventory.take_selected();
            }
            edits.write(BlockEditRequest {
                pos,
                block: stack.block,
            });
            *cooldown = REPEAT_DELAY;
        }
    }
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
        ourcraft_common::protocol::Channel::Reliable,
        &ourcraft_common::protocol::ClientMessage::UseTool {
            tool: tool.clone(),
            direction: player.look_direction().to_array(),
            secondary: !actions.break_block,
        },
    );
}

/// Outline the targeted block.
fn draw_highlight(
    target: Res<Target>,
    tools: Res<crate::gamemode::ToolHotbar>,
    rules: Res<crate::gamemode::Rules>,
    mut gizmos: Gizmos,
) {
    if tools.active() || !rules.0.can_build {
        return;
    }
    if let Some(hit) = target.0 {
        let center = hit.block.as_vec3() + Vec3::splat(0.5);
        gizmos.cube(
            Transform::from_translation(center).with_scale(Vec3::splat(1.004)),
            Color::srgba(0.05, 0.05, 0.05, 0.9),
        );
    }
}

//! Survival on the server: dropped items and furnaces.
//!
//! - When a survival player breaks a block, what it drops (depending on the
//!   tool they held) appears as a small spinning item. It falls to the
//!   ground; walking over it puts it in your inventory.
//! - Furnaces keep their own items. Players open one to see its screen;
//!   every slot click is checked here, and the fire burns on even when
//!   nobody's looking.

use bevy::prelude::*;
use ourcraft_common::block::{BlockId, blocks};
use ourcraft_common::container::{Container, ContainerKind, FURNACE_OUTPUT};
use ourcraft_common::inventory::{ItemStack, click_slot};
use ourcraft_common::item::ItemId;
use ourcraft_common::protocol::{Channel, ClientMessage, NetContainer, NetStack, ServerMessage};

use crate::entities::{Entities, ServerEntity};
use crate::mods::AppliedEdits;
use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{OpenWorld, Registry, ServerWorld};

/// Dropped items disappear after this many seconds.
const ITEM_LIFETIME: f32 = 300.0;
/// Seconds before a fresh drop can be picked up (so you see it pop out).
const PICKUP_DELAY: f32 = 0.5;
/// How close (in blocks) to your body an item must be to pick it up.
const PICKUP_REACH: f32 = 1.0;
const ITEM_GRAVITY: f32 = 18.0;
/// Height of a resting item's centre above the block it lies on.
const ITEM_REST: f32 = 0.2;
/// Max distance from which a furnace can be used.
const CONTAINER_REACH: f32 = 8.0;
/// Furnaces update this often (seconds).
const CONTAINER_TICK: f32 = 0.1;

pub struct SurvivalPlugin;

impl Plugin for SurvivalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AppliedEdits>().add_systems(
            Update,
            (
                handle_container_messages,
                drop_broken_blocks,
                simulate_items,
                pick_up_items,
                tick_containers,
            )
                .chain()
                .after(crate::players::PlayersSet),
        );
    }
}

/// The model name clients recognise as "draw this item".
pub fn item_model(item: ItemId) -> String {
    format!("@item:{}", item.0)
}

/// Throw an item into the world at `at` (a block's centre, say).
pub fn spawn_item(entities: &mut Entities, at: Vec3, stack: ItemStack) -> u64 {
    // A little hop in a random-ish direction, so drops don't stack up.
    let h = (at.x * 12.9898 + at.z * 78.233 + at.y * 37.719).sin() * 43_758.547;
    let angle = h.fract() * std::f32::consts::TAU;
    entities.spawn(ServerEntity {
        model: item_model(stack.item),
        position: at,
        velocity: Vec3::new(angle.cos() * 1.5, 4.0, angle.sin() * 1.5),
        motion: ourcraft_mod_api::EntityMotion::Fixed,
        lifetime: ITEM_LIFETIME,
        age: 0.0,
        owner: None,
        moved: true,
        item: Some(stack),
        yaw: angle,
    })
}

fn container_message(pos: IVec3, c: &Container) -> ServerMessage {
    ServerMessage::Container(NetContainer {
        pos: pos.to_array(),
        kind: c.kind,
        slots: c.slots.iter().map(|s| s.map(NetStack::from)).collect(),
        burn: c.burn_fraction(),
        smelt: c.smelt_fraction(),
    })
}

/// Tell everyone looking at the container at `pos` what's in it now.
fn show_container(
    net: &mut Network,
    players: &Query<&mut ConnectedPlayer>,
    pos: IVec3,
    c: &Container,
) {
    let msg = container_message(pos, c);
    for p in players.iter().filter(|p| p.open_container == Some(pos)) {
        net.send(p.client, Channel::Reliable, &msg);
    }
}

fn container_kind(
    registry: &ourcraft_common::block::BlockRegistry,
    block: BlockId,
) -> Option<ContainerKind> {
    if block == blocks::FURNACE {
        Some(ContainerKind::Furnace)
    } else if registry.get(block).name == "core:chest" {
        Some(ContainerKind::Chest)
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_container_messages(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    mut players: Query<&mut ConnectedPlayer>,
    mut open: ResMut<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut entities: ResMut<Entities>,
    mut net: ResMut<Network>,
) {
    let open = &mut *open;
    for FromClient { client, msg } in messages.read() {
        let Some(&entity) = players_map.0.get(client) else {
            continue;
        };
        match msg {
            ClientMessage::OpenContainer { pos } => {
                let pos = IVec3::from_array(*pos);
                let Ok(mut p) = players.get_mut(entity) else {
                    continue;
                };
                let near =
                    p.position().distance(pos.as_vec3() + Vec3::splat(0.5)) <= CONTAINER_REACH;
                let kind = world
                    .get_block(pos.x, pos.y, pos.z)
                    .and_then(|b| container_kind(&registry.0, b));
                let (Some(kind), true, true) = (kind, near, p.name.is_some()) else {
                    continue;
                };
                let c = open
                    .containers
                    .entry(pos)
                    .or_insert_with(|| Container::new(kind));
                p.open_container = Some(pos);
                net.send(*client, Channel::Reliable, &container_message(pos, c));
            }
            ClientMessage::CloseContainer => {
                if let Ok(mut p) = players.get_mut(entity) {
                    p.open_container = None;
                }
            }
            ClientMessage::ContainerClick {
                slot,
                cursor,
                secondary,
            } => {
                let Ok(p) = players.get(entity) else { continue };
                let reply = |net: &mut Network, cursor| {
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::ContainerCursor(cursor),
                    )
                };
                let cursor_stack = cursor
                    .map(ItemStack::from)
                    .filter(|s| s.count > 0 && (s.item.0 as usize) < open.items.len());
                let Some(pos) = p.open_container else {
                    reply(&mut net, *cursor);
                    continue;
                };
                let near =
                    p.position().distance(pos.as_vec3() + Vec3::splat(0.5)) <= CONTAINER_REACH;
                let items = open.items.clone();
                let Some(c) = open.containers.get_mut(&pos).filter(|_| near) else {
                    reply(&mut net, *cursor);
                    continue;
                };
                let slot = *slot as usize;
                if slot >= c.slots.len() {
                    reply(&mut net, *cursor);
                    continue;
                }
                let current = c.slots[slot];
                let (new_slot, new_cursor) = if c.kind == ContainerKind::Furnace
                    && slot == FURNACE_OUTPUT
                {
                    // Take-only: pick it up, or add it to what you hold.
                    match (current, cursor_stack) {
                        (Some(s), None) => click_slot(&items, Some(s), None, *secondary),
                        (Some(s), Some(h)) if h.stacks_with(&s) => {
                            let n = s.count.min(items.max_stack(s.item).saturating_sub(h.count));
                            let left = s.count - n;
                            (
                                (left > 0).then_some(ItemStack { count: left, ..s }),
                                Some(ItemStack {
                                    count: h.count + n,
                                    ..h
                                }),
                            )
                        }
                        _ => (current, cursor_stack),
                    }
                } else if cursor_stack.is_some_and(|h| !c.kind.accepts(&items, slot, &h)) {
                    (current, cursor_stack)
                } else {
                    click_slot(&items, current, cursor_stack, *secondary)
                };
                c.slots[slot] = new_slot;
                reply(&mut net, new_cursor.map(NetStack::from));
                let c = c.clone();
                show_container(&mut net, &players, pos, &c);
            }
            ClientMessage::DropItems(stack) => {
                let Ok(p) = players.get(entity) else { continue };
                let stack = ItemStack::from(*stack);
                if p.name.is_none() || stack.count == 0 || stack.item.0 as usize >= open.items.len()
                {
                    continue;
                }
                let look = crate::mods::look_vector(p.state.yaw, p.state.pitch);
                let look = Vec3::from_array(look);
                let eye = p.position() + Vec3::Y * ourcraft_common::physics::EYE_HEIGHT;
                let id = spawn_item(&mut entities, eye + look * 0.4, stack);
                if let Some(e) = entities.list.get_mut(&id) {
                    e.velocity = look * 5.0 + Vec3::Y * 1.5;
                    // Give the thrower a moment before it can come back.
                    e.age = -1.0;
                }
            }
            _ => {}
        }
    }
}

/// Survival players get drops for the blocks they break. Broken furnaces
/// spill what they held, and torches hanging on a broken block fall off.
#[allow(clippy::too_many_arguments)]
fn drop_broken_blocks(
    mut applied: ResMut<AppliedEdits>,
    mut players: Query<&mut ConnectedPlayer>,
    mut open: ResMut<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<crate::world::ColumnCache>,
    mut entities: ResMut<Entities>,
    mut net: ResMut<Network>,
    mut updates: ResMut<crate::updates::BlockUpdates>,
    mut luck: Local<u32>,
) {
    for edit in std::mem::take(&mut applied.0) {
        // Water may flow into the gap, sand may fall…
        updates.around(edit.pos, 2);
        if edit.old.is_air() || edit.old == edit.new {
            continue;
        }
        let center = edit.pos.as_vec3() + Vec3::splat(0.5);
        let survival_breaker = players
            .iter()
            .any(|p| p.client == edit.client && !p.creative);
        // A sign's words go with it.
        if open.signs.remove(&edit.pos).is_some() {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::SignText {
                    pos: edit.pos.to_array(),
                    lines: Vec::new(),
                },
            );
        }
        // The other half of a door or bed goes too. Whichever half you
        // break, you get one door (or bed) back.
        if let Some(o) = registry.0.other_half(edit.old) {
            let at = edit.pos + IVec3::from_array(o);
            let kind = registry.0.variant_kind(edit.old);
            let partner = world
                .get_block(at.x, at.y, at.z)
                .filter(|b| registry.0.variant_kind(*b) == kind);
            if let Some(partner) = partner {
                crate::world::set_block(&mut world, &mut cache, &open, at, BlockId::AIR);
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::BlockChanged {
                        pos: at.to_array(),
                        block: 0,
                    },
                );
                let broke_the_dropless_half =
                    registry.0.get(edit.old).drops == ourcraft_common::block::Drops::Nothing;
                if survival_breaker && broke_the_dropless_half {
                    for (item, n) in open.items.drops(&registry.0, partner, None, *luck) {
                        spawn_item(&mut entities, center, ItemStack::new(item, n));
                    }
                }
            }
        }
        // Torches (and anything else hanging) on the block that went away.
        if edit.new.is_air() {
            for d in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::Z, IVec3::NEG_Z] {
                let at = edit.pos + d;
                let Some(b) = world.get_block(at.x, at.y, at.z) else {
                    continue;
                };
                let hangs_here = registry
                    .0
                    .attached_to(b)
                    .is_some_and(|o| at + IVec3::from_array(o) == edit.pos);
                if !hangs_here {
                    continue;
                }
                crate::world::set_block(&mut world, &mut cache, &open, at, BlockId::AIR);
                let msg = ServerMessage::BlockChanged {
                    pos: at.to_array(),
                    block: 0,
                };
                net.broadcast(Channel::Reliable, &msg);
                if survival_breaker {
                    *luck = luck.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    for (item, n) in open.items.drops(&registry.0, b, None, *luck >> 4) {
                        spawn_item(
                            &mut entities,
                            at.as_vec3() + Vec3::splat(0.5),
                            ItemStack::new(item, n),
                        );
                    }
                }
            }
        }
        if let Some(c) = open.containers.remove(&edit.pos) {
            for stack in c.slots.into_iter().flatten() {
                spawn_item(&mut entities, center, stack);
            }
            for mut p in players
                .iter_mut()
                .filter(|p| p.open_container == Some(edit.pos))
            {
                p.open_container = None;
                net.send(p.client, Channel::Reliable, &ServerMessage::ContainerClosed);
            }
        }
        if !survival_breaker || !edit.new.is_air() {
            continue;
        }
        let held = (edit.held != ItemId::NONE).then_some(edit.held);
        // Sometimes a little extra (apples from leaves).
        if let Some((name, one_in)) = &registry.0.get(edit.old).bonus_drop {
            let mix = (edit.pos.x as u32) ^ (edit.pos.z as u32).rotate_left(16);
            *luck = luck
                .wrapping_mul(1_103_515_245)
                .wrapping_add(12_345)
                .wrapping_add(mix);
            if (*luck >> 8).is_multiple_of((*one_in).max(1) as u32)
                && let Some(item) = open.items.by_name(name)
            {
                spawn_item(&mut entities, center, ItemStack::new(item, 1));
            }
        }
        *luck = luck.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        for (item, count) in open.items.drops(&registry.0, edit.old, held, *luck >> 4) {
            let max = open.items.max_stack(item);
            let mut left = count;
            while left > 0 {
                let n = left.min(max);
                spawn_item(&mut entities, center, ItemStack::new(item, n));
                left -= n;
            }
        }
    }
}

/// Dropped items fall until they land on something solid.
fn simulate_items(
    time: Res<Time>,
    mut entities: ResMut<Entities>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
) {
    let dt = time.delta_secs().min(0.1);
    let solid = |p: Vec3| {
        let b = p.floor().as_ivec3();
        world
            .get_block(b.x, b.y, b.z)
            .is_some_and(|id| registry.0.is_solid(id))
    };
    for e in entities.list.values_mut().filter(|e| e.item.is_some()) {
        if e.velocity == Vec3::ZERO {
            // Resting; start falling again if the ground went away.
            if !solid(e.position - Vec3::Y * (ITEM_REST + 0.05)) {
                e.velocity.y = -0.1;
            }
            continue;
        }
        e.velocity.y -= ITEM_GRAVITY * dt;
        let mut next = e.position + e.velocity * dt;
        // Sideways: stop at walls.
        if solid(Vec3::new(next.x, e.position.y, e.position.z)) {
            next.x = e.position.x;
            e.velocity.x = 0.0;
        }
        if solid(Vec3::new(next.x, e.position.y, next.z)) {
            next.z = e.position.z;
            e.velocity.z = 0.0;
        }
        // Down: land on the top of a block.
        if e.velocity.y < 0.0 && solid(next - Vec3::Y * ITEM_REST) {
            next.y = (next.y - ITEM_REST).floor() + 1.0 + ITEM_REST;
            e.velocity = Vec3::ZERO;
        } else if e.velocity.y > 0.0 && solid(next + Vec3::Y * 0.2) {
            e.velocity.y = 0.0;
            next.y = e.position.y;
        }
        e.position = next;
        e.moved = true;
    }
}

/// Walking over an item picks it up, if there's room in your inventory.
fn pick_up_items(
    mut entities: ResMut<Entities>,
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut net: ResMut<Network>,
) {
    let mut gone = Vec::new();
    for (&id, e) in entities.list.iter_mut() {
        let Some(stack) = e.item.as_mut() else {
            continue;
        };
        if e.age < PICKUP_DELAY {
            continue;
        }
        for mut p in players.iter_mut() {
            if p.name.is_none() || p.creative {
                continue;
            }
            let reach = ourcraft_common::physics::Aabb::player(p.position()).inflate(PICKUP_REACH);
            if !(e.position.cmpge(reach.min).all() && e.position.cmple(reach.max).all()) {
                continue;
            }
            let room = p.inventory.room_for(&open.items, stack.item);
            let n = (stack.count as u32).min(room) as u16;
            if n == 0 {
                continue;
            }
            let taken = ItemStack { count: n, ..*stack };
            // Keep our copy in step until the client reports back.
            p.inventory.add_stack(&open.items, taken);
            net.send(
                p.client,
                Channel::Reliable,
                &ServerMessage::GiveItems(taken.into()),
            );
            net.send(
                p.client,
                Channel::Reliable,
                &ServerMessage::Sound {
                    name: "pop".into(),
                    at: None,
                },
            );
            stack.count -= n;
            if stack.count == 0 {
                gone.push(id);
                break;
            }
        }
    }
    for id in gone {
        entities.remove(id);
    }
}

/// Burn the furnaces, and show viewers what changed.
fn tick_containers(
    time: Res<Time>,
    mut open: ResMut<OpenWorld>,
    players: Query<&mut ConnectedPlayer>,
    mut net: ResMut<Network>,
    mut timer: Local<f32>,
) {
    *timer += time.delta_secs();
    if *timer < CONTAINER_TICK {
        return;
    }
    let dt = std::mem::take(&mut *timer);
    let open = &mut *open;
    let items = open.items.clone();
    for (pos, c) in open.containers.iter_mut() {
        if c.tick(&items, dt) {
            show_container(&mut net, &players, *pos, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_models_are_recognisable() {
        assert_eq!(item_model(ItemId(42)), "@item:42");
    }
}

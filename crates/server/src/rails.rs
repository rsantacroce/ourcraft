//! Rails and mine carts on the server (milestone 22).
//!
//! Rails join up with the rails next to them when one is laid. A mine
//! cart put on a rail rolls along it: whoever sits in it pushes it by
//! walking forward, powered rails with power speed it up (and brake it
//! without), and it stops at the end of the line.

use std::collections::HashMap;

use bevy::prelude::*;
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::item::ItemId;
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};
use ourcraft_common::shapes::{Dir4, VariantKind};
use ourcraft_mod_api::EntityMotion;

use crate::entities::{Entities, ServerEntity};
use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{ColumnCache, OpenWorld, Registry, ServerWorld};

/// Fastest a cart goes (blocks per second).
const MAX_SPEED: f32 = 10.0;
const PUSH: f32 = 5.0;
const FRICTION: f32 = 0.8;
const BOOST: f32 = 14.0;
const BRAKE: f32 = 12.0;
/// Speed a cart loses climbing a step of rail (and gains going down).
const STEP_SPEED: f32 = 1.5;
/// How hard a slope pulls a cart down it (blocks per second, per second).
const SLOPE_PULL: f32 = 2.5;

pub struct RailsPlugin;

impl Plugin for RailsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Carts>()
            .init_resource::<RailDirty>()
            .add_systems(
                Update,
                (remember_carts, use_carts, join_rails, roll_carts)
                    .chain()
                    .after(crate::players::PlayersSet),
            );
    }
}

/// A cart: which way it's going and how fast.
#[derive(Debug, Clone, Copy)]
struct Cart {
    dir: Vec3,
    speed: f32,
}

#[derive(Resource, Default)]
struct Carts(HashMap<u64, Cart>);

/// Blocks that changed this tick (rails may need to join up around them).
#[derive(Resource, Default)]
pub struct RailDirty(pub Vec<(IVec3, bool)>);

fn dir_vec(d: Dir4) -> Vec3 {
    IVec3::from_array(d.vector()).as_vec3()
}

/// The way along a vector, if it's one of the four.
fn dir_of(v: Vec3) -> Option<Dir4> {
    Dir4::ALL.into_iter().find(|d| dir_vec(*d).dot(v) > 0.9)
}

/// Bring back the carts saved with the world (once), then keep the list
/// to save up to date.
fn remember_carts(
    mut open: ResMut<OpenWorld>,
    mut entities: ResMut<Entities>,
    mut carts: ResMut<Carts>,
    mut loaded: Local<bool>,
    mut timer: Local<u32>,
) {
    if !*loaded {
        *loaded = true;
        for (_, at, yaw) in &open.carts {
            let id = entities.spawn(ServerEntity {
                model: "@cart".into(),
                position: Vec3::from_array(*at),
                velocity: Vec3::ZERO,
                motion: EntityMotion::Fixed,
                lifetime: 0.0,
                age: 0.0,
                owner: None,
                moved: true,
                item: None,
                yaw: *yaw,
            });
            carts.0.insert(
                id,
                Cart {
                    dir: Vec3::Z,
                    speed: 0.0,
                },
            );
        }
        return;
    }
    *timer += 1;
    if (*timer).is_multiple_of(10) {
        let list = carts
            .0
            .keys()
            .filter_map(|id| {
                let e = entities.list.get(id)?;
                Some(("core:minecart".to_owned(), e.position.to_array(), e.yaw))
            })
            .collect();
        open.bypass_change_detection().carts = list;
    }
}

#[allow(clippy::too_many_arguments)]
fn use_carts(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut entities: ResMut<Entities>,
    mut carts: ResMut<Carts>,
    mut net: ResMut<Network>,
) {
    for FromClient { client, msg } in messages.read() {
        match msg {
            // ---- A cart onto a rail. ----
            ClientMessage::UseBlock { pos, held } => {
                if open.items.name(ItemId(*held)) != "core:minecart" {
                    continue;
                }
                let Some(p) = players_map.0.get(client).and_then(|e| players.get(*e).ok()) else {
                    continue;
                };
                let pos = IVec3::from_array(*pos);
                let on_rail = world
                    .get_block(pos.x, pos.y, pos.z)
                    .is_some_and(|b| registry.0.is_rail(b));
                if !on_rail || p.position().distance(pos.as_vec3()) > crate::players::MAX_REACH {
                    continue;
                }
                let id = entities.spawn(ServerEntity {
                    model: "@cart".into(),
                    position: pos.as_vec3() + Vec3::new(0.5, 0.05, 0.5),
                    velocity: Vec3::ZERO,
                    motion: EntityMotion::Fixed,
                    lifetime: 0.0,
                    age: 0.0,
                    owner: None,
                    moved: true,
                    item: None,
                    yaw: 0.0,
                });
                carts.0.insert(
                    id,
                    Cart {
                        dir: Vec3::Z,
                        speed: 0.0,
                    },
                );
                if !p.creative {
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::TakeItems(NetStack {
                            item: *held,
                            count: 1,
                            wear: 0,
                        }),
                    );
                }
            }
            // ---- Getting in. ----
            ClientMessage::UseEntity { id, .. } if carts.0.contains_key(id) => {
                let taken = players.iter().any(|o| o.cart == Some(*id));
                let Some(at) = entities.list.get(id).map(|e| e.position) else {
                    continue;
                };
                let Some(mut p) = players_map
                    .0
                    .get(client)
                    .and_then(|e| players.get_mut(*e).ok())
                else {
                    continue;
                };
                if taken
                    || p.cart.is_some()
                    || p.position().distance(at) > crate::players::MAX_REACH
                {
                    continue;
                }
                p.cart = Some(*id);
                net.send(
                    *client,
                    Channel::Reliable,
                    &ServerMessage::InCart { cart: Some(*id) },
                );
            }
            // ---- Picking an empty cart up. ----
            ClientMessage::HitEntity { id, .. } if carts.0.contains_key(id) => {
                if players.iter().any(|o| o.cart == Some(*id)) {
                    continue;
                }
                let Some(e) = entities.list.get(id).cloned() else {
                    continue;
                };
                carts.0.remove(id);
                entities.remove(*id);
                let creative = players_map
                    .0
                    .get(client)
                    .and_then(|e| players.get(*e).ok())
                    .is_some_and(|p| p.creative);
                if !creative && let Some(item) = open.items.by_name("core:minecart") {
                    crate::survival::spawn_item(
                        &mut entities,
                        e.position + Vec3::Y * 0.4,
                        ItemStack::new(item, 1),
                    );
                }
            }
            _ => {}
        }
    }
}

/// Newly laid rails (and the rails next to them) join up.
#[allow(clippy::too_many_arguments)]
fn join_rails(
    mut dirty: ResMut<RailDirty>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
) {
    let reg = &registry.0;
    let get = |w: &ServerWorld, p: IVec3| w.get_block(p.x, p.y, p.z).unwrap_or_default();
    let mut todo: Vec<IVec3> = Vec::new();
    for (pos, rail) in std::mem::take(&mut dirty.0) {
        if rail {
            todo.push(pos);
            for d in Dir4::ALL {
                for dy in -1..=1 {
                    todo.push(pos + IVec3::from_array(d.vector()) + IVec3::Y * dy);
                }
            }
        }
    }
    for p in todo {
        let b = get(&world, p);
        let Some(v) = reg.get(b).variant.clone().filter(|_| reg.is_rail(b)) else {
            continue;
        };
        // Which neighbours are rails (level, or a step up or down)?
        let near: Vec<Dir4> = Dir4::ALL
            .into_iter()
            .filter(|d| {
                (-1..=1).any(|dy| {
                    let q = p + IVec3::from_array(d.vector()) + IVec3::Y * dy;
                    reg.is_rail(get(&world, q))
                })
            })
            .collect();
        let pair = match near.as_slice() {
            [] => continue,
            [one] => (*one, one.opposite()),
            many => {
                // Straight through if we can; otherwise round a corner.
                let straight = many.iter().find(|d| many.contains(&d.opposite()));
                match straight {
                    Some(d) => (*d, d.opposite()),
                    None => (many[0], many[1]),
                }
            }
        };
        // Going straight with the rail ahead a block up (and none level
        // there): a slope up to it.
        let on = reg.variant_kind(b) == Some(VariantKind::PoweredRail) && reg.is_on(b);
        let rail_off = |d: Dir4, dy: i32| {
            reg.is_rail(get(
                &world,
                p + IVec3::from_array(d.vector()) + IVec3::Y * dy,
            ))
        };
        let up = (pair.0 == pair.1.opposite())
            .then_some([pair.0, pair.1])
            .and_then(|ends| {
                ends.into_iter()
                    .find(|d| rail_off(*d, 1) && !rail_off(*d, 0))
            });
        let joined = match up {
            Some(up) => reg.rail_sloping(&v.family, up, on),
            None => reg
                .rail_joining(&v.family, pair.0, pair.1)
                .or_else(|| reg.rail_joining(&v.family, pair.0, pair.0.opposite()))
                .and_then(|n| {
                    if reg.variant_kind(n) == Some(VariantKind::PoweredRail) {
                        reg.powered(n, on)
                    } else {
                        Some(n)
                    }
                }),
        };
        if let Some(new) = joined.filter(|n| *n != b) {
            crate::building::change(&mut world, &mut cache, &open, &mut net, p, new);
        }
    }
}

/// Carts roll along their rails, carrying whoever sits in them.
fn roll_carts(
    time: Res<Time>,
    mut players: Query<&mut ConnectedPlayer>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut entities: ResMut<Entities>,
    mut carts: ResMut<Carts>,
    mut net: ResMut<Network>,
) {
    let dt = time.delta_secs().min(0.1);
    let reg = &registry.0;
    let rail_at = |p: IVec3| world.get_block(p.x, p.y, p.z).filter(|b| reg.is_rail(*b));
    // Riders and which way they'd like to go.
    let riders: HashMap<u64, (Vec2, f32)> = players
        .iter()
        .filter_map(|p| Some((p.cart?, (p.last_move, p.state.yaw))))
        .collect();
    carts.0.retain(|id, _| entities.list.contains_key(id));
    for (id, cart) in carts.0.iter_mut() {
        let Some(e) = entities.list.get_mut(id) else {
            continue;
        };
        let mut cell = e.position.floor().as_ivec3();
        // Near the top of a slope a cart is higher than its block.
        if rail_at(cell).is_none()
            && rail_at(cell - IVec3::Y).is_some_and(|r| reg.rail_slope(r).is_some())
        {
            cell -= IVec3::Y;
        }
        let Some(rail) = rail_at(cell) else {
            cart.speed = 0.0;
            continue;
        };
        let Some(ends) = reg.rail_ends(rail) else {
            continue;
        };
        // A rider walking forward pushes the cart the way they face (along
        // the rail).
        if let Some((walk, yaw)) = riders.get(id)
            && walk.y > 0.1
        {
            let facing = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
            let along = ends
                .iter()
                .map(|d| dir_vec(*d))
                .max_by(|a, b| a.dot(facing).total_cmp(&b.dot(facing)))
                .unwrap_or(Vec3::Z);
            if cart.speed < 0.2 {
                cart.dir = along;
            }
            if cart.dir.dot(along) > 0.0 {
                cart.speed = (cart.speed + PUSH * dt).min(MAX_SPEED * 0.6);
            }
        }
        // Slopes pull carts down them (and a cart left on one rolls).
        let slope = reg.rail_slope(rail).map(dir_vec);
        if let Some(up) = slope {
            if cart.speed < 0.2 && riders.get(id).is_none_or(|(w, _)| w.y <= 0.1) {
                cart.dir = -up;
                cart.speed = cart.speed.max(0.3);
            }
            if cart.dir.dot(up) > 0.5 {
                cart.speed -= SLOPE_PULL * dt;
                if cart.speed <= 0.0 {
                    // Not fast enough to get up: roll back down.
                    cart.dir = -up;
                    cart.speed = 0.3;
                }
            } else {
                cart.speed = (cart.speed + SLOPE_PULL * dt).min(MAX_SPEED);
            }
        }
        // Powered rails speed up (or brake), other rails slow down a bit.
        if reg.variant_kind(rail) == Some(VariantKind::PoweredRail) {
            if reg.is_on(rail) {
                if cart.speed < 0.2 {
                    cart.dir = dir_vec(ends[0]);
                }
                cart.speed = (cart.speed + BOOST * dt).min(MAX_SPEED);
            } else {
                cart.speed = (cart.speed - BRAKE * dt).max(0.0);
            }
        } else {
            cart.speed = (cart.speed - FRICTION * dt).max(0.0);
        }
        if cart.speed <= 0.0 {
            continue;
        }
        // Make sure we're heading along this rail.
        if !ends.iter().any(|d| dir_vec(*d).dot(cart.dir) > 0.9) {
            let back = cart.dir * -1.0;
            cart.dir = ends
                .iter()
                .map(|d| dir_vec(*d))
                .find(|v| v.dot(back) < 0.9)
                .unwrap_or(cart.dir);
        }
        let before = e.position;
        let mut next = e.position + cart.dir * cart.speed * dt;
        // On a slope the height follows the ramp (so it carries on into
        // the rail at the top, or the one at the bottom).
        if let Some(up) = slope {
            next.y = ramp_height(cell, next, up);
        }
        let mut next_cell = next.floor().as_ivec3();
        if next_cell != cell
            && rail_at(next_cell).is_none()
            && let Some(below) = rail_at(next_cell - IVec3::Y)
            && let Some(up) = reg.rail_slope(below)
        {
            // Over the top of a slope going down: onto the ramp.
            next_cell -= IVec3::Y;
            next.y = ramp_height(next_cell, next, dir_vec(up));
        }
        if next_cell != cell && rail_at(next_cell).is_none() {
            // Rails a step up or down: up costs speed, down gives some.
            if rail_at(next_cell + IVec3::Y).is_some() {
                next_cell += IVec3::Y;
                next.y += 1.0;
                cart.speed -= STEP_SPEED;
            } else if rail_at(next_cell - IVec3::Y).is_some() {
                next_cell -= IVec3::Y;
                next.y -= 1.0;
                cart.speed = (cart.speed + STEP_SPEED).min(MAX_SPEED);
            }
            if cart.speed <= 0.0 {
                // Not fast enough to climb: roll back down.
                cart.speed = 0.5;
                cart.dir *= -1.0;
                continue;
            }
        }
        if next_cell != cell {
            match rail_at(next_cell).and_then(|r| reg.rail_ends(r)) {
                Some(next_ends) => {
                    // Came in from the side opposite our heading; leave by
                    // the rail's other end.
                    let came_from = dir_of(cart.dir * -1.0);
                    let exit = next_ends.iter().copied().find(|d| Some(*d) != came_from);
                    match (came_from.filter(|c| next_ends.contains(c)), exit) {
                        (Some(_), Some(out)) => {
                            cart.dir = dir_vec(out);
                            if let Some(up) = rail_at(next_cell).and_then(|r| reg.rail_slope(r)) {
                                next.y = ramp_height(next_cell, next, dir_vec(up));
                            }
                            // Keep to the middle of the rail.
                            let c = next_cell.as_vec3() + Vec3::splat(0.5);
                            if cart.dir.x != 0.0 {
                                next.z = c.z;
                            } else {
                                next.x = c.x;
                            }
                        }
                        _ => {
                            cart.speed = 0.0;
                            next = before;
                        }
                    }
                }
                None => {
                    // The end of the line.
                    cart.speed = 0.0;
                    next = before;
                }
            }
        }
        e.position = next;
        e.yaw = cart.dir.x.atan2(cart.dir.z);
        e.moved = true;
    }
    // Riders sit in their carts.
    for mut p in &mut players {
        let Some(id) = p.cart else { continue };
        match entities.list.get(&id) {
            Some(e) => {
                let seat = e.position + Vec3::Y * 0.3;
                p.physics.position = seat;
                p.physics.velocity = Vec3::ZERO;
                p.state.position = seat.to_array();
            }
            None => {
                p.cart = None;
                net.send(
                    p.client,
                    Channel::Reliable,
                    &ServerMessage::InCart { cart: None },
                );
            }
        }
    }
}

/// How high a cart at `at` sits on the slope in `cell` going up towards
/// `up`: from the bottom edge to a block higher at the top edge.
fn ramp_height(cell: IVec3, at: Vec3, up: Vec3) -> f32 {
    let centre = cell.as_vec3() + Vec3::splat(0.5);
    let along = ((at - centre).dot(up) + 0.5).clamp(0.0, 1.0);
    cell.y as f32 + 0.05 + along
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carts_ride_up_ramps() {
        let cell = IVec3::new(4, 10, -3);
        let up = Vec3::new(0.0, 0.0, -1.0); // up to the north
        let h = |z: f32| ramp_height(cell, Vec3::new(4.5, 0.0, z), up);
        assert!((h(-2.0) - 10.05).abs() < 1e-4, "bottom (south edge)");
        assert!((h(-2.5) - 10.55).abs() < 1e-4, "middle");
        assert!((h(-3.0) - 11.05).abs() < 1e-4, "top (north edge)");
    }

    #[test]
    fn directions_from_vectors() {
        assert_eq!(dir_of(Vec3::new(0.0, 0.0, -1.0)), Some(Dir4::North));
        assert_eq!(dir_of(Vec3::new(1.0, 0.0, 0.0)), Some(Dir4::East));
        assert_eq!(dir_of(Vec3::new(0.5, 0.0, 0.5)), None);
    }
}

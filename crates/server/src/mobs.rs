//! Animals: they wander, follow food, have babies, give wool and eggs.
//!
//! Each animal is an entity (so clients get told where it is, like any
//! other entity) plus some state here: what it's doing and its timers.
//! The client draws it from its kind's definition.
//!
//! Animals appear on their own on grass around players, and quietly go
//! away when everyone is far off; they aren't saved.

use std::collections::HashMap;

use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::item::ItemId;
use ourcraft_common::mobs::{MobDef, mob_model, parse_mob_model};
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};
use ourcraft_common::world::WORLD_HEIGHT;
use ourcraft_mod_api::{EffectKind, EntityMotion};

use crate::entities::{Entities, ServerEntity};
use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{OpenWorld, Registry, ServerWorld};

/// Animals wanted within [`NEAR`] blocks of each player.
const WANTED_NEAR_PLAYER: usize = 8;
const NEAR: f32 = 48.0;
/// At most this many animals in the whole world.
const MAX_MOBS: usize = 60;
/// Natural animals further than this from everyone go away.
const DESPAWN: f32 = 96.0;
const GRAVITY: f32 = 20.0;
/// How close you must be to use (pet, feed, shear) an animal.
const USE_REACH: f32 = 6.0;
/// Seconds a baby takes to grow up.
const GROW_UP: f32 = 300.0;
/// Seconds for wool to grow back.
const WOOL_REGROW: f32 = 90.0;
/// Seconds an animal stays "in love" after eating.
const LOVE: f32 = 20.0;
/// Animals notice food held this close (blocks).
const FOLLOW: f32 = 10.0;
/// Seconds before it can have another baby.
const BREED_COOLDOWN: f32 = 60.0;

pub struct MobsPlugin;

impl Plugin for MobsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MobStates>()
            .insert_resource(Rng(0x9E37_79B9))
            .add_systems(
                Update,
                (
                    use_mobs,
                    spawn_naturally,
                    think,
                    walk,
                    life_events,
                    despawn_far,
                )
                    .chain()
                    .after(crate::players::PlayersSet),
            );
    }
}

/// A tiny random number generator.
#[derive(Resource)]
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    /// 0..1
    fn unit(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Goal {
    Idle,
    /// Walk in this direction (yaw).
    Wander(f32),
    /// Walk towards a point.
    Towards(Vec3),
}

#[derive(Debug, Clone)]
pub struct MobState {
    pub kind: String,
    /// Appeared on its own (and so may go away on its own).
    natural: bool,
    goal: Goal,
    /// Seconds until it picks a new goal.
    timer: f32,
    vy: f32,
    /// Seconds left as a baby (0 = grown up).
    baby_for: f32,
    /// Seconds until the wool is back (0 = has wool).
    sheared_for: f32,
    love: f32,
    breed_cooldown: f32,
    /// Seconds until it lays (chickens) or makes a noise.
    egg_in: f32,
    noise_in: f32,
}

impl MobState {
    fn new(kind: &str, natural: bool, baby: bool, rng: &mut Rng) -> Self {
        Self {
            kind: kind.to_owned(),
            natural,
            goal: Goal::Idle,
            timer: rng.range(0.5, 3.0),
            vy: 0.0,
            baby_for: if baby { GROW_UP } else { 0.0 },
            sheared_for: 0.0,
            love: 0.0,
            breed_cooldown: 0.0,
            egg_in: rng.range(60.0, 150.0),
            noise_in: rng.range(8.0, 30.0),
        }
    }

    fn model(&self) -> String {
        mob_model(&self.kind, self.baby_for > 0.0, self.sheared_for > 0.0)
    }
}

/// Entity id → animal state.
#[derive(Resource, Default)]
pub struct MobStates(pub HashMap<u64, MobState>);

/// Put an animal into the world. (Its state is created on the next tick.)
pub fn spawn_mob(entities: &mut Entities, name: &str, at: Vec3, baby: bool) -> u64 {
    let yaw = (at.x * 3.1 + at.z * 1.7).sin() * std::f32::consts::PI;
    entities.spawn(ServerEntity {
        model: mob_model(name, baby, false),
        position: at,
        velocity: Vec3::ZERO,
        motion: EntityMotion::Fixed,
        lifetime: 0.0,
        age: 0.0,
        owner: None,
        moved: true,
        item: None,
        yaw,
    })
}

/// A mod's animal: its model's boxes become the parts; boxes that start
/// at the ground are legs.
pub fn mob_from_spec(
    spec: &ourcraft_mod_api::MobSpec,
    model: &ourcraft_mod_api::ModelSpec,
) -> MobDef {
    let mut leg_sign = 1.0;
    let parts = model
        .boxes
        .iter()
        .map(|b| {
            let leg = if b.min[1] <= 0.01 && b.max[1] < spec.size[1] * 0.5 {
                leg_sign = -leg_sign;
                leg_sign
            } else {
                0.0
            };
            ourcraft_common::mobs::MobPart {
                min: b.min,
                max: b.max,
                color: b.color,
                leg,
                wool: false,
            }
        })
        .collect();
    MobDef {
        name: spec.name.clone(),
        display_name: spec.display_name.clone(),
        size: spec.size,
        speed: spec.speed.clamp(0.1, 6.0),
        parts,
        skin: [0; 4],
        spawns_on: spec.spawns_on.clone(),
        food: spec.food.clone(),
        shears_into: None,
        lays: None,
        sound: spec.sound.clone(),
    }
}

fn def<'a>(open: &'a OpenWorld, kind: &str) -> Option<&'a MobDef> {
    open.mobs.iter().find(|m| m.name == kind)
}

/// Replace an animal's entity with one showing a new look (baby → grown
/// up, wool → sheared), keeping its state.
fn change_look(entities: &mut Entities, states: &mut MobStates, id: u64) {
    let (Some(state), Some(old)) = (states.0.remove(&id), entities.list.get(&id).cloned()) else {
        return;
    };
    entities.remove(id);
    let new_id = entities.spawn(ServerEntity {
        model: state.model(),
        moved: true,
        ..old
    });
    states.0.insert(new_id, state);
}

/// Right-clicking an animal: shear it, feed it, or pet it.
#[allow(clippy::too_many_arguments)]
fn use_mobs(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut entities: ResMut<Entities>,
    mut states: ResMut<MobStates>,
    mut net: ResMut<Network>,
    mut rng: ResMut<Rng>,
) {
    for FromClient { client, msg } in messages.read() {
        let ClientMessage::UseEntity { id, held } = msg else {
            continue;
        };
        let Some(p) = players_map.0.get(client).and_then(|e| players.get(*e).ok()) else {
            continue;
        };
        let (Some(e), Some(state)) = (entities.list.get(id), states.0.get_mut(id)) else {
            continue;
        };
        let at = e.position;
        if p.position().distance(at) > USE_REACH {
            continue;
        }
        let Some(kind) = def(&open, &state.kind) else {
            continue;
        };
        let held = ItemId(*held);
        let held_name = open.items.name(held);
        let mut sound = None;
        let mut effect = None;
        if held_name == "core:shears" && state.sheared_for <= 0.0 && state.baby_for <= 0.0 {
            if let Some(wool) = kind
                .shears_into
                .as_deref()
                .and_then(|n| open.items.by_name(n))
            {
                let n = 1 + (rng.next() % 3) as u16;
                crate::survival::spawn_item(
                    &mut entities,
                    at + Vec3::Y * 0.8,
                    ItemStack::new(wool, n),
                );
                states.0.get_mut(id).expect("checked").sheared_for = WOOL_REGROW;
                change_look(&mut entities, &mut states, *id);
                sound = Some("grab");
            }
        } else if kind.food.as_deref() == Some(held_name) && held != ItemId::NONE {
            let fed = if state.baby_for > 0.0 {
                // Food helps babies grow up faster.
                state.baby_for = (state.baby_for - 60.0).max(0.01);
                true
            } else if state.love <= 0.0 && state.breed_cooldown <= 0.0 {
                state.love = LOVE;
                true
            } else {
                false
            };
            if fed {
                net.send(
                    *client,
                    Channel::Reliable,
                    &ServerMessage::TakeItems(NetStack {
                        item: held.0,
                        count: 1,
                        wear: 0,
                    }),
                );
                effect = Some(EffectKind::Hearts);
                sound = Some(kind.sound.as_str());
            }
        } else {
            // A pat on the head: a happy hop.
            if state.vy == 0.0 {
                state.vy = 5.0;
            }
            effect = Some(EffectKind::Hearts);
            sound = Some(kind.sound.as_str());
        }
        if let Some(kind) = effect {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Effect {
                    kind,
                    position: (at + Vec3::Y * 1.1).to_array(),
                    color: [255, 90, 140, 255],
                },
            );
        }
        if let Some(name) = sound {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Sound {
                    name: name.to_owned(),
                    at: Some(at.to_array()),
                },
            );
        }
    }
}

/// The highest solid block in a column, if it's loaded.
fn ground(world: &ServerWorld, registry: &Registry, x: i32, z: i32) -> Option<(i32, BlockId)> {
    (0..WORLD_HEIGHT)
        .rev()
        .find_map(|y| {
            let b = world.get_block(x, y, z)?;
            (!b.is_air()).then_some((y, b))
        })
        .filter(|(_, b)| registry.0.is_solid(*b))
}

/// Keep a few animals around every player.
#[allow(clippy::too_many_arguments)]
fn spawn_naturally(
    time: Res<Time>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut entities: ResMut<Entities>,
    mut states: ResMut<MobStates>,
    mut rng: ResMut<Rng>,
    mut timer: Local<f32>,
) {
    *timer += time.delta_secs();
    if *timer < 2.0 {
        return;
    }
    *timer = 0.0;
    // Pick up animals mods spawned (they have no state yet).
    let new: Vec<(u64, String, bool)> = entities
        .list
        .iter()
        .filter(|(id, _)| !states.0.contains_key(id))
        .filter_map(|(id, e)| {
            let (name, baby, _) = parse_mob_model(&e.model)?;
            Some((*id, name.to_owned(), baby))
        })
        .collect();
    for (id, name, baby) in new {
        states
            .0
            .insert(id, MobState::new(&name, false, baby, &mut rng));
    }
    if states.0.len() >= MAX_MOBS {
        return;
    }
    for p in players.iter().filter(|p| p.name.is_some()) {
        let me = p.position();
        let near = states
            .0
            .keys()
            .filter_map(|id| entities.list.get(id))
            .filter(|e| e.position.distance(me) < NEAR)
            .count();
        if near >= WANTED_NEAR_PLAYER {
            continue;
        }
        // Somewhere 20–40 blocks away, on the right kind of ground.
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let dist = rng.range(20.0, 40.0);
        let x = (me.x + angle.cos() * dist).floor() as i32;
        let z = (me.z + angle.sin() * dist).floor() as i32;
        let Some((y, top)) = ground(&world, &registry, x, z) else {
            continue;
        };
        let top_name = &registry.0.get(top).name;
        let kinds: Vec<&MobDef> = open
            .mobs
            .iter()
            .filter(|m| m.spawns_on.as_deref() == Some(top_name.as_str()))
            .collect();
        if kinds.is_empty() {
            continue;
        }
        let kind = kinds[rng.next() as usize % kinds.len()].name.clone();
        // A small group.
        for _ in 0..2 + rng.next() % 3 {
            let gx = x + (rng.next() % 5) as i32 - 2;
            let gz = z + (rng.next() % 5) as i32 - 2;
            let Some((gy, gtop)) = ground(&world, &registry, gx, gz) else {
                continue;
            };
            if gtop != top || (gy - y).abs() > 2 {
                continue;
            }
            let at = Vec3::new(gx as f32 + 0.5, gy as f32 + 1.0, gz as f32 + 0.5);
            let baby = rng.next().is_multiple_of(5);
            let id = spawn_mob(&mut entities, &kind, at, baby);
            states
                .0
                .insert(id, MobState::new(&kind, true, baby, &mut rng));
        }
    }
}

/// Decide what each animal does next: follow food, find a partner, or
/// wander about.
#[allow(clippy::too_many_arguments)]
fn think(
    time: Res<Time>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut entities: ResMut<Entities>,
    mut states: ResMut<MobStates>,
    mut net: ResMut<Network>,
    mut rng: ResMut<Rng>,
) {
    let dt = time.delta_secs();
    // Who's holding what.
    let holders: Vec<(Vec3, String)> = players
        .iter()
        .filter(|p| p.name.is_some())
        .filter_map(|p| {
            Some((
                p.position(),
                open.items.name(p.inventory.held()?).to_owned(),
            ))
        })
        .collect();
    let positions: HashMap<u64, Vec3> = entities
        .list
        .iter()
        .map(|(id, e)| (*id, e.position))
        .collect();
    let in_love: Vec<(u64, String, Vec3)> = states
        .0
        .iter()
        .filter(|(_, s)| s.love > 0.0 && s.baby_for <= 0.0)
        .filter_map(|(id, s)| Some((*id, s.kind.clone(), *positions.get(id)?)))
        .collect();
    let mut babies = Vec::new();

    for (id, s) in states.0.iter_mut() {
        let Some(&me) = positions.get(id) else {
            continue;
        };
        s.timer -= dt;
        s.love = (s.love - dt).max(0.0);
        s.breed_cooldown = (s.breed_cooldown - dt).max(0.0);
        let Some(kind) = def(&open, &s.kind) else {
            continue;
        };

        // In love: go to the nearest partner of the same kind.
        if s.love > 0.0 {
            let partner = in_love
                .iter()
                .filter(|(other, k, _)| other != id && *k == s.kind)
                .min_by(|a, b| a.2.distance(me).total_cmp(&b.2.distance(me)));
            if let Some((other, _, at)) = partner {
                if at.distance(me) < 1.4 {
                    // Each pair is handled once (by the smaller id).
                    if id < other {
                        babies.push((s.kind.clone(), (me + *at) / 2.0, *id, *other));
                    }
                } else {
                    s.goal = Goal::Towards(*at);
                    continue;
                }
            }
        }
        // Someone nearby holding my favourite food: follow them.
        let food = kind.food.as_deref();
        let follow = holders
            .iter()
            .filter(|(_, held)| Some(held.as_str()) == food)
            .map(|(at, _)| *at)
            .filter(|at| at.distance(me) < FOLLOW)
            .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)));
        if let Some(at) = follow {
            s.goal = if at.distance(me) > 2.0 {
                Goal::Towards(at)
            } else {
                Goal::Idle
            };
            continue;
        }
        if matches!(s.goal, Goal::Towards(_)) || s.timer <= 0.0 {
            s.goal = if rng.unit() < 0.45 {
                Goal::Idle
            } else {
                Goal::Wander(rng.range(-std::f32::consts::PI, std::f32::consts::PI))
            };
            s.timer = rng.range(2.0, 6.0);
        }
    }

    for (kind, at, a, b) in babies {
        for parent in [a, b] {
            if let Some(s) = states.0.get_mut(&parent) {
                s.love = 0.0;
                s.breed_cooldown = BREED_COOLDOWN;
            }
        }
        let id = spawn_mob(&mut entities, &kind, at, true);
        states
            .0
            .insert(id, MobState::new(&kind, true, true, &mut rng));
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Effect {
                kind: EffectKind::Hearts,
                position: (at + Vec3::Y).to_array(),
                color: [255, 90, 140, 255],
            },
        );
    }
}

/// Turn `from` towards `to` (radians) by at most `max`.
fn turn_towards(from: f32, to: f32, max: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let diff = (to - from + PI).rem_euclid(TAU) - PI;
    from + diff.clamp(-max, max)
}

/// Move animals: walk where they're heading, step up single blocks, fall,
/// and turn around at walls, water and cliffs.
fn walk(
    time: Res<Time>,
    open: Res<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut entities: ResMut<Entities>,
    mut states: ResMut<MobStates>,
    mut rng: ResMut<Rng>,
) {
    let dt = time.delta_secs().min(0.1);
    let block = |p: Vec3| {
        let b = p.floor().as_ivec3();
        world.get_block(b.x, b.y, b.z)
    };
    let solid = |p: Vec3| block(p).is_some_and(|b| registry.0.is_solid(b));
    let liquid = |p: Vec3| block(p).is_some_and(|b| registry.0.is_liquid(b));

    for (id, s) in states.0.iter_mut() {
        let Some(e) = entities.list.get_mut(id) else {
            continue;
        };
        let Some(kind) = def(&open, &s.kind) else {
            continue;
        };
        // Don't move in unloaded places.
        if block(e.position).is_none() {
            continue;
        }
        let before = (e.position, e.yaw);
        let half = kind.size[0] / 2.0;

        // Falling, floating and hopping.
        let on_ground = solid(e.position - Vec3::Y * 0.05);
        if liquid(e.position + Vec3::Y * 0.3) {
            s.vy = 2.0; // bob up to the surface
        } else if !on_ground || s.vy > 0.0 {
            s.vy -= GRAVITY * dt;
        } else {
            s.vy = 0.0;
        }
        if s.vy != 0.0 {
            let next = e.position + Vec3::Y * s.vy * dt;
            if s.vy < 0.0 && solid(next) {
                e.position.y = next.y.floor() + 1.0;
                s.vy = 0.0;
            } else if s.vy > 0.0 && solid(next + Vec3::Y * kind.size[1]) {
                s.vy = 0.0;
            } else {
                e.position = next;
            }
        }

        // Walking.
        let target_yaw = match s.goal {
            Goal::Idle => None,
            Goal::Wander(yaw) => Some(yaw),
            Goal::Towards(at) => {
                let d = at - e.position;
                Some(d.x.atan2(d.z))
            }
        };
        if let Some(yaw) = target_yaw {
            e.yaw = turn_towards(e.yaw, yaw, 4.0 * dt);
            let dir = Vec3::new(e.yaw.sin(), 0.0, e.yaw.cos());
            let speed = kind.speed
                * if matches!(s.goal, Goal::Towards(_)) {
                    1.4
                } else {
                    1.0
                };
            let next = e.position + dir * speed * dt;
            let probe = next + dir * half;
            let feet = Vec3::new(probe.x, e.position.y + 0.01, probe.z);
            let mut blocked = false;
            let mut step_up = false;
            if solid(feet) {
                // One block up is fine if there's room above it.
                let room = (1..=kind.size[1].ceil() as i32 + 1)
                    .all(|dy| !solid(feet + Vec3::Y * dy as f32));
                if on_ground && room {
                    step_up = true;
                } else {
                    blocked = true;
                }
            } else {
                // Don't walk into water or off cliffs.
                let drop = (1..=4).find(|d| solid(feet - Vec3::Y * *d as f32));
                if drop.is_none() || liquid(feet) || liquid(feet - Vec3::Y) {
                    blocked = true;
                }
            }
            if blocked {
                s.goal = Goal::Wander(e.yaw + std::f32::consts::PI + rng.range(-1.0, 1.0));
                s.timer = rng.range(1.0, 3.0);
            } else {
                e.position.x = next.x;
                e.position.z = next.z;
                if step_up {
                    e.position.y = e.position.y.floor() + 1.0;
                }
            }
        }
        if (e.position, e.yaw) != before {
            e.moved = true;
        }
    }
}

/// Growing up, wool growing back, eggs, and the odd noise.
#[allow(clippy::too_many_arguments)]
fn life_events(
    time: Res<Time>,
    open: Res<OpenWorld>,
    players: Query<&ConnectedPlayer>,
    mut entities: ResMut<Entities>,
    mut states: ResMut<MobStates>,
    mut net: ResMut<Network>,
    mut rng: ResMut<Rng>,
) {
    let dt = time.delta_secs();
    let mut new_look = Vec::new();
    let mut eggs = Vec::new();
    let mut noises = Vec::new();
    for (id, s) in states.0.iter_mut() {
        let Some(e) = entities.list.get(id) else {
            continue;
        };
        if s.baby_for > 0.0 {
            s.baby_for -= dt;
            if s.baby_for <= 0.0 {
                s.baby_for = 0.0;
                new_look.push(*id);
            }
        }
        if s.sheared_for > 0.0 {
            s.sheared_for -= dt;
            if s.sheared_for <= 0.0 {
                s.sheared_for = 0.0;
                new_look.push(*id);
            }
        }
        let Some(kind) = def(&open, &s.kind) else {
            continue;
        };
        if s.baby_for <= 0.0
            && let Some(item) = kind.lays.as_deref().and_then(|n| open.items.by_name(n))
        {
            s.egg_in -= dt;
            if s.egg_in <= 0.0 {
                s.egg_in = rng.range(60.0, 150.0);
                eggs.push((e.position, item));
            }
        }
        s.noise_in -= dt;
        if s.noise_in <= 0.0 {
            s.noise_in = rng.range(10.0, 40.0);
            let heard = players
                .iter()
                .any(|p| p.position().distance(e.position) < 16.0);
            if heard {
                noises.push((kind.sound.clone(), e.position));
            }
        }
    }
    for id in new_look {
        change_look(&mut entities, &mut states, id);
    }
    for (at, item) in eggs {
        crate::survival::spawn_item(&mut entities, at + Vec3::Y * 0.3, ItemStack::new(item, 1));
    }
    for (name, at) in noises {
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Sound {
                name,
                at: Some(at.to_array()),
            },
        );
    }
}

/// Animals that appeared on their own go away when nobody is near.
fn despawn_far(
    players: Query<&ConnectedPlayer>,
    mut entities: ResMut<Entities>,
    mut states: ResMut<MobStates>,
    mut timer: Local<u32>,
) {
    *timer += 1;
    if !(*timer).is_multiple_of(60) {
        return;
    }
    let near: Vec<Vec3> = players
        .iter()
        .filter(|p| p.name.is_some())
        .map(|p| p.position())
        .collect();
    let gone: Vec<u64> = states
        .0
        .iter()
        .filter(|(id, s)| {
            let Some(e) = entities.list.get(*id) else {
                return true; // removed by a mod
            };
            s.natural && !near.iter().any(|p| p.distance(e.position) < DESPAWN)
        })
        .map(|(id, _)| *id)
        .collect();
    for id in gone {
        states.0.remove(&id);
        entities.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_takes_the_short_way() {
        let a = turn_towards(3.0, -3.0, 0.1);
        assert!(a > 3.0, "turns past PI instead of all the way round");
        assert!((turn_towards(0.0, 0.05, 0.1) - 0.05).abs() < 1e-5);
    }

    #[test]
    fn mod_mob_legs_are_found() {
        let model = ourcraft_mod_api::ModelSpec {
            name: "m:dog".into(),
            boxes: vec![
                ourcraft_mod_api::ModelBox {
                    min: [-0.3, 0.3, -0.4],
                    max: [0.3, 0.7, 0.4],
                    color: [1, 2, 3, 255],
                },
                ourcraft_mod_api::ModelBox {
                    min: [-0.25, 0.0, -0.3],
                    max: [-0.15, 0.3, -0.2],
                    color: [1, 2, 3, 255],
                },
            ],
            spin: false,
        };
        let spec = ourcraft_mod_api::MobSpec {
            name: "m:dog".into(),
            display_name: "Dog".into(),
            model: "m:dog".into(),
            size: [0.6, 0.8],
            speed: 2.0,
            spawns_on: None,
            food: None,
            sound: "pop".into(),
        };
        let d = mob_from_spec(&spec, &model);
        assert_eq!(d.parts[0].leg, 0.0);
        assert_ne!(d.parts[1].leg, 0.0);
    }
}

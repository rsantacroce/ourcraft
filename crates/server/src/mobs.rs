//! Animals: they wander, follow food, have babies, give wool and eggs.
//!
//! Each animal is an entity (so clients get told where it is, like any
//! other entity) plus some state here: what it's doing and its timers.
//! The client draws it from its kind's definition.
//!
//! Animals appear on their own on grass around players, and quietly go
//! away when everyone is far off. Once a player looks after one (pets,
//! feeds or shears it), or it's born or summoned, it's *kept*: it stays,
//! and it's saved with the world.

use std::collections::HashMap;

use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::item::ItemId;
use ourcraft_common::mobs::{MobDef, mob_model, parse_mob_model};
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};
use ourcraft_common::save::SavedAnimal;
use ourcraft_common::world::WORLD_HEIGHT;
use ourcraft_mod_api::{EffectKind, EntityMotion};

use crate::entities::{Entities, ServerEntity};
use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{OpenWorld, Registry, ServerWorld};

/// Animals wanted within [`NEAR`] blocks of each player.
const WANTED_NEAR_PLAYER: usize = 8;
const NEAR: f32 = 48.0;
/// At most this many wild animals in the whole world.
const MAX_MOBS: usize = 60;
/// Kept animals stop having babies beyond this (so the server stays quick).
const MAX_KEPT: usize = 200;
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
/// Monsters wanted near each survival player at night.
const MONSTERS_NEAR_PLAYER: usize = 4;
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
                    hit_mobs,
                    spawn_naturally,
                    think,
                    walk,
                    monsters,
                    life_events,
                    despawn_far,
                    remember_kept,
                    carry_riders,
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
    /// Monsters: health left (set when first hit).
    health: Option<f32>,
    /// Seconds until a monster can bonk again, or hop again.
    attack_cd: f32,
    hop_cd: f32,
    /// Seconds left running away after being hit.
    flee: f32,
    /// Wool colour (sheep; 0 = white). See `colours::COLOURS`.
    colour: u8,
    /// On a rope, led by this player.
    leash: Option<u64>,
    /// Wearing a saddle (pigs), so it can be ridden.
    saddled: bool,
    /// Someone's riding it: it goes where they go.
    ridden: bool,
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
            health: None,
            attack_cd: 0.0,
            hop_cd: 0.0,
            flee: 0.0,
            colour: 0,
            leash: None,
            saddled: false,
            ridden: false,
        }
    }

    fn model(&self) -> String {
        ourcraft_common::mobs::mob_model_coloured(
            &self.kind,
            self.baby_for > 0.0,
            self.sheared_for > 0.0,
            self.colour,
        )
    }

    /// Saved with the world: looked after by a player (not wild), and not
    /// a monster.
    fn kept(&self, open: &OpenWorld) -> bool {
        !self.natural && def(open, &self.kind).is_some_and(|d| d.hostile.is_none())
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
        hostile: None,
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
    mut players: Query<&mut ConnectedPlayer>,
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
        let Some(mut p) = players_map
            .0
            .get(client)
            .and_then(|e| players.get_mut(*e).ok())
        else {
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
        if kind.hostile.is_none() {
            state.natural = false;
        }
        let held = ItemId(*held);
        let held_name = open.items.name(held);
        let mut sound = None;
        let mut effect = None;
        let dye = ourcraft_common::colours::dye_colour(held_name);
        let take_one = |net: &mut Network| {
            net.send(
                *client,
                Channel::Reliable,
                &ServerMessage::TakeItems(NetStack {
                    item: held.0,
                    count: 1,
                    wear: 0,
                }),
            );
        };
        if held_name == "core:rope" && kind.hostile.is_none() {
            // A rope: it follows you (or, if it already does, it's let go).
            state.leash = match state.leash {
                Some(c) if c == *client => None,
                _ => Some(*client),
            };
            effect = Some(EffectKind::Hearts);
            sound = Some(kind.sound.as_str());
        } else if held_name == "core:saddle" && kind.name == "core:pig" && !state.saddled {
            state.saddled = true;
            if !p.creative {
                take_one(&mut net);
            }
            sound = Some("grab");
        } else if state.saddled
            && !kind.likes(held_name)
            && p.riding.is_none()
            && p.boat.is_none()
            && state.baby_for <= 0.0
        {
            // Hop on.
            p.riding = Some(*id);
            p.physics.position = at;
            p.physics.velocity = Vec3::ZERO;
            p.state.position = at.to_array();
            state.ridden = true;
            net.send(
                *client,
                Channel::Reliable,
                &ServerMessage::Riding { mount: Some(*id) },
            );
            sound = Some(kind.sound.as_str());
        } else if let Some(colour) =
            dye.filter(|c| kind.shears_into.is_some() && *c != state.colour)
        {
            // Dyeing a sheep: its wool (and what grows back) is that colour.
            state.colour = colour;
            change_look(&mut entities, &mut states, *id);
            net.send(
                *client,
                Channel::Reliable,
                &ServerMessage::TakeItems(NetStack {
                    item: held.0,
                    count: 1,
                    wear: 0,
                }),
            );
            sound = Some("pop");
        } else if held_name == "core:shears" && state.sheared_for <= 0.0 && state.baby_for <= 0.0 {
            let coloured = (state.colour > 0).then(|| {
                ourcraft_common::colours::wool_name(
                    ourcraft_common::colours::COLOURS[state.colour as usize % 16].0,
                )
            });
            if let Some(wool) = coloured
                .as_deref()
                .or(kind.shears_into.as_deref())
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
        } else if kind.likes(held_name) && held != ItemId::NONE {
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

/// Monsters only appear where the light from torches and lamps is below
/// this (0–15).
const DARK_ENOUGH: u8 = 7;

/// About how much light torches, lamps and lava give a spot: the
/// brightest one nearby, one level less per block away (walls aren't
/// counted, so it's never darker than the player sees).
fn block_light(world: &ServerWorld, registry: &Registry, at: IVec3) -> u8 {
    const R: i32 = 14;
    let mut best = 0u8;
    for dy in -R..=R {
        for dz in -R..=R {
            let left = R - dy.abs() - dz.abs();
            if left < 0 {
                continue;
            }
            for dx in -left..=left {
                let p = at + IVec3::new(dx, dy, dz);
                let Some(b) = world.get_block(p.x, p.y, p.z) else {
                    continue;
                };
                let emit = registry.0.get(b).light;
                let dist = (dx.abs() + dy.abs() + dz.abs()) as u8;
                best = best.max(emit.saturating_sub(dist));
            }
        }
    }
    best
}

/// Somewhere to stand in a cave near height `y`: air with room for a
/// monster, solid floor, and rock overhead. Returns the y of the feet.
fn cave_floor(world: &ServerWorld, registry: &Registry, x: i32, y: i32, z: i32) -> Option<i32> {
    let solid = |y: i32| {
        world
            .get_block(x, y, z)
            .is_some_and(|b| registry.0.is_solid(b))
    };
    let air = |y: i32| world.get_block(x, y, z).is_some_and(|b| b.is_air());
    let top = ground(world, registry, x, z)?.0;
    (y - 10..=y + 6)
        .rev()
        .find(|&f| f < top && solid(f - 1) && air(f) && air(f + 1))
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
    let new: Vec<(u64, String, bool, u8)> = entities
        .list
        .iter()
        .filter(|(id, _)| !states.0.contains_key(id))
        .filter_map(|(id, e)| {
            let (name, baby, _) = parse_mob_model(&e.model)?;
            let colour = ourcraft_common::mobs::mob_colour(&e.model);
            Some((*id, name.to_owned(), baby, colour))
        })
        .collect();
    for (id, name, baby, colour) in new {
        let mut state = MobState::new(&name, false, baby, &mut rng);
        state.colour = colour % 16;
        states.0.insert(id, state);
    }
    if states.0.values().filter(|s| s.natural).count() >= MAX_MOBS {
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
            let mut state = MobState::new(&kind, true, baby, &mut rng);
            // Most sheep are white; a few are grey, black or brown.
            let woolly = def(&open, &kind).is_some_and(|d| d.shears_into.is_some());
            if woolly {
                state.colour = match rng.next() % 100 {
                    0..80 => 0,
                    80..86 => 1,
                    86..92 => 2,
                    92..97 => 3,
                    _ => 4,
                };
            }
            let coloured = state.colour > 0;
            states.0.insert(id, state);
            if coloured {
                change_look(&mut entities, &mut states, id);
            }
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
    let leaders: HashMap<u64, Vec3> = players
        .iter()
        .filter(|p| p.name.is_some())
        .map(|p| (p.client, p.position()))
        .collect();
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

    // Survival players monsters can chase.
    let prey: Vec<Vec3> = players
        .iter()
        .filter(|p| crate::needs::applies(p, &open))
        .map(|p| p.position())
        .collect();

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
        // Just got bopped: keep running.
        if s.flee > 0.0 {
            s.flee -= dt;
            continue;
        }
        // Monsters go after the nearest survival player.
        if kind.hostile.is_some() {
            let target = prey
                .iter()
                .filter(|p| p.distance(me) < 20.0)
                .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)));
            if let Some(at) = target {
                s.goal = Goal::Towards(*at);
                continue;
            }
        }

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
        // On a rope: follow whoever holds it (let go if they're far away).
        if let Some(leader) = s.leash {
            match leaders.get(&leader) {
                Some(at) if at.distance(me) < 16.0 => {
                    s.goal = if at.distance(me) > 2.5 {
                        Goal::Towards(*at)
                    } else {
                        Goal::Idle
                    };
                    continue;
                }
                _ => s.leash = None,
            }
        }
        // Someone nearby holding my favourite food: follow them.
        let follow = holders
            .iter()
            .filter(|(_, held)| kind.likes(held))
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

    let kept = states.0.values().filter(|s| !s.natural).count();
    for (kind, at, a, b) in babies.into_iter().take(MAX_KEPT.saturating_sub(kept)) {
        for parent in [a, b] {
            if let Some(s) = states.0.get_mut(&parent) {
                s.love = 0.0;
                s.breed_cooldown = BREED_COOLDOWN;
            }
        }
        let id = spawn_mob(&mut entities, &kind, at, true);
        let mut baby = MobState::new(&kind, false, true, &mut rng);
        // Takes after one of its parents.
        let pick = if rng.next().is_multiple_of(2) { a } else { b };
        baby.colour = states.0.get(&pick).map_or(0, |s| s.colour);
        let coloured = baby.colour > 0;
        states.0.insert(id, baby);
        if coloured {
            change_look(&mut entities, &mut states, id);
        }
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
        // Don't move in unloaded places, or when someone's riding it.
        if block(e.position).is_none() || s.ridden {
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
        // Bouncy monsters move only in hops.
        let hops = kind.hostile.is_some_and(|h| h.hops);
        if hops && on_ground && s.vy == 0.0 {
            s.hop_cd -= dt;
            if target_yaw.is_some() && s.hop_cd <= 0.0 {
                s.hop_cd = 0.5;
                s.vy = 6.0;
            }
        }
        let grounded_hopper = hops && on_ground && s.vy <= 0.0;
        if let Some(yaw) = target_yaw.filter(|_| !grounded_hopper) {
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

/// Bopping animals (they hop away) and monsters (they lose health, and
/// go away in a puff of confetti when it runs out).
#[allow(clippy::too_many_arguments)]
fn hit_mobs(
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
        let ClientMessage::HitEntity { id, held } = msg else {
            continue;
        };
        let Some(p) = players_map.0.get(client).and_then(|e| players.get(*e).ok()) else {
            continue;
        };
        let (Some(e), Some(s)) = (entities.list.get_mut(id), states.0.get_mut(id)) else {
            continue;
        };
        if p.position().distance(e.position) > USE_REACH || p.name.is_none() {
            continue;
        }
        let Some(kind) = def(&open, &s.kind) else {
            continue;
        };
        // Knocked back a little, and runs off.
        let away = (e.position - p.position()).with_y(0.0).normalize_or_zero();
        e.position += away * 0.6;
        e.moved = true;
        s.vy = 4.0;
        s.flee = 1.5;
        s.goal = Goal::Wander(away.x.atan2(away.z));
        let at = e.position;
        let Some(h) = kind.hostile else {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Sound {
                    name: kind.sound.clone(),
                    at: Some(at.to_array()),
                },
            );
            continue;
        };
        // Tools hit harder: hand 2, wood 3, stone 4, iron 5, diamond 6.
        let tier = open
            .items
            .get(ourcraft_common::item::ItemId(*held))
            .and_then(|d| d.tool())
            .map_or(0, |(_, t)| t);
        let health = s.health.get_or_insert(h.health);
        *health -= 2.0 + tier as f32;
        let gone = *health <= 0.0;
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Sound {
                name: "bonk".into(),
                at: Some(at.to_array()),
            },
        );
        if gone {
            states.0.remove(id);
            entities.remove(*id);
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Effect {
                    kind: EffectKind::Confetti,
                    position: (at + Vec3::Y * 0.6).to_array(),
                    color: [255, 255, 255, 255],
                },
            );
            // A little thank-you for being brave: sometimes some coal.
            if rng.next().is_multiple_of(2)
                && let Some(coal) = open.items.by_name("core:coal")
            {
                crate::survival::spawn_item(
                    &mut entities,
                    at + Vec3::Y * 0.5,
                    ItemStack::new(coal, 1),
                );
            }
        }
    }
}

/// Night: monsters come out near survival players and bonk them. Morning
/// (or peaceful): they go away.
#[allow(clippy::too_many_arguments)]
fn monsters(
    time: Res<Time>,
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut entities: ResMut<Entities>,
    mut states: ResMut<MobStates>,
    mut net: ResMut<Network>,
    mut rng: ResMut<Rng>,
    mut timer: Local<f32>,
) {
    let dt = time.delta_secs();
    let night = crate::building::is_night(open.time_of_day());
    let allowed = !open.peaceful && open.meta.game_mode.is_none();
    let is_monster = |s: &MobState| def(&open, &s.kind).is_some_and(|d| d.hostile.is_some());

    // Bonks.
    for (id, s) in states.0.iter_mut() {
        s.attack_cd = (s.attack_cd - dt).max(0.0);
        let Some(h) = def(&open, &s.kind).and_then(|d| d.hostile) else {
            continue;
        };
        let Some(e) = entities.list.get(id) else {
            continue;
        };
        if s.attack_cd > 0.0 {
            continue;
        }
        for mut p in &mut players {
            if !crate::needs::applies(&p, &open) {
                continue;
            }
            let body = ourcraft_common::physics::Aabb::player(p.position()).inflate(0.5);
            let close =
                e.position.cmpge(body.min - Vec3::Y).all() && e.position.cmple(body.max).all();
            if close {
                s.attack_cd = 1.2;
                // Armour softens the bonk.
                let points = p.inventory.armour_points(&open.items) as f32;
                let soften =
                    (1.0 - points * ourcraft_common::adventure::PROTECTION_PER_POINT).max(0.2);
                p.needs.hurt(h.damage * soften);
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::Sound {
                        name: "bonk".into(),
                        at: Some(e.position.to_array()),
                    },
                );
                break;
            }
        }
    }

    *timer += dt;
    if *timer < 2.0 {
        return;
    }
    *timer = 0.0;

    // Morning: monsters under the open sky puff away, a few at a time
    // (in peaceful, all of them). Cave monsters stay in their caves.
    let open_sky = |p: Vec3| {
        let b = p.floor().as_ivec3();
        ground(&world, &registry, b.x, b.z).is_none_or(|(y, _)| y < b.y)
    };
    if !allowed || !night {
        let leaving: Vec<u64> = states
            .0
            .iter()
            .filter(|(_, s)| is_monster(s))
            .filter(|(id, _)| {
                !allowed || entities.list.get(id).is_none_or(|e| open_sky(e.position))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in leaving {
            if rng.unit() < 0.5 {
                continue;
            }
            states.0.remove(&id);
            if let Some(e) = entities.list.get(&id) {
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::Effect {
                        kind: EffectKind::Poof,
                        position: (e.position + Vec3::Y * 0.6).to_array(),
                        color: [255, 255, 255, 255],
                    },
                );
            }
            entities.remove(id);
        }
        if !allowed {
            return;
        }
    }

    // A few around each survival player: on the surface at night, and in
    // dark caves any time. Never where torches or lamps light things up.
    let kinds: Vec<String> = open
        .mobs
        .iter()
        .filter(|m| m.hostile.is_some())
        .map(|m| m.name.clone())
        .collect();
    if kinds.is_empty() {
        return;
    }
    let hunted: Vec<Vec3> = players
        .iter()
        .filter(|p| crate::needs::applies(p, &open))
        .map(|p| p.position())
        .collect();
    for me in hunted {
        let near = states
            .0
            .iter()
            .filter(|(_, s)| is_monster(s))
            .filter_map(|(id, _)| entities.list.get(id))
            .filter(|e| e.position.distance(me) < NEAR)
            .count();
        let wild = states.0.values().filter(|s| s.natural).count();
        if near >= MONSTERS_NEAR_PLAYER || wild >= MAX_MOBS {
            continue;
        }
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let in_cave = !open_sky(me + Vec3::Y);
        let dist = if in_cave {
            rng.range(10.0, 22.0)
        } else {
            rng.range(16.0, 30.0)
        };
        let x = (me.x + angle.cos() * dist).floor() as i32;
        let z = (me.z + angle.sin() * dist).floor() as i32;
        let spot = if in_cave {
            cave_floor(&world, &registry, x, me.y.floor() as i32, z)
        } else if night {
            ground(&world, &registry, x, z)
                .filter(|(_, top)| !registry.0.is_liquid(*top))
                .map(|(y, _)| y + 1)
        } else {
            None
        };
        let Some(y) = spot else {
            continue;
        };
        if block_light(&world, &registry, IVec3::new(x, y, z)) >= DARK_ENOUGH {
            continue;
        }
        let y = y - 1;
        let kind = kinds[rng.next() as usize % kinds.len()].clone();
        let at = Vec3::new(x as f32 + 0.5, y as f32 + 1.0, z as f32 + 0.5);
        let id = spawn_mob(&mut entities, &kind, at, false);
        states
            .0
            .insert(id, MobState::new(&kind, true, false, &mut rng));
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Effect {
                kind: EffectKind::Poof,
                position: (at + Vec3::Y * 0.6).to_array(),
                color: [120, 80, 170, 255],
            },
        );
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
                // Only where someone might find them.
                if players
                    .iter()
                    .any(|p| p.position().distance(e.position) < NEAR)
                {
                    eggs.push((e.position, item));
                    // …and now and then a feather.
                    if let Some(feather) = open.items.by_name("core:feather")
                        && rng.next().is_multiple_of(2)
                    {
                        eggs.push((e.position, feather));
                    }
                }
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

/// Bring back the animals saved with the world (once), then keep the list
/// to save up to date.
fn remember_kept(
    mut open: ResMut<OpenWorld>,
    mut entities: ResMut<Entities>,
    mut states: ResMut<MobStates>,
    mut rng: ResMut<Rng>,
    mut loaded: Local<bool>,
    mut missing: Local<Vec<SavedAnimal>>,
    mut timer: Local<u32>,
) {
    if !*loaded {
        *loaded = true;
        for a in &open.animals {
            // Kinds from a mod that's gone stay in the file, in case it
            // comes back.
            if def(&open, &a.kind).is_none() {
                missing.push(a.clone());
                continue;
            }
            let mut state = MobState::new(&a.kind, false, false, &mut rng);
            state.baby_for = a.baby_for.max(0.0);
            state.sheared_for = a.sheared_for.max(0.0);
            state.colour = a.colour % 16;
            let id = entities.spawn(ServerEntity {
                model: state.model(),
                position: Vec3::from_array(a.position),
                velocity: Vec3::ZERO,
                motion: EntityMotion::Fixed,
                lifetime: 0.0,
                age: 0.0,
                owner: None,
                moved: true,
                item: None,
                yaw: a.yaw,
            });
            states.0.insert(id, state);
        }
        return;
    }
    *timer += 1;
    if !(*timer).is_multiple_of(10) {
        return;
    }
    let mut list: Vec<SavedAnimal> = states
        .0
        .iter()
        .filter(|(_, s)| s.kept(&open))
        .filter_map(|(id, s)| {
            let e = entities.list.get(id)?;
            Some(SavedAnimal {
                kind: s.kind.clone(),
                position: e.position.to_array(),
                yaw: e.yaw,
                baby_for: s.baby_for,
                sheared_for: s.sheared_for,
                colour: s.colour,
            })
        })
        .collect();
    list.extend(missing.iter().cloned());
    open.bypass_change_detection().animals = list;
}

/// Ridden animals go where their riders go.
fn carry_riders(
    mut players: Query<&mut ConnectedPlayer>,
    mut entities: ResMut<Entities>,
    mut states: ResMut<MobStates>,
    mut net: ResMut<Network>,
) {
    for s in states.0.values_mut() {
        s.ridden = false;
    }
    for mut p in &mut players {
        let Some(id) = p.riding else { continue };
        let (Some(e), Some(s)) = (entities.list.get_mut(&id), states.0.get_mut(&id)) else {
            p.riding = None;
            net.send(
                p.client,
                Channel::Reliable,
                &ServerMessage::Riding { mount: None },
            );
            continue;
        };
        s.ridden = true;
        let (at, yaw) = (p.physics.position, p.state.yaw + std::f32::consts::PI);
        if e.position != at || e.yaw != yaw {
            e.position = at;
            e.yaw = yaw;
            e.moved = true;
        }
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

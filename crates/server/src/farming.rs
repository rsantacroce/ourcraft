//! Farming on the server (milestone 17): plants grow, farmland gets wet
//! or dries out, and players use cakes, composters and bone meal.
//!
//! What grows into what is in `ourcraft_common::farming`. Here, every
//! couple of seconds, we look at the chunks around players that have
//! anything growing in them, and give each plant a chance to move on.

use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::farming::{CANE_HEIGHT, StageKind};
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::item::ItemId;
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};
use ourcraft_common::world::ColumnPos;
use ourcraft_mod_api::EffectKind;

use renet::ClientId;

use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{ColumnCache, OpenWorld, Registry, ServerWorld};

/// Seconds between growing rounds.
const GROW_EVERY: f32 = 2.0;
/// Columns around each player where things grow.
const GROW_RADIUS: i32 = 6;
/// Farmland this close (sideways) to water stays wet.
const WATER_REACH: i32 = 4;

pub struct FarmingPlugin;

impl Plugin for FarmingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Hives>().add_systems(
            Update,
            (use_farm_blocks, grow).after(crate::players::PlayersSet),
        );
    }
}

/// Where the bee hives near players are (bees come out of them).
#[derive(Resource, Default)]
pub struct Hives(pub Vec<IVec3>);

/// A tiny random number generator.
fn next(seed: &mut u32) -> u32 {
    if *seed == 0 {
        *seed = 0x2545_F491;
    }
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

/// One chance in `n`.
fn chance(seed: &mut u32, n: u32) -> bool {
    next(seed).is_multiple_of(n.max(1))
}

fn change(
    world: &mut ServerWorld,
    cache: &mut ColumnCache,
    open: &OpenWorld,
    net: &mut Network,
    pos: IVec3,
    block: BlockId,
) {
    crate::building::change(world, cache, open, net, pos, block);
}

/// Where a ripe stem can put its fruit: an empty spot next to it with
/// soil underneath, unless a fruit is already there.
fn fruit_spot(
    world: &ServerWorld,
    registry: &Registry,
    stem: IVec3,
    fruit: BlockId,
    roll: u32,
) -> Option<IVec3> {
    let get = |p: IVec3| world.get_block(p.x, p.y, p.z);
    let sides = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z];
    if sides.iter().any(|d| get(stem + *d) == Some(fruit)) {
        return None;
    }
    let at = stem + sides[roll as usize % 4];
    let below = get(at - IVec3::Y)?;
    let soil = below == ourcraft_common::block::blocks::DIRT
        || below == ourcraft_common::block::blocks::GRASS
        || registry.0.is_farmland(below);
    (get(at)?.is_air() && soil).then_some(at)
}

/// Grow the tree a sapling at `at` becomes, if there's room: its trunk
/// needs empty space (leaves only fill empty space). Returns whether it
/// grew.
fn grow_tree(
    world: &mut ServerWorld,
    cache: &mut ColumnCache,
    open: &OpenWorld,
    net: &mut Network,
    registry: &Registry,
    at: IVec3,
    wood: &str,
    roll: u32,
) -> bool {
    use ourcraft_common::woods::{Part, Wood, tree_blocks, tree_shape};
    let Some(wood) = Wood::from_key(wood) else {
        return false;
    };
    let reg = &registry.0;
    let (log, leaves) = tree_blocks(reg, wood);
    let shape = tree_shape(wood, roll as u64 | ((roll as u64) << 32));
    let free = |w: &ServerWorld, p: IVec3| {
        w.get_block(p.x, p.y, p.z)
            .is_some_and(|b| b.is_air() || reg.is_soft(b) || p == at)
    };
    let fits = shape
        .iter()
        .filter(|(_, part)| *part == Part::Log)
        .all(|(o, _)| free(world, at + IVec3::from_array(*o)));
    if !fits {
        return false;
    }
    for (o, part) in &shape {
        let p = at + IVec3::from_array(*o);
        let block = match part {
            Part::Log => log,
            Part::Leaves if free(world, p) => leaves,
            Part::Leaves => continue,
        };
        change(world, cache, open, net, p, block);
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn grow(
    time: Res<Time>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
    mut timer: Local<f32>,
    mut seed: Local<u32>,
    mut hives: ResMut<Hives>,
) {
    *timer += time.delta_secs();
    if *timer < GROW_EVERY {
        return;
    }
    *timer = 0.0;
    let reg = &registry.0;
    hives.0.clear();
    // Columns near anyone.
    let mut columns: Vec<ColumnPos> = Vec::new();
    for p in players.iter().filter(|p| p.name.is_some()) {
        let here = ColumnPos::from_world(p.state.position[0], p.state.position[2]);
        for dz in -GROW_RADIUS..=GROW_RADIUS {
            for dx in -GROW_RADIUS..=GROW_RADIUS {
                let c = ColumnPos::new(here.x + dx, here.z + dz);
                if !columns.contains(&c) && world.has_column(c) {
                    columns.push(c);
                }
            }
        }
    }
    let lively = |b: BlockId| reg.grows(b) || reg.is_farmland(b);
    // Find everything that might change first, then change it.
    let mut found: Vec<(IVec3, BlockId)> = Vec::new();
    for col in columns {
        for cy in 0..ourcraft_common::world::WORLD_HEIGHT_CHUNKS {
            let pos = col.chunk(cy);
            let Some(chunk) = world.chunk(pos) else {
                continue;
            };
            if !chunk.palette().iter().any(|b| lively(*b)) {
                continue;
            }
            let origin = IVec3::new(pos.x, pos.y, pos.z) * 16;
            for ((x, y, z), b) in chunk.iter() {
                if lively(b) {
                    found.push((origin + IVec3::new(x as i32, y as i32, z as i32), b));
                }
            }
        }
    }
    let get = |world: &ServerWorld, p: IVec3| world.get_block(p.x, p.y, p.z).unwrap_or_default();
    let farmland = reg.by_name("core:farmland");
    let farmland_wet = reg.by_name("core:farmland_wet");
    for (p, b) in found {
        // Something else may have changed it already this round.
        if get(&world, p) != b {
            continue;
        }
        if reg.is_farmland(b) {
            let wet = (-WATER_REACH..=WATER_REACH).any(|dz| {
                (-WATER_REACH..=WATER_REACH).any(|dx| {
                    (0..=1).any(|dy| {
                        reg.fluid(get(&world, p + IVec3::new(dx, dy, dz)))
                            .is_some_and(|f| f.family == "core:water")
                    })
                })
            });
            let above = get(&world, p + IVec3::Y);
            let new = if wet {
                farmland_wet
            } else if above.is_air() && chance(&mut seed, 10) {
                // Dry and bare: back to dirt, now and then.
                Some(ourcraft_common::block::blocks::DIRT)
            } else {
                farmland
            };
            if let Some(new) = new.filter(|n| *n != b) {
                change(&mut world, &mut cache, &open, &mut net, p, new);
            }
            continue;
        }
        let Some(stage) = reg.stage(b).cloned() else {
            continue;
        };
        // Green plants need light: open sky (glass and leaves let it
        // through), or a torch or lamp close by. Mushrooms don't mind.
        let needs_light = matches!(
            stage.kind,
            StageKind::Crop | StageKind::Stem { .. } | StageKind::Cane | StageKind::Sapling { .. }
        );
        if needs_light && !lit(&world, &registry, p) {
            continue;
        }
        match stage.kind {
            StageKind::Crop | StageKind::Stem { .. } if stage.stage < stage.max => {
                let wet = Some(get(&world, p - IVec3::Y)) == farmland_wet;
                if chance(&mut seed, if wet { 8 } else { 20 })
                    && let Some(new) = reg.advanced(b, 1)
                {
                    change(&mut world, &mut cache, &open, &mut net, p, new);
                }
            }
            StageKind::Stem { fruit } => {
                let Some(fruit) = reg.by_name(&fruit) else {
                    continue;
                };
                let roll = next(&mut seed);
                if chance(&mut seed, 8)
                    && let Some(at) = fruit_spot(&world, &registry, p, fruit, roll)
                {
                    change(&mut world, &mut cache, &open, &mut net, at, fruit);
                }
            }
            StageKind::Hive => {
                hives.0.push(p);
                if stage.stage < stage.max
                    && chance(&mut seed, 40)
                    && let Some(full) = reg.advanced(b, 1)
                {
                    change(&mut world, &mut cache, &open, &mut net, p, full);
                }
            }
            StageKind::Sapling { wood } => {
                if chance(&mut seed, 30) {
                    let roll = next(&mut seed);
                    grow_tree(
                        &mut world, &mut cache, &open, &mut net, &registry, p, &wood, roll,
                    );
                }
            }
            StageKind::Cane => {
                // Only the top of a stalk grows, up to three tall.
                let is_cane = |w: &ServerWorld, q: IVec3| get(w, q) == b;
                let height = (0..CANE_HEIGHT)
                    .take_while(|i| is_cane(&world, p - IVec3::Y * *i))
                    .count() as i32;
                let up = p + IVec3::Y;
                if height < CANE_HEIGHT && get(&world, up).is_air() && chance(&mut seed, 10) {
                    change(&mut world, &mut cache, &open, &mut net, up, b);
                }
            }
            _ => {}
        }
    }
}

/// Light a plant needs from torches and lamps when it can't see the sky.
const PLANT_LIGHT: u8 = 9;

/// Can a plant here see the sky, or is there a lamp close enough?
fn lit(world: &ServerWorld, registry: &Registry, p: IVec3) -> bool {
    let sky = (p.y + 1..ourcraft_common::world::WORLD_HEIGHT).all(|y| {
        world
            .get_block(p.x, y, p.z)
            .is_none_or(|b| !registry.0.is_opaque(b))
    });
    sky || crate::mobs::block_light(world, registry, p) >= PLANT_LIGHT
}

/// What a composter takes, and one chance in how many it fills a level.
fn compost_chance(item: &str) -> Option<u32> {
    match item {
        "core:wheat_seeds"
        | "core:beetroot_seeds"
        | "core:pumpkin_seeds"
        | "core:melon_seeds"
        | "core:tall_grass"
        | "core:fern"
        | "core:leaves" => Some(3),
        "core:wheat"
        | "core:carrot"
        | "core:potato"
        | "core:beetroot"
        | "core:apple"
        | "core:melon_slice"
        | "core:sugar_cane"
        | "core:red_mushroom"
        | "core:brown_mushroom" => Some(2),
        "core:pumpkin" | "core:melon" | "core:bread" | "core:baked_potato" | "core:cookie"
        | "core:pumpkin_pie" => Some(1),
        f if ourcraft_common::colours::FLOWERS
            .iter()
            .any(|(n, _, _)| *n == f) =>
        {
            Some(2)
        }
        _ => None,
    }
}

/// Using cakes, composters and bone meal.
#[allow(clippy::too_many_arguments)]
fn use_farm_blocks(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut entities: ResMut<crate::entities::Entities>,
    mut net: ResMut<Network>,
    mut seed: Local<u32>,
) {
    let reg = &registry.0;
    for FromClient { client, msg } in messages.read() {
        let ClientMessage::UseBlock { pos, held } = msg else {
            continue;
        };
        let Some(mut p) = players_map
            .0
            .get(client)
            .and_then(|e| players.get_mut(*e).ok())
        else {
            continue;
        };
        let pos = IVec3::from_array(*pos);
        let eye = p.position() + Vec3::Y * ourcraft_common::physics::EYE_HEIGHT;
        if eye.distance(pos.as_vec3() + Vec3::splat(0.5)) > crate::players::MAX_REACH {
            continue;
        }
        let Some(b) = world.get_block(pos.x, pos.y, pos.z) else {
            continue;
        };
        let held = ItemId(*held);
        let held_name = open.items.name(held).to_owned();
        let survival = !p.creative;
        let take = |net: &mut Network, client: &ClientId| {
            if survival {
                net.send(
                    *client,
                    Channel::Reliable,
                    &ServerMessage::TakeItems(NetStack {
                        item: held.0,
                        count: 1,
                        wear: 0,
                    }),
                );
            }
        };
        let sound = |net: &mut Network, name: &str| {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Sound {
                    name: name.into(),
                    at: Some((pos.as_vec3() + Vec3::splat(0.5)).to_array()),
                },
            );
        };
        let sparkle = |net: &mut Network, at: IVec3| {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Effect {
                    kind: EffectKind::Poof,
                    position: (at.as_vec3() + Vec3::new(0.5, 0.4, 0.5)).to_array(),
                    color: [120, 220, 90, 255],
                },
            );
        };
        let stage = reg.stage(b).cloned();

        // ---- Bone meal: plants shoot up, grass sprouts tall grass. ----
        if held_name == "core:bone_meal" {
            if let Some(s) = &stage
                && reg.grows(b)
            {
                let grown = reg.advanced(b, 2 + (next(&mut seed) % 3) as u8);
                if let StageKind::Sapling { wood } = &s.kind {
                    // Half the time, a tree straight away.
                    let roll = next(&mut seed);
                    if roll.is_multiple_of(2) {
                        grow_tree(
                            &mut world, &mut cache, &open, &mut net, &registry, pos, wood, roll,
                        );
                    }
                } else if let Some(new) = grown {
                    change(&mut world, &mut cache, &open, &mut net, pos, new);
                } else if let StageKind::Stem { fruit } = &s.kind
                    && let Some(fruit) = reg.by_name(fruit)
                    && let Some(at) = fruit_spot(&world, &registry, pos, fruit, next(&mut seed))
                {
                    change(&mut world, &mut cache, &open, &mut net, at, fruit);
                } else {
                    continue; // nothing to do: keep the bone meal
                }
                take(&mut net, client);
                sparkle(&mut net, pos);
            } else if b == ourcraft_common::block::blocks::GRASS
                && let Some(grass) = reg.by_name("core:tall_grass")
            {
                for _ in 0..6 {
                    let r = next(&mut seed);
                    let at = pos + IVec3::new((r % 5) as i32 - 2, 1, ((r >> 8) % 5) as i32 - 2);
                    let on_grass = world.get_block(at.x, at.y - 1, at.z)
                        == Some(ourcraft_common::block::blocks::GRASS);
                    if on_grass && world.get_block(at.x, at.y, at.z) == Some(BlockId::AIR) {
                        change(&mut world, &mut cache, &open, &mut net, at, grass);
                    }
                }
                take(&mut net, client);
                sparkle(&mut net, pos + IVec3::Y);
            }
            continue;
        }
        let Some(stage) = stage else { continue };
        match stage.kind {
            // ---- A full hive: honey in a bowl. ----
            StageKind::Hive => {
                if stage.stage >= stage.max
                    && held_name == "core:bowl"
                    && let (Some(empty), Some(honey)) = (
                        reg.at_stage(&stage.family, 0),
                        open.items.by_name("core:honey_bowl"),
                    )
                {
                    change(&mut world, &mut cache, &open, &mut net, pos, empty);
                    take(&mut net, client);
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::GiveItems(NetStack {
                            item: honey.0,
                            count: 1,
                            wear: 0,
                        }),
                    );
                    sound(&mut net, "buzz");
                }
            }
            // ---- Cake: a slice each time (if you're hungry). ----
            StageKind::Cake => {
                let hungry = p.needs.food < crate::needs::MAX_FOOD;
                if survival && !hungry {
                    continue;
                }
                if survival && crate::needs::applies(&p, &open) {
                    p.needs.food = (p.needs.food + 2.0).min(crate::needs::MAX_FOOD);
                }
                let new = reg.advanced(b, 1).unwrap_or(BlockId::AIR);
                change(&mut world, &mut cache, &open, &mut net, pos, new);
                sound(&mut net, "munch");
            }
            // ---- Composter: plant scraps in, bone meal out. ----
            StageKind::Composter => {
                if stage.stage >= stage.max {
                    if let Some(meal) = open.items.by_name("core:bone_meal") {
                        crate::survival::spawn_item(
                            &mut entities,
                            pos.as_vec3() + Vec3::new(0.5, 1.1, 0.5),
                            ItemStack::new(meal, 1),
                        );
                    }
                    if let Some(empty) = reg.at_stage(&stage.family, 0) {
                        change(&mut world, &mut cache, &open, &mut net, pos, empty);
                    }
                    sound(&mut net, "pop");
                    continue;
                }
                let Some(odds) = compost_chance(&held_name) else {
                    continue;
                };
                take(&mut net, client);
                if chance(&mut seed, odds)
                    && let Some(fuller) = reg.advanced(b, 1)
                {
                    change(&mut world, &mut cache, &open, &mut net, pos, fuller);
                    sparkle(&mut net, pos);
                }
                sound(&mut net, "place");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composters_take_plants_only() {
        assert!(compost_chance("core:wheat_seeds").is_some());
        assert_eq!(compost_chance("core:pumpkin"), Some(1));
        assert_eq!(compost_chance("core:stone"), None);
    }
}

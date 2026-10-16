//! Tools and adventure on the server (milestone 20): fishing, shooting
//! arrows at targets, cooking on a campfire, and telling everyone what
//! armour players wear.

use bevy::prelude::*;
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::item::ItemId;
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};
use ourcraft_mod_api::{EffectKind, EntityMotion, Hit};
use renet::ClientId;

use crate::entities::{Entities, ServerEntity};
use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{OpenWorld, Registry, ServerWorld};

/// How fast an arrow leaves the bow (blocks per second).
const ARROW_SPEED: f32 = 28.0;
/// Seconds food takes on a campfire.
const COOK_TIME: f32 = 6.0;

pub struct AdventurePlugin;

impl Plugin for AdventurePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fishing>()
            .init_resource::<Cooking>()
            .add_systems(
                Update,
                (use_items, fish_bite, arrow_hits, cook, show_armour)
                    .chain()
                    .after(crate::players::PlayersSet),
            );
    }
}

/// Someone fishing: their bobber, and when a fish bites.
struct Line {
    client: ClientId,
    bobber: u64,
    bite_in: f32,
}

#[derive(Resource, Default)]
struct Fishing(Vec<Line>);

/// Food on campfires: (where, what it becomes, seconds left).
#[derive(Resource, Default)]
struct Cooking(Vec<(IVec3, ItemId, f32)>);

fn next(seed: &mut u32) -> u32 {
    if *seed == 0 {
        *seed = 0x1234_5679;
    }
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

#[allow(clippy::too_many_arguments)]
fn use_items(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut entities: ResMut<Entities>,
    mut fishing: ResMut<Fishing>,
    mut cooking: ResMut<Cooking>,
    mut net: ResMut<Network>,
    mut seed: Local<u32>,
) {
    for FromClient { client, msg } in messages.read() {
        let Some(p) = players_map.0.get(client).and_then(|e| players.get(*e).ok()) else {
            continue;
        };
        match msg {
            // ---- The bow: an arrow flies where you aim. ----
            ClientMessage::UseItem { held, direction } => {
                if open.items.name(ItemId(*held)) != "core:bow" {
                    continue;
                }
                let dir = Vec3::from_array(*direction).normalize_or_zero();
                if dir == Vec3::ZERO {
                    continue;
                }
                let eye = p.position() + Vec3::Y * ourcraft_common::physics::EYE_HEIGHT;
                entities.spawn(ServerEntity {
                    model: "@arrow".into(),
                    position: eye + dir * 0.6,
                    velocity: dir * ARROW_SPEED,
                    motion: EntityMotion::Projectile {
                        gravity: 16.0,
                        radius: 0.1,
                        hits_players: false,
                    },
                    lifetime: 20.0,
                    age: 0.0,
                    owner: Some(*client),
                    moved: true,
                    item: None,
                    yaw: dir.x.atan2(dir.z),
                });
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::Sound {
                        name: "click".into(),
                        at: Some(eye.to_array()),
                    },
                );
            }
            ClientMessage::UseBlock { pos, held } => {
                let pos = IVec3::from_array(*pos);
                let near = p.position().distance(pos.as_vec3()) < crate::players::MAX_REACH + 2.0;
                let Some(b) = world.get_block(pos.x, pos.y, pos.z).filter(|_| near) else {
                    continue;
                };
                let held = ItemId(*held);
                let name = open.items.name(held);
                // ---- Fishing: cast (or reel in). ----
                if name == "core:fishing_rod" {
                    if let Some(i) = fishing.0.iter().position(|l| l.client == *client) {
                        let line = fishing.0.remove(i);
                        entities.remove(line.bobber);
                        continue;
                    }
                    let water = registry
                        .0
                        .fluid(b)
                        .is_some_and(|f| f.family == "core:water");
                    if !water {
                        continue;
                    }
                    let bobber = entities.spawn(ServerEntity {
                        model: "@bobber".into(),
                        position: pos.as_vec3() + Vec3::new(0.5, 0.95, 0.5),
                        velocity: Vec3::ZERO,
                        motion: EntityMotion::Fixed,
                        lifetime: 0.0,
                        age: 0.0,
                        owner: Some(*client),
                        moved: true,
                        item: None,
                        yaw: 0.0,
                    });
                    fishing.0.push(Line {
                        client: *client,
                        bobber,
                        bite_in: 4.0 + (next(&mut seed) % 800) as f32 / 100.0,
                    });
                    continue;
                }
                // ---- A campfire: put food on to cook. ----
                if registry.0.get(b).name == "core:campfire"
                    && let Some(cooked) =
                        ourcraft_common::adventure::campfire_result(&open.items, name)
                            .and_then(|n| open.items.by_name(n))
                {
                    cooking.0.push((pos, cooked, COOK_TIME));
                    if !p.creative {
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
                    net.broadcast(
                        Channel::Reliable,
                        &ServerMessage::Sound {
                            name: "place".into(),
                            at: Some((pos.as_vec3() + Vec3::splat(0.5)).to_array()),
                        },
                    );
                }
            }
            _ => {}
        }
    }
}

/// A fish bites: it's caught straight away (no reflexes needed).
fn fish_bite(
    time: Res<Time>,
    players_map: Res<Players>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut entities: ResMut<Entities>,
    mut fishing: ResMut<Fishing>,
    mut net: ResMut<Network>,
    mut seed: Local<u32>,
) {
    let dt = time.delta_secs();
    let mut done = Vec::new();
    for (i, line) in fishing.0.iter_mut().enumerate() {
        let player = players_map
            .0
            .get(&line.client)
            .and_then(|e| players.get(*e).ok());
        let bobber = entities.list.get(&line.bobber).map(|e| e.position);
        let (Some(p), Some(at)) = (player, bobber) else {
            done.push((i, None));
            continue;
        };
        // Walked away: the line snaps.
        if p.position().distance(at) > 24.0 {
            done.push((i, None));
            continue;
        }
        line.bite_in -= dt;
        if line.bite_in <= 0.0 {
            // Mostly fish; now and then something else.
            let catch = match next(&mut seed) % 100 {
                0..85 => "core:fish",
                85..92 => "core:string",
                92..97 => "core:feather",
                _ => "core:saddle",
            };
            done.push((
                i,
                open.items
                    .by_name(catch)
                    .map(|item| (line.client, item, at)),
            ));
        }
    }
    for (i, catch) in done.into_iter().rev() {
        let line = fishing.0.remove(i);
        entities.remove(line.bobber);
        if let Some((client, item, at)) = catch {
            net.send(
                client,
                Channel::Reliable,
                &ServerMessage::GiveItems(NetStack {
                    item: item.0,
                    count: 1,
                    wear: 0,
                }),
            );
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Sound {
                    name: "splat".into(),
                    at: Some(at.to_array()),
                },
            );
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Effect {
                    kind: EffectKind::Splat,
                    position: at.to_array(),
                    color: [90, 140, 230, 255],
                },
            );
        }
    }
}

/// Arrows that landed: targets cheer, and the arrow can be picked up again.
fn arrow_hits(
    mut entities: ResMut<Entities>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut net: ResMut<Network>,
) {
    for (arrow, hit) in std::mem::take(&mut entities.core_hits) {
        if arrow.model != "@arrow" {
            continue;
        }
        let Hit::Block { pos, normal } = hit else {
            continue;
        };
        let pos = IVec3::from_array(pos);
        let target = world
            .get_block(pos.x, pos.y, pos.z)
            .is_some_and(|b| registry.0.get(b).name == "core:target");
        if target {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Effect {
                    kind: EffectKind::Confetti,
                    position: arrow.position.to_array(),
                    color: [255, 255, 255, 255],
                },
            );
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Sound {
                    name: "cheer".into(),
                    at: Some(arrow.position.to_array()),
                },
            );
        }
        // Survival archers get their arrow back, in front of what it hit.
        let survival = players
            .iter()
            .any(|p| Some(p.client) == arrow.owner && !p.creative);
        if survival && let Some(item) = open.items.by_name("core:arrow") {
            let at = pos.as_vec3() + Vec3::splat(0.5) + IVec3::from_array(normal).as_vec3() * 0.7;
            crate::survival::spawn_item(&mut entities, at, ItemStack::new(item, 1));
        }
    }
}

/// Food on campfires cooks, then pops off.
fn cook(time: Res<Time>, mut entities: ResMut<Entities>, mut cooking: ResMut<Cooking>) {
    let dt = time.delta_secs();
    let mut done = Vec::new();
    for (i, (pos, item, left)) in cooking.0.iter_mut().enumerate() {
        *left -= dt;
        if *left <= 0.0 {
            done.push((i, *pos, *item));
        }
    }
    for (i, pos, item) in done.into_iter().rev() {
        cooking.0.remove(i);
        // The food pops out (even if the campfire went meanwhile).
        crate::survival::spawn_item(
            &mut entities,
            pos.as_vec3() + Vec3::new(0.5, 1.1, 0.5),
            ItemStack::new(item, 1),
        );
    }
}

/// Tell everyone what each player wears, whenever it changes (and
/// everything again when someone joins).
fn show_armour(
    mut players: Query<&mut ConnectedPlayer>,
    mut net: ResMut<Network>,
    mut known: Local<std::collections::HashSet<ClientId>>,
) {
    let here: std::collections::HashSet<ClientId> = players
        .iter()
        .filter(|p| p.name.is_some())
        .map(|p| p.client)
        .collect();
    if here.iter().any(|c| !known.contains(c)) {
        for mut p in &mut players {
            p.armour_sent = None;
        }
    }
    *known = here;
    for mut p in &mut players {
        if p.name.is_none() {
            continue;
        }
        let worn = p.inventory.armour.map(|s| s.map_or(0, |s| s.item.0));
        if p.armour_sent == Some(worn) {
            continue;
        }
        p.armour_sent = Some(worn);
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Armour {
                player: p.client,
                pieces: worn,
            },
        );
    }
}

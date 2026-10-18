//! Cloud portals on the server: lighting a sunstone frame with a feather,
//! standing in a portal to travel, coming out on the other side, and
//! drifting gently home if you fall off the Cloud Isles.
//!
//! **Travelling** takes a moment: the player is held still while the land
//! where they're going loads (see [`Travel`]). Then, for a portal trip, we
//! look for a portal near the same x and z on the other side, and build a
//! small one if there isn't one (so the way back is always there). The
//! player comes out standing in it, and it won't take them back until
//! they've stepped out and in again.
//!
//! **Falling** out of the bottom of the Cloud Isles doesn't hurt: you
//! float down and land on the ground at home, at the same x and z.
//!
//! Moving the player is all this module does; `dimensions` notices they're
//! somewhere else and tells their game.

use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::cloud_isles::ISLAND_LEVEL;
use ourcraft_common::dimension::Dimension;
use ourcraft_common::item::ItemId;
use ourcraft_common::portal::{self, PORTAL_SECS, Part};
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};
use ourcraft_common::world::{ColumnPos, WORLD_HEIGHT, WORLD_HEIGHT_CHUNKS};
use ourcraft_mod_api::EffectKind;

use crate::mods::AppliedEdits;
use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{ColumnCache, OpenWorld, Registry, ServerWorld, set_block};

/// Columns loaded round the arrival point before anyone comes out there
/// (1 = the 3 × 3 around it).
pub const ARRIVAL_COLUMNS: i32 = 1;
/// How far below the Cloud Isles' bottom you fall before floating home.
const VOID_DEPTH: f32 = 24.0;
/// Give up on a trip if the land there doesn't load in this long.
const TRAVEL_TIMEOUT: f32 = 20.0;
/// How far round the arrival point (blocks) to look for islands to put a
/// new portal on.
const LAND_SEARCH: i32 = 12;

pub struct PortalsPlugin;

impl Plugin for PortalsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                light_portals,
                check_portals.before(crate::survival::drop_broken_blocks),
                travel,
            )
                .chain()
                .after(crate::players::PlayersSet)
                .before(crate::world::WorldSet),
        );
    }
}

/// A trip to another dimension, waiting for the land there to load.
#[derive(Debug, Clone)]
pub struct Travel {
    pub to: Dimension,
    /// Where to come out (the same x and z as where they left).
    pub x: i32,
    pub z: i32,
    /// Through a portal (come out in one), or falling (land on the ground).
    pub by_portal: bool,
    waited: f32,
}

impl Travel {
    pub fn column(&self) -> ColumnPos {
        ColumnPos::from_block(self.x, self.z)
    }
}

/// The blocks portals are made of.
struct PortalBlocks {
    frame: BlockId,
    portal: BlockId,
    cloud: BlockId,
}

impl PortalBlocks {
    fn get(registry: &Registry) -> Option<Self> {
        let r = &registry.0;
        Some(Self {
            frame: r.by_name("core:sunstone")?,
            portal: r.by_name("core:cloud_portal")?,
            cloud: r.by_name("core:cloud")?,
        })
    }
}

fn block(world: &ServerWorld, p: IVec3) -> BlockId {
    world.get_block(p.x, p.y, p.z).unwrap_or_default()
}

/// Change a block and tell everyone who can see it.
fn put(
    world: &mut ServerWorld,
    cache: &mut ColumnCache,
    open: &OpenWorld,
    net: &mut Network,
    pos: IVec3,
    b: BlockId,
) {
    if set_block(world, cache, open, pos, b).is_some_and(|old| old != b) {
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::BlockChanged {
                pos: pos.to_array(),
                block: b.0,
            },
        );
    }
}

/// Touching a sunstone frame with a feather fills it with portal.
#[allow(clippy::too_many_arguments)]
fn light_portals(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
) {
    let Some(blocks) = PortalBlocks::get(&registry) else {
        return;
    };
    for FromClient { client, msg } in messages.read() {
        let ClientMessage::UseBlock { pos, held } = msg else {
            continue;
        };
        if open.items.name(ItemId(*held)) != "core:feather" {
            continue;
        }
        let Some(p) = players_map.0.get(client).and_then(|e| players.get(*e).ok()) else {
            continue;
        };
        let pos = IVec3::from_array(*pos);
        let has_feather = p.creative || p.inventory.count(ItemId(*held)) > 0;
        if p.position().distance(pos.as_vec3()) > crate::players::MAX_REACH + 1.0
            || block(&world, pos) != blocks.frame
            || !has_feather
        {
            continue;
        }
        let shape = portal::light_from(
            pos,
            |q| block(&world, q) == blocks.frame,
            |q| {
                let b = block(&world, q);
                b.is_air() || b == blocks.portal
            },
        );
        let Some(shape) = shape else {
            net.send(
                *client,
                Channel::Reliable,
                &ServerMessage::Chat {
                    from: None,
                    text: "A portal needs sunstone all the way round a space at least 2 wide and 3 tall."
                        .into(),
                },
            );
            continue;
        };
        let cells: Vec<IVec3> = shape.cells().collect();
        for c in &cells {
            put(&mut world, &mut cache, &open, &mut net, *c, blocks.portal);
        }
        // A feather is used up (not in creative).
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
        let middle =
            cells.iter().map(|c| c.as_vec3()).sum::<Vec3>() / cells.len() as f32 + Vec3::splat(0.5);
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Effect {
                kind: EffectKind::Confetti,
                position: middle.to_array(),
                color: [190, 225, 255, 255],
            },
        );
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Sound {
                name: "shimmer".into(),
                at: Some(middle.to_array()),
            },
        );
    }
}

/// A portal goes out when its frame is broken (or a portal block is).
#[allow(clippy::too_many_arguments)]
fn check_portals(
    applied: Res<AppliedEdits>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
) {
    let Some(blocks) = PortalBlocks::get(&registry) else {
        return;
    };
    for edit in &applied.0 {
        if edit.new == blocks.frame || edit.new == blocks.portal {
            continue;
        }
        for d in [
            IVec3::ZERO,
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
        ] {
            let start = edit.pos + d;
            if block(&world, start) != blocks.portal {
                continue;
            }
            let whole = [portal::Axis::X, portal::Axis::Z].into_iter().any(|axis| {
                portal::find_inside(
                    start,
                    axis,
                    |q| block(&world, q) == blocks.frame,
                    |q| block(&world, q) == blocks.portal,
                )
                .is_some()
            });
            if whole {
                continue;
            }
            // Put out every portal block joined to this one.
            let mut todo = vec![start];
            let mut seen = vec![start];
            while let Some(q) = todo.pop() {
                put(&mut world, &mut cache, &open, &mut net, q, BlockId::AIR);
                for s in [
                    IVec3::X,
                    IVec3::NEG_X,
                    IVec3::Y,
                    IVec3::NEG_Y,
                    IVec3::Z,
                    IVec3::NEG_Z,
                ] {
                    let n = q + s;
                    let limit = (portal::MAX_INSIDE * portal::MAX_INSIDE) as usize;
                    if !seen.contains(&n) && seen.len() < limit && block(&world, n) == blocks.portal
                    {
                        seen.push(n);
                        todo.push(n);
                    }
                }
            }
        }
    }
}

/// Standing in portals, falling out of the sky, and arriving.
#[allow(clippy::too_many_arguments)]
fn travel(
    time: Res<Time>,
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
) {
    let Some(blocks) = PortalBlocks::get(&registry) else {
        return;
    };
    let dt = time.delta_secs();
    for mut p in &mut players {
        if p.name.is_none() {
            continue;
        }
        let p = &mut *p;
        // On the way: come out once the land there has loaded.
        if let Some(mut t) = p.travel.take() {
            t.waited += dt;
            let ready = around(t.column(), ARRIVAL_COLUMNS).all(|c| world.has_column_in(t.to, c));
            if !ready {
                if t.waited < TRAVEL_TIMEOUT {
                    p.travel = Some(t);
                } else {
                    warn!("The land in {} didn't load in time", t.to.display_name());
                }
                continue;
            }
            let feet = if t.by_portal {
                arrive_by_portal(
                    &mut world, &mut cache, &open, &registry, &mut net, &blocks, &t,
                )
            } else {
                landing(&world, &registry, &t)
            };
            p.physics.position = feet;
            p.physics.velocity = Vec3::ZERO;
            p.state.position = feet.to_array();
            p.state.velocity = [0.0; 3];
            p.portal_blocked = t.by_portal;
            p.portal_time = 0.0;
            if !t.by_portal {
                net.send(
                    p.client,
                    Channel::Reliable,
                    &ServerMessage::Chat {
                        from: None,
                        text: "You fell through the clouds and floated gently down to the ground."
                            .into(),
                    },
                );
            }
            continue;
        }

        let feet = p.physics.position;
        let dim = Dimension::of_y(feet.y);
        let (x, z) = (feet.x.floor() as i32, feet.z.floor() as i32);
        // Fell off the Cloud Isles: float home.
        if dim == Dimension::CloudIsles && feet.y < dim.base_y() as f32 - VOID_DEPTH {
            p.travel = Some(Travel {
                to: Dimension::Overworld,
                x,
                z,
                by_portal: false,
                waited: 0.0,
            });
            continue;
        }
        let inside = [0.2, 1.2].iter().any(|dy| {
            let b = (feet + Vec3::Y * *dy).floor().as_ivec3();
            block(&world, b) == blocks.portal
        });
        if !inside {
            p.portal_time = 0.0;
            p.portal_blocked = false;
            continue;
        }
        let busy = p.boat.is_some() || p.cart.is_some() || p.riding.is_some() || p.sleeping;
        if p.portal_blocked || busy {
            continue;
        }
        let before = p.portal_time;
        p.portal_time += dt;
        // Sparkles while it gathers you up.
        if (before / 0.5).floor() != (p.portal_time / 0.5).floor() {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Effect {
                    kind: EffectKind::Poof,
                    position: (feet + Vec3::Y).to_array(),
                    color: [200, 230, 255, 255],
                },
            );
        }
        if p.portal_time >= PORTAL_SECS {
            p.portal_time = 0.0;
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Sound {
                    name: "whoosh".into(),
                    at: Some((feet + Vec3::Y).to_array()),
                },
            );
            p.travel = Some(Travel {
                to: dim.other(),
                x,
                z,
                by_portal: true,
                waited: 0.0,
            });
        }
    }
}

/// Columns within `r` of `center`.
fn around(center: ColumnPos, r: i32) -> impl Iterator<Item = ColumnPos> {
    (-r..=r)
        .flat_map(move |dz| (-r..=r).map(move |dx| ColumnPos::new(center.x + dx, center.z + dz)))
}

/// The highest block at `x, z` in a dimension you could stand on (or swim
/// in), as a server-space height. Plants and leaves don't count.
fn ground(world: &ServerWorld, registry: &Registry, dim: Dimension, x: i32, z: i32) -> Option<i32> {
    let r = &registry.0;
    (dim.base_y()..dim.base_y() + WORLD_HEIGHT)
        .rev()
        .find(|&y| {
            let b = block(world, IVec3::new(x, y, z));
            r.is_liquid(b) || (r.is_solid(b) && r.is_opaque(b) && !r.get(b).name.ends_with("log"))
        })
}

/// Where someone who fell out of the sky lands.
fn landing(world: &ServerWorld, registry: &Registry, t: &Travel) -> Vec3 {
    let y = ground(world, registry, t.to, t.x, t.z).unwrap_or(t.to.base_y() + 64);
    Vec3::new(t.x as f32 + 0.5, y as f32 + 1.0, t.z as f32 + 0.5)
}

/// Find a portal near the arrival point, or build one. Returns where the
/// traveller stands (feet).
fn arrive_by_portal(
    world: &mut ServerWorld,
    cache: &mut ColumnCache,
    open: &OpenWorld,
    registry: &Registry,
    net: &mut Network,
    blocks: &PortalBlocks,
    t: &Travel,
) -> Vec3 {
    if let Some(found) = nearest_portal(world, blocks.portal, t) {
        return found.as_vec3() + Vec3::new(0.5, 0.0, 0.5);
    }
    let (base, floating) = portal_site(world, registry, t);
    let r = &registry.0;
    let floor = match t.to {
        Dimension::Overworld => ourcraft_common::block::blocks::COBBLESTONE,
        Dimension::CloudIsles => blocks.cloud,
    };
    // Over nothing at all: a little cloud to stand on first.
    if floating {
        for dz in -2..=2 {
            for dx in -1..=4 {
                let q = base + IVec3::new(dx, -1, dz);
                if !r.is_solid(block(world, q)) {
                    put(world, cache, open, net, q, blocks.cloud);
                }
            }
        }
    }
    for (q, part) in portal::structure(base) {
        let here = block(world, q);
        let new = match part {
            Part::Frame => blocks.frame,
            Part::Portal => blocks.portal,
            Part::Air if here == blocks.frame || here == blocks.portal => continue,
            Part::Air => BlockId::AIR,
            Part::Floor if r.is_solid(here) => continue,
            Part::Floor => floor,
        };
        put(world, cache, open, net, q, new);
    }
    portal::arrival(base)
}

/// The bottom block of the portal nearest the arrival point, if there's
/// one in the columns round it.
fn nearest_portal(world: &ServerWorld, portal_block: BlockId, t: &Travel) -> Option<IVec3> {
    let mut best: Option<(i32, IVec3)> = None;
    let base = t.to.base_chunk_y();
    for col in around(t.column(), ARRIVAL_COLUMNS) {
        for cy in 0..WORLD_HEIGHT_CHUNKS {
            let cpos = col.chunk(base + cy);
            let Some(chunk) = world.chunk(cpos) else {
                continue;
            };
            if !chunk.palette().contains(&portal_block) {
                continue;
            }
            let origin = IVec3::new(cpos.x, cpos.y, cpos.z) * 16;
            for ((x, y, z), b) in chunk.iter() {
                if b != portal_block {
                    continue;
                }
                let p = origin + IVec3::new(x as i32, y as i32, z as i32);
                let d = (p.x - t.x).pow(2) + (p.z - t.z).pow(2);
                if best.is_none_or(|(bd, bp)| d < bd || (d == bd && p.y < bp.y)) {
                    best = Some((d, p));
                }
            }
        }
    }
    // Stand on the bottom row of it.
    let mut p = best?.1;
    while block(world, p - IVec3::Y) == portal_block {
        p -= IVec3::Y;
    }
    Some(p)
}

/// Where to build a portal for someone arriving: the bottom-left frame
/// block, and whether it's out over empty sky.
fn portal_site(world: &ServerWorld, registry: &Registry, t: &Travel) -> (IVec3, bool) {
    let dim = t.to;
    let top_y = dim.base_y() + WORLD_HEIGHT - 6;
    // Nearest land, searching outwards (in the overworld, right here).
    let reach = match dim {
        Dimension::Overworld => 0,
        Dimension::CloudIsles => LAND_SEARCH,
    };
    for r in 0..=reach {
        for i in -r..=r {
            for (dx, dz) in [(i, -r), (i, r), (-r, i), (r, i)] {
                let (x, z) = (t.x + dx, t.z + dz);
                if let Some(y) = ground(world, registry, dim, x, z) {
                    // The arrival spot (one block in from the frame's
                    // corner) is right over this land.
                    return (IVec3::new(x - 1, (y + 1).min(top_y), z), false);
                }
            }
        }
    }
    let y = dim.base_y()
        + match dim {
            Dimension::CloudIsles => ISLAND_LEVEL,
            Dimension::Overworld => ourcraft_common::worldgen::SEA_LEVEL + 1,
        };
    (IVec3::new(t.x - 1, y, t.z), true)
}

//! Entities that mods create: projectiles, flags and other props.
//!
//! Projectiles fly with gravity and stop at the first solid block or player
//! they touch; the owning mod then gets an `Event::ProjectileHit`. Clients
//! are told when entities appear and disappear; they move projectiles
//! themselves in between (same formula), so they fly smoothly without a
//! stream of position updates.

use std::collections::HashMap;

use bevy::prelude::*;
use ourcraft_common::physics::Aabb;
use ourcraft_common::protocol::{Channel, ServerMessage};
use ourcraft_mod_api::{EntityMotion, Event, Hit};

use crate::mods::ModQueue;
use crate::net::Network;
use crate::players::ConnectedPlayer;
use crate::world::{Registry, ServerWorld};

pub struct EntitiesPlugin;

impl Plugin for EntitiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Entities>()
            .add_systems(Update, (simulate_entities, announce_entities).chain());
    }
}

#[derive(Debug, Clone)]
pub struct ServerEntity {
    pub model: String,
    pub position: Vec3,
    pub velocity: Vec3,
    pub motion: EntityMotion,
    /// Seconds before it disappears (0 = never).
    pub lifetime: f32,
    pub age: f32,
    pub owner: Option<u64>,
    /// Moved by a mod since the last update was sent.
    pub moved: bool,
    /// For dropped items: what you get by walking over it.
    pub item: Option<ourcraft_common::inventory::ItemStack>,
}

#[derive(Resource, Default)]
pub struct Entities {
    next_id: u64,
    pub list: HashMap<u64, ServerEntity>,
    /// Not yet announced to clients.
    pub spawned: Vec<u64>,
    pub removed: Vec<u64>,
}

impl Entities {
    pub fn spawn(&mut self, e: ServerEntity) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.list.insert(id, e);
        self.spawned.push(id);
        id
    }

    pub fn remove(&mut self, id: u64) {
        if self.list.remove(&id).is_some() {
            self.spawned.retain(|s| *s != id);
            self.removed.push(id);
        }
    }

    /// The message that makes a client show this entity.
    pub fn spawn_message(id: u64, e: &ServerEntity) -> ServerMessage {
        ServerMessage::EntitySpawned {
            id,
            model: e.model.clone(),
            position: e.position.to_array(),
            velocity: e.velocity.to_array(),
            gravity: match e.motion {
                EntityMotion::Projectile { gravity, .. } => Some(gravity),
                EntityMotion::Fixed => None,
            },
        }
    }
}

/// Small steps so fast projectiles can't skip through thin walls.
const SUBSTEPS: usize = 4;

fn simulate_entities(
    time: Res<Time>,
    mut entities: ResMut<Entities>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    players: Query<&ConnectedPlayer>,
    mut queue: ResMut<ModQueue>,
) {
    let dt = time.delta_secs();
    let mut done: Vec<(u64, Option<Hit>)> = Vec::new();
    for (&id, e) in entities.list.iter_mut() {
        e.age += dt;
        if e.lifetime > 0.0 && e.age >= e.lifetime {
            done.push((id, None));
            continue;
        }
        let EntityMotion::Projectile {
            gravity,
            radius,
            hits_players,
        } = e.motion
        else {
            continue;
        };
        let h = dt / SUBSTEPS as f32;
        'steps: for _ in 0..SUBSTEPS {
            let before = e.position;
            e.velocity.y -= gravity * h;
            e.position += e.velocity * h;
            // Blocks: is the projectile's centre inside a solid block?
            let b = e.position.floor().as_ivec3();
            let solid = world
                .get_block(b.x, b.y, b.z)
                .is_some_and(|id| registry.0.is_solid(id));
            if solid {
                // The face we came through: the axis where we crossed into
                // this block.
                let prev = before.floor().as_ivec3();
                let d = b - prev;
                let normal = if d.x != 0 {
                    [-d.x.signum(), 0, 0]
                } else if d.y != 0 {
                    [0, -d.y.signum(), 0]
                } else {
                    [0, 0, -d.z.signum()]
                };
                done.push((
                    id,
                    Some(Hit::Block {
                        pos: b.to_array(),
                        normal,
                    }),
                ));
                break 'steps;
            }
            if hits_players {
                for p in players.iter().filter(|p| p.name.is_some()) {
                    if Some(p.client) == e.owner {
                        continue;
                    }
                    let aabb = Aabb::player(p.physics.position).inflate(radius);
                    let inside =
                        e.position.cmpge(aabb.min).all() && e.position.cmple(aabb.max).all();
                    if inside {
                        done.push((
                            id,
                            Some(Hit::Player {
                                id: p.client,
                                at: e.position.to_array(),
                            }),
                        ));
                        break 'steps;
                    }
                }
            }
        }
        // Fell out of the world.
        if e.position.y < -32.0 {
            done.push((id, None));
        }
    }
    for (id, hit) in done {
        let owner = entities.list.get(&id).and_then(|e| e.owner);
        entities.remove(id);
        if let Some(hit) = hit {
            queue.events.push(Event::ProjectileHit {
                entity: id,
                owner,
                hit,
            });
        }
    }
}

fn announce_entities(
    mut entities: ResMut<Entities>,
    mut net: ResMut<Network>,
    mut timer: Local<u32>,
) {
    let entities = &mut *entities;
    for id in std::mem::take(&mut entities.spawned) {
        if let Some(e) = entities.list.get(&id) {
            net.broadcast(Channel::Reliable, &Entities::spawn_message(id, e));
        }
    }
    for id in std::mem::take(&mut entities.removed) {
        net.broadcast(Channel::Reliable, &ServerMessage::EntityRemoved(id));
    }
    // Positions of entities mods moved (e.g. a carried flag), 20×/s.
    *timer += 1;
    if (*timer).is_multiple_of(3) {
        let moved: Vec<(u64, [f32; 3])> = entities
            .list
            .iter_mut()
            .filter(|(_, e)| e.moved)
            .map(|(id, e)| {
                e.moved = false;
                (*id, e.position.to_array())
            })
            .collect();
        if !moved.is_empty() {
            net.broadcast(Channel::Unreliable, &ServerMessage::EntitiesMoved(moved));
        }
    }
}

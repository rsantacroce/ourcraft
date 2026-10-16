//! Boats (milestone 19): put one on the water (right click with a boat),
//! right click it to get in, "down" (Shift) to get out, and hit it to pick
//! it up again. Whoever sits in a boat floats and rows quickly (the shared
//! physics does that); the boat entity just follows them.

use std::collections::HashMap;

use bevy::prelude::*;
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::item::ItemId;
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};
use ourcraft_mod_api::EntityMotion;

use crate::entities::{Entities, ServerEntity};
use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{OpenWorld, Registry, ServerWorld};

pub struct BoatsPlugin;

impl Plugin for BoatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Boats>().add_systems(
            Update,
            (use_boats, follow_riders)
                .chain()
                .after(crate::players::PlayersSet),
        );
    }
}

/// Every boat in the world: entity id → the boat item it came from.
#[derive(Resource, Default)]
pub struct Boats(pub HashMap<u64, String>);

/// The model of a boat of a wood: `@boat:birch`.
fn model(item: &str) -> String {
    let wood = item.trim_start_matches("core:").trim_end_matches("_boat");
    format!("@boat:{wood}")
}

#[allow(clippy::too_many_arguments)]
fn use_boats(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut entities: ResMut<Entities>,
    mut boats: ResMut<Boats>,
    mut net: ResMut<Network>,
) {
    for FromClient { client, msg } in messages.read() {
        let Some(mut p) = players_map
            .0
            .get(client)
            .and_then(|e| players.get_mut(*e).ok())
        else {
            continue;
        };
        let tell = |net: &mut Network, boat: Option<u64>| {
            net.send(*client, Channel::Reliable, &ServerMessage::Boating { boat });
        };
        match msg {
            // ---- Putting a boat down (on water, or on the ground). ----
            ClientMessage::UseBlock { pos, held } => {
                let name = open.items.name(ItemId(*held)).to_owned();
                if !name.ends_with("_boat") {
                    continue;
                }
                let pos = IVec3::from_array(*pos);
                let near = p.position().distance(pos.as_vec3()) < crate::players::MAX_REACH;
                let Some(b) = world.get_block(pos.x, pos.y, pos.z).filter(|_| near) else {
                    continue;
                };
                // On water it floats at the top of the block; on anything
                // else it sits on top.
                let y = if registry.0.is_liquid(b) {
                    pos.y as f32 + 0.9
                } else {
                    pos.y as f32 + 1.0
                };
                let at = Vec3::new(pos.x as f32 + 0.5, y, pos.z as f32 + 0.5);
                let id = entities.spawn(ServerEntity {
                    model: model(&name),
                    position: at,
                    velocity: Vec3::ZERO,
                    motion: EntityMotion::Fixed,
                    lifetime: 0.0,
                    age: 0.0,
                    owner: None,
                    moved: true,
                    item: None,
                    // Its long side across the way you face.
                    yaw: p.state.yaw + std::f32::consts::FRAC_PI_2,
                });
                boats.0.insert(id, name);
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
            ClientMessage::UseEntity { id, .. } => {
                let Some(e) = entities.list.get(id).filter(|_| boats.0.contains_key(id)) else {
                    continue;
                };
                let taken = players_map
                    .0
                    .values()
                    .filter_map(|e| players.get(*e).ok())
                    .any(|o| o.boat == Some(*id));
                let Some(mut p) = players_map
                    .0
                    .get(client)
                    .and_then(|e| players.get_mut(*e).ok())
                else {
                    continue;
                };
                if taken || p.position().distance(e.position) > crate::players::MAX_REACH {
                    continue;
                }
                p.boat = Some(*id);
                // Sit down in it (the player's game is corrected to here).
                p.physics.position = e.position;
                p.physics.velocity = Vec3::ZERO;
                p.state.position = e.position.to_array();
                tell(&mut net, Some(*id));
            }
            // ---- Getting out. ----
            ClientMessage::LeaveBoat => {
                if p.boat.take().is_some() {
                    p.physics.position.y += 0.6;
                    tell(&mut net, None);
                }
                // Out of a mine cart, or off a pig, too.
                if p.cart.take().is_some() {
                    p.physics.position.y += 0.6;
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::InCart { cart: None },
                    );
                }
                if p.riding.take().is_some() {
                    p.physics.position.y += 0.4;
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::Riding { mount: None },
                    );
                }
            }
            // ---- Picking a boat up (an empty one). ----
            ClientMessage::HitEntity { id, .. } => {
                let ridden = players_map
                    .0
                    .values()
                    .filter_map(|e| players.get(*e).ok())
                    .any(|o| o.boat == Some(*id));
                let Some(e) = entities.list.get(id).cloned() else {
                    continue;
                };
                if ridden || !boats.0.contains_key(id) {
                    continue;
                }
                let Some(name) = boats.0.remove(id) else {
                    continue;
                };
                entities.remove(*id);
                let creative = players_map
                    .0
                    .get(client)
                    .and_then(|e| players.get(*e).ok())
                    .is_some_and(|p| p.creative);
                if !creative && let Some(item) = open.items.by_name(&name) {
                    crate::survival::spawn_item(
                        &mut entities,
                        e.position + Vec3::Y * 0.5,
                        ItemStack::new(item, 1),
                    );
                }
            }
            _ => {}
        }
    }
}

/// Boats go where their riders go (and riders whose boat is gone stand
/// up).
fn follow_riders(
    mut players: Query<&mut ConnectedPlayer>,
    mut entities: ResMut<Entities>,
    boats: Res<Boats>,
    mut net: ResMut<Network>,
) {
    for mut p in &mut players {
        let Some(id) = p.boat else { continue };
        let Some(e) = entities
            .list
            .get_mut(&id)
            .filter(|_| boats.0.contains_key(&id))
        else {
            p.boat = None;
            net.send(
                p.client,
                Channel::Reliable,
                &ServerMessage::Boating { boat: None },
            );
            continue;
        };
        let (at, yaw) = (
            p.physics.position,
            p.state.yaw + std::f32::consts::FRAC_PI_2,
        );
        if e.position != at || e.yaw != yaw {
            e.position = at;
            e.yaw = yaw;
            e.moved = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boat_models_name_their_wood() {
        assert_eq!(model("core:birch_boat"), "@boat:birch");
        assert_eq!(model("core:oak_boat"), "@boat:oak");
    }
}

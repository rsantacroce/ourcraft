//! Decoration on the server (milestone 21): planting in flower pots, and
//! putting things in item frames (or taking them out again).

use bevy::prelude::*;
use ourcraft_common::inventory::ItemStack;
use ourcraft_common::item::ItemId;
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};

use crate::entities::Entities;
use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{ColumnCache, OpenWorld, Registry, ServerWorld};

pub struct DecorationPlugin;

impl Plugin for DecorationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, use_decoration.after(crate::players::PlayersSet));
    }
}

#[allow(clippy::too_many_arguments)]
fn use_decoration(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    players: Query<&ConnectedPlayer>,
    mut open: ResMut<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut entities: ResMut<Entities>,
    mut net: ResMut<Network>,
) {
    for FromClient { client, msg } in messages.read() {
        let ClientMessage::UseBlock { pos, held } = msg else {
            continue;
        };
        let Some(p) = players_map.0.get(client).and_then(|e| players.get(*e).ok()) else {
            continue;
        };
        let pos = IVec3::from_array(*pos);
        if p.position().distance(pos.as_vec3()) > crate::players::MAX_REACH + 1.0 {
            continue;
        }
        let Some(b) = world.get_block(pos.x, pos.y, pos.z) else {
            continue;
        };
        let held = ItemId(*held);
        let held_name = open.items.name(held).to_owned();
        let block_name = registry.0.get(b).name.clone();
        let take = |net: &mut Network| {
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
        };
        // ---- Planting in an empty flower pot. ----
        if block_name == "core:flower_pot"
            && ourcraft_common::decoration::pottable().contains(&held_name)
            && let Some(potted) = registry
                .0
                .by_name(&ourcraft_common::decoration::potted(&held_name))
        {
            crate::building::change(&mut world, &mut cache, &open, &mut net, pos, potted);
            take(&mut net);
            continue;
        }
        // ---- Item frames: put something in, or take it out. ----
        if registry
            .0
            .get(b)
            .variant
            .as_ref()
            .is_some_and(|v| v.family == "core:item_frame")
        {
            let centre = pos.as_vec3() + Vec3::splat(0.5);
            if let Some(old) = open.frames.remove(&pos) {
                crate::survival::spawn_item(&mut entities, centre, ItemStack::new(old, 1));
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::FrameItem {
                        pos: pos.to_array(),
                        item: 0,
                    },
                );
            } else if held != ItemId::NONE {
                open.frames.insert(pos, held);
                take(&mut net);
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::FrameItem {
                        pos: pos.to_array(),
                        item: held.0,
                    },
                );
            }
        }
    }
}

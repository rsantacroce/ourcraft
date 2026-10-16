//! Our own magic on the server (milestone 24): wishing on a wish table to
//! make a tool or armour sparkly, and drinking potions.

use bevy::prelude::*;
use ourcraft_common::item::{ItemId, ItemKind};
use ourcraft_common::magic::{self, Effect};
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};
use ourcraft_mod_api::EffectKind;
use renet::ClientId;

use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{OpenWorld, Registry, ServerWorld};

pub struct MagicPlugin;

impl Plugin for MagicPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (wish, drink, wear_off)
                .chain()
                .after(crate::players::PlayersSet),
        );
    }
}

fn give(net: &mut Network, client: ClientId, item: ItemId) {
    net.send(
        client,
        Channel::Reliable,
        &ServerMessage::GiveItems(NetStack {
            item: item.0,
            count: 1,
            wear: 0,
        }),
    );
}

fn take(net: &mut Network, client: ClientId, item: ItemId) {
    net.send(
        client,
        Channel::Reliable,
        &ServerMessage::TakeItems(NetStack {
            item: item.0,
            count: 1,
            wear: 0,
        }),
    );
}

/// Using a wish table with a tool or armour in hand (and a wish star in
/// the bag) swaps it for a sparkly one.
fn wish(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    world: Res<ServerWorld>,
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
        let table = world
            .get_block(pos.x, pos.y, pos.z)
            .is_some_and(|b| registry.0.get(b).name == "core:wish_table");
        if !table {
            continue;
        }
        let held = ItemId(*held);
        let Some(def) = open.items.get(held) else {
            continue;
        };
        let wishable = matches!(def.kind, ItemKind::Tool { .. } | ItemKind::Armour { .. });
        let shiny = open.items.by_name(&magic::sparkly(&def.name));
        let star = open.items.by_name("core:wish_star");
        let (Some(shiny), Some(star)) = (shiny.filter(|_| wishable && !def.sparkly), star) else {
            continue;
        };
        // Creative players wish for free; everyone else spends a star.
        if !p.creative {
            if p.inventory.count(star) == 0 || p.inventory.count(held) == 0 {
                net.send(
                    *client,
                    Channel::Reliable,
                    &ServerMessage::Chat {
                        from: None,
                        text: "You need a wish star to make a wish.".into(),
                    },
                );
                continue;
            }
            take(&mut net, *client, star);
            take(&mut net, *client, held);
        }
        give(&mut net, *client, shiny);
        let at = (pos.as_vec3() + Vec3::new(0.5, 1.2, 0.5)).to_array();
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Effect {
                kind: EffectKind::Confetti,
                position: at,
                color: [200, 140, 255, 255],
            },
        );
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Sound {
                name: "cheer".into(),
                at: Some(at),
            },
        );
    }
}

/// Drinking a potion (the same "eat" as food): its effect starts, or
/// starts over, and the bottle comes back.
fn drink(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut net: ResMut<Network>,
) {
    for FromClient { client, msg } in messages.read() {
        let ClientMessage::Eat { item } = msg else {
            continue;
        };
        let Some(mut p) = players_map
            .0
            .get(client)
            .and_then(|e| players.get_mut(*e).ok())
        else {
            continue;
        };
        let item = ItemId(*item);
        let Some((effect, seconds)) = magic::potion_effect(open.items.name(item)) else {
            continue;
        };
        if p.name.is_none() || p.inventory.count(item) == 0 {
            continue;
        }
        p.effects[effect as usize] = seconds;
        // Keep our copy in step (so a second drink right away is refused).
        if let Some(slot) = p
            .inventory
            .slots
            .iter_mut()
            .find(|s| s.is_some_and(|s| s.item == item))
        {
            *slot = None;
        }
        take(&mut net, *client, item);
        if let Some(bottle) = open.items.by_name("core:bottle") {
            give(&mut net, *client, bottle);
        }
        let seconds = p.effects;
        net.send(
            *client,
            Channel::Reliable,
            &ServerMessage::Effects { seconds },
        );
        net.send(
            *client,
            Channel::Reliable,
            &ServerMessage::Sound {
                name: "munch".into(),
                at: None,
            },
        );
    }
}

/// Count effects down, keep swimmers' breath full, and say when one
/// runs out.
fn wear_off(time: Res<Time>, mut players: Query<&mut ConnectedPlayer>, mut net: ResMut<Network>) {
    let dt = time.delta_secs();
    for mut p in &mut players {
        if p.effects == [0.0; 3] {
            continue;
        }
        let before = Effect::bits(&p.effects);
        for s in &mut p.effects {
            *s = (*s - dt).max(0.0);
        }
        if p.effects[Effect::Swim as usize] > 0.0 {
            p.needs.air = crate::needs::MAX_AIR;
        }
        if Effect::bits(&p.effects) != before {
            let (client, seconds) = (p.client, p.effects);
            net.send(
                client,
                Channel::Reliable,
                &ServerMessage::Effects { seconds },
            );
        }
    }
}

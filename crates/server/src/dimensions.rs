//! Players in different dimensions.
//!
//! The server keeps every dimension stacked in one world (see
//! [`ourcraft_common::dimension`]), but each player's game only knows its
//! own, at the usual heights. Two things make that work:
//!
//! - **Translation at the door.** Every message out to a player goes
//!   through [`localize`]: anything about a place (a block, an animal,
//!   another player, a sound) is moved to that player's own heights, or
//!   left out if it's in another dimension. That's also why players in
//!   different dimensions never see each other. Messages in from a player
//!   go through [`globalize`], the other way.
//! - **Noticing a move.** A player's dimension is just a question of how
//!   high they are, so however they got somewhere else (a portal, falling
//!   off an island, fainting and waking up in bed, a mod), the
//!   [`follow_dimension_changes`] system notices, tells their game to start
//!   afresh, and the column streaming sends the new dimension's land.

use std::borrow::Cow;

use bevy::prelude::*;
use ourcraft_common::dimension::Dimension;
use ourcraft_common::protocol::{Channel, ClientMessage, NetPlayerState, ServerMessage};

use crate::entities::Entities;
use crate::net::Network;
use crate::players::ConnectedPlayer;

/// A player's state at their own dimension's heights.
fn local_state(state: &NetPlayerState, dim: Dimension) -> NetPlayerState {
    NetPlayerState {
        position: dim.to_local(state.position),
        ..*state
    }
}

/// `msg` as the player in `dim` should hear it: positions moved to their
/// dimension's own heights, or `None` if it's all about somewhere else.
pub fn localize(msg: &ServerMessage, dim: Dimension) -> Option<Cow<'_, ServerMessage>> {
    use ServerMessage as M;
    let here = |p: &[f32; 3]| Dimension::of_y(p[1]) == dim;
    let here_block = |p: &[i32; 3]| Dimension::of_block_y(p[1]) == Some(dim);
    let at = |p: &[f32; 3]| dim.to_local(*p);
    let at_block = |p: &[i32; 3]| dim.block_to_local(*p);
    // Somewhere in this dimension, moved to its heights.
    let place = |p: &[i32; 3], make: &dyn Fn([i32; 3]) -> M| {
        here_block(p).then(|| Cow::Owned(make(at_block(p))))
    };
    Some(match msg {
        M::Welcome { state, .. } => {
            let mut m = msg.clone();
            if let M::Welcome {
                state: s,
                dimension,
                ..
            } = &mut m
            {
                *s = local_state(state, dim);
                *dimension = dim;
            }
            Cow::Owned(m)
        }
        M::ChangedDimension { state, .. } => Cow::Owned(M::ChangedDimension {
            dimension: dim,
            state: local_state(state, dim),
        }),
        M::YourState { ack_tick, state } => Cow::Owned(M::YourState {
            ack_tick: *ack_tick,
            state: local_state(state, dim),
        }),
        M::PlayerStates { tick, players } => Cow::Owned(M::PlayerStates {
            tick: *tick,
            players: players
                .iter()
                .filter(|(_, s)| here(&s.position))
                .map(|(id, s)| (*id, local_state(s, dim)))
                .collect(),
        }),
        M::BlockChanged { pos, block } => {
            return place(pos, &|pos| M::BlockChanged { pos, block: *block });
        }
        M::SignText { pos, lines } => {
            return place(pos, &|pos| M::SignText {
                pos,
                lines: lines.clone(),
            });
        }
        M::BannerPattern { pos, layers } => {
            return place(pos, &|pos| M::BannerPattern {
                pos,
                layers: layers.clone(),
            });
        }
        M::FrameItem { pos, item } => {
            return place(pos, &|pos| M::FrameItem { pos, item: *item });
        }
        M::Home { pos } => return place(pos, &|pos| M::Home { pos }),
        M::Sleeping { bed: Some(bed) } => {
            return place(bed, &|pos| M::Sleeping { bed: Some(pos) });
        }
        M::Container(c) if here_block(&c.pos) => {
            let mut c = c.clone();
            c.pos = at_block(&c.pos);
            Cow::Owned(M::Container(c))
        }
        M::Container(_) => return None,
        M::EntitySpawned {
            id,
            model,
            position,
            velocity,
            gravity,
            yaw,
        } => {
            if !here(position) {
                return None;
            }
            Cow::Owned(M::EntitySpawned {
                id: *id,
                model: model.clone(),
                position: at(position),
                velocity: *velocity,
                gravity: *gravity,
                yaw: *yaw,
            })
        }
        M::EntitiesMoved(list) => {
            let list: Vec<_> = list
                .iter()
                .filter(|(_, p, _)| here(p))
                .map(|(id, p, yaw)| (*id, at(p), *yaw))
                .collect();
            if list.is_empty() {
                return None;
            }
            Cow::Owned(M::EntitiesMoved(list))
        }
        M::Effect {
            kind,
            position,
            color,
        } => {
            if !here(position) {
                return None;
            }
            Cow::Owned(M::Effect {
                kind: *kind,
                position: at(position),
                color: *color,
            })
        }
        M::Sound { name, at: Some(p) } => {
            if !here(p) {
                return None;
            }
            Cow::Owned(M::Sound {
                name: name.clone(),
                at: Some(at(p)),
            })
        }
        // Everything else isn't about a place (chat, time, your inventory…).
        _ => Cow::Borrowed(msg),
    })
}

/// Does this message from a player name a place?
pub fn has_place(msg: &ClientMessage) -> bool {
    matches!(
        msg,
        ClientMessage::SetBlock { .. }
            | ClientMessage::OpenContainer { .. }
            | ClientMessage::UseBlock { .. }
            | ClientMessage::SetSign { .. }
    )
}

/// A message from the player in `dim`, with the places it names moved to
/// the server's stacked heights.
pub fn globalize(mut msg: ClientMessage, dim: Dimension) -> ClientMessage {
    match &mut msg {
        ClientMessage::SetBlock { pos, .. }
        | ClientMessage::OpenContainer { pos }
        | ClientMessage::UseBlock { pos, .. }
        | ClientMessage::SetSign { pos, .. } => *pos = dim.block_to_server(*pos),
        _ => {}
    }
    msg
}

/// Notice players who are now in another dimension, and start them afresh
/// there. Runs just before the columns are streamed.
pub fn follow_dimension_changes(
    mut players: Query<&mut ConnectedPlayer>,
    entities: Res<Entities>,
    mut net: ResMut<Network>,
) {
    for mut p in &mut players {
        let dim = Dimension::of_y(p.state.position[1]);
        if p.name.is_none() || dim == p.dimension {
            continue;
        }
        info!(
            "{} went to {}",
            p.name.as_deref().unwrap_or("?"),
            dim.display_name()
        );
        p.dimension = dim;
        // Their game forgets the old dimension, so send everything again.
        p.sent.clear();
        if p.open_container.take().is_some() {
            net.send(p.client, Channel::Reliable, &ServerMessage::ContainerClosed);
        }
        net.set_dimension(p.client, dim);
        net.send(
            p.client,
            Channel::Reliable,
            &ServerMessage::ChangedDimension {
                dimension: dim,
                state: p.state,
            },
        );
        // What's about over there (only that dimension's get through).
        for (id, e) in &entities.list {
            net.send(
                p.client,
                Channel::Reliable,
                &Entities::spawn_message(*id, e),
            );
        }
        let title = match dim {
            Dimension::CloudIsles => "Welcome to the Cloud Isles!",
            Dimension::Overworld => "Home again!",
        };
        net.send(
            p.client,
            Channel::Reliable,
            &ServerMessage::Title {
                text: title.into(),
                seconds: 3.0,
            },
        );
    }
}

/// Things that fall out of the bottom of a dimension (an item dropped off
/// an island, say) are gone for good.
pub fn remove_fallen(mut entities: ResMut<Entities>) {
    let fallen: Vec<u64> = entities
        .list
        .iter()
        .filter(|(_, e)| {
            let (low, _) = Dimension::of_y(e.position.y).player_range();
            e.position.y < low
        })
        .map(|(id, _)| *id)
        .collect();
    for id in fallen {
        entities.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKY: Dimension = Dimension::CloudIsles;

    #[test]
    fn places_move_to_the_players_heights() {
        let msg = ServerMessage::BlockChanged {
            pos: [5, SKY.base_y() + 70, -2],
            block: 3,
        };
        assert_eq!(
            localize(&msg, SKY).unwrap().into_owned(),
            ServerMessage::BlockChanged {
                pos: [5, 70, -2],
                block: 3
            }
        );
        // Nobody in the overworld hears about it.
        assert!(localize(&msg, Dimension::Overworld).is_none());
        // And the overworld's own blocks stay where they are.
        let home = ServerMessage::BlockChanged {
            pos: [5, 70, -2],
            block: 3,
        };
        assert_eq!(*localize(&home, Dimension::Overworld).unwrap(), home);
        assert!(localize(&home, SKY).is_none());
    }

    #[test]
    fn players_only_see_their_own_dimension() {
        let at = |y: f32| NetPlayerState {
            position: [1.0, y, 2.0],
            ..Default::default()
        };
        let msg = ServerMessage::PlayerStates {
            tick: 1,
            players: vec![(1, at(70.0)), (2, at(SKY.base_y() as f32 + 90.0))],
        };
        let ServerMessage::PlayerStates { players, .. } = localize(&msg, SKY).unwrap().into_owned()
        else {
            panic!()
        };
        assert_eq!(players, vec![(2, at(90.0))]);
        let moved = ServerMessage::EntitiesMoved(vec![(7, [0.0, 64.0, 0.0], 0.0)]);
        assert!(localize(&moved, SKY).is_none());
        // Chat is for everyone.
        let chat = ServerMessage::Chat {
            from: None,
            text: "hi".into(),
        };
        assert!(matches!(localize(&chat, SKY), Some(Cow::Borrowed(_))));
        // Your own position always comes, at your heights.
        let me = ServerMessage::YourState {
            ack_tick: 3,
            state: at(SKY.base_y() as f32 + 10.0),
        };
        assert_eq!(
            localize(&me, SKY).unwrap().into_owned(),
            ServerMessage::YourState {
                ack_tick: 3,
                state: at(10.0)
            }
        );
    }

    #[test]
    fn messages_in_come_back_up() {
        let msg = ClientMessage::UseBlock {
            pos: [1, 64, 1],
            held: 9,
        };
        assert!(has_place(&msg));
        assert_eq!(
            globalize(msg.clone(), SKY),
            ClientMessage::UseBlock {
                pos: [1, 64 + SKY.base_y(), 1],
                held: 9
            }
        );
        assert_eq!(globalize(msg.clone(), Dimension::Overworld), msg);
        assert!(!has_place(&ClientMessage::Chat { text: "x".into() }));
    }
}

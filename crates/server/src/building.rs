//! Using blocks: opening doors and trapdoors, writing on signs, and
//! sleeping in beds (which skips the night once everyone is in bed, and
//! sets where you'll wake up).

use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::protocol::{Channel, ClientMessage, ServerMessage, sanitize_sign};
use ourcraft_common::shapes::VariantKind;

use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{ColumnCache, OpenWorld, Registry, ServerWorld, set_block};

/// How far away blocks can be used from.
const USE_REACH: f32 = 8.0;
/// Time of day when sleeping ends (a little after sunrise).
const MORNING: f32 = 0.27;

pub struct BuildingPlugin;

impl Plugin for BuildingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (use_blocks, wake_up)
                .chain()
                .after(crate::players::PlayersSet),
        );
    }
}

/// Night is when the sun is down: before 6:00 and after 18:00.
pub fn is_night(time_of_day: f32) -> bool {
    !(0.25..0.75).contains(&time_of_day)
}

/// Change a block and tell everyone.
fn change(
    world: &mut ServerWorld,
    cache: &mut ColumnCache,
    open: &OpenWorld,
    net: &mut Network,
    pos: IVec3,
    block: BlockId,
) {
    if set_block(world, cache, open, pos, block).is_some() {
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::BlockChanged {
                pos: pos.to_array(),
                block: block.0,
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn use_blocks(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    mut players: Query<&mut ConnectedPlayer>,
    mut open: ResMut<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
) {
    for FromClient { client, msg } in messages.read() {
        let Some(&entity) = players_map.0.get(client) else {
            continue;
        };
        let (pos, sign_lines) = match msg {
            ClientMessage::LeaveBed => {
                if let Ok(mut p) = players.get_mut(entity)
                    && p.sleeping
                {
                    p.sleeping = false;
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::Sleeping { bed: None },
                    );
                }
                continue;
            }
            ClientMessage::UseBlock { pos } => (IVec3::from_array(*pos), None),
            ClientMessage::SetSign { pos, lines } => (IVec3::from_array(*pos), Some(lines)),
            _ => continue,
        };
        let Ok(mut p) = players.get_mut(entity) else {
            continue;
        };
        let Some(name) = p.name.clone() else { continue };
        if p.position().distance(pos.as_vec3() + Vec3::splat(0.5)) > USE_REACH || !p.rules.can_build
        {
            continue;
        }
        let Some(block) = world.get_block(pos.x, pos.y, pos.z) else {
            continue;
        };
        let kind = registry.0.variant_kind(block);

        // Writing on a sign.
        if let Some(lines) = sign_lines {
            if !matches!(kind, Some(VariantKind::Sign | VariantKind::WallSign)) {
                continue;
            }
            let lines = sanitize_sign(lines);
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::SignText {
                    pos: pos.to_array(),
                    lines: lines.clone(),
                },
            );
            open.signs.insert(pos, lines);
            continue;
        }

        match kind {
            // Doors, trapdoors and fence gates open and close.
            Some(VariantKind::Door | VariantKind::Trapdoor | VariantKind::FenceGate) => {
                let Some(changes) = registry.0.toggled(block) else {
                    continue;
                };
                for (offset, new) in changes {
                    let at = pos + IVec3::from_array(offset);
                    // Only the door's own other half (not whatever's there).
                    let ok = world
                        .get_block(at.x, at.y, at.z)
                        .is_some_and(|b| registry.0.variant_kind(b) == kind);
                    if ok {
                        change(&mut world, &mut cache, &open, &mut net, at, new);
                    }
                }
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::Sound {
                        name: "place".into(),
                        at: Some((pos.as_vec3() + Vec3::splat(0.5)).to_array()),
                    },
                );
            }
            // Beds: this is where you'll wake up; at night, sleep.
            Some(VariantKind::Bed) => {
                open.beds.insert(name, pos);
                let text = if is_night(open.time_of_day()) {
                    p.sleeping = true;
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::Sleeping {
                            bed: Some(pos.to_array()),
                        },
                    );
                    "Good night... Zzz (the night ends when everyone is in bed)"
                } else {
                    "You'll wake up here. You can only sleep at night."
                };
                net.send(
                    *client,
                    Channel::Reliable,
                    &ServerMessage::Chat {
                        from: None,
                        text: text.into(),
                    },
                );
            }
            _ => {}
        }
    }
}

/// When everyone online is asleep, skip to the morning.
fn wake_up(
    mut players: Query<&mut ConnectedPlayer>,
    mut open: ResMut<OpenWorld>,
    mut net: ResMut<Network>,
) {
    let named = || players.iter().filter(|p| p.name.is_some());
    let any = named().any(|p| p.sleeping);
    if !any {
        return;
    }
    let all = named().all(|p| p.sleeping);
    let night = is_night(open.time_of_day());
    if all && night {
        open.meta.time_of_day = MORNING;
        net.broadcast(Channel::Reliable, &ServerMessage::TimeOfDay(MORNING));
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Chat {
                from: None,
                text: "Good morning!".into(),
            },
        );
    }
    if all || !night {
        for mut p in &mut players {
            if p.sleeping {
                p.sleeping = false;
                net.send(
                    p.client,
                    Channel::Reliable,
                    &ServerMessage::Sleeping { bed: None },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_and_day() {
        assert!(is_night(0.0));
        assert!(is_night(0.9));
        assert!(!is_night(0.5));
        assert!(!is_night(MORNING));
    }
}

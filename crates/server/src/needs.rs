//! Survival needs: health, hunger and breath.
//!
//! Gentle on purpose (this is a game for kids):
//! - Falls hurt only from high up, water breaks any fall, and lava stings.
//! - Hunger grows slowly; a full tummy heals you. Hunger alone never makes
//!   you faint, it stops at one heart.
//! - At zero health you *faint*: you wake up in your bed (or at the world
//!   spawn) with everything you were carrying.
//! - `/peaceful` turns hunger and monsters off for the whole world.
//!
//! Only for survival players in worlds without a game mode (game modes
//! like Splat Squad have their own health).

use bevy::prelude::*;
use ourcraft_common::item::ItemId;
use ourcraft_common::physics::EYE_HEIGHT;
use ourcraft_common::protocol::{Channel, ClientMessage, NetStack, ServerMessage};
use ourcraft_mod_api::EffectKind;

use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{OpenWorld, Registry, ServerWorld};

pub const MAX_HEALTH: f32 = 20.0;
pub const MAX_FOOD: f32 = 20.0;
/// Seconds of breath under water.
pub const MAX_AIR: f32 = 12.0;
/// Falls shorter than this don't hurt.
const SAFE_FALL: f32 = 4.0;

pub struct NeedsPlugin;

impl Plugin for NeedsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (eat, update_needs, faint, send_vitals)
                .chain()
                .after(crate::players::PlayersSet),
        );
    }
}

/// A player's survival state.
#[derive(Debug, Clone)]
pub struct Needs {
    pub health: f32,
    pub food: f32,
    pub air: f32,
    /// How tired: every whole point costs a point of food.
    exhaustion: f32,
    /// Highest point of the current fall.
    fall_from: Option<f32>,
    /// Seconds until the next tick of healing, starving, drowning…
    regen: f32,
    starve: f32,
    drown: f32,
    burn: f32,
    /// What the client was last told (rounded).
    sent: Option<(i32, i32, i32)>,
}

impl Default for Needs {
    fn default() -> Self {
        Self {
            health: MAX_HEALTH,
            food: MAX_FOOD,
            air: MAX_AIR,
            exhaustion: 0.0,
            fall_from: None,
            regen: 0.0,
            starve: 0.0,
            drown: 0.0,
            burn: 0.0,
            sent: None,
        }
    }
}

impl Needs {
    pub fn hurt(&mut self, amount: f32) {
        self.health = (self.health - amount).max(0.0);
    }
}

/// Do needs apply to this player in this world right now?
pub fn applies(p: &ConnectedPlayer, open: &OpenWorld) -> bool {
    p.name.is_some() && !p.creative && open.meta.game_mode.is_none()
}

fn eat(
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
        if !applies(&p, &open) {
            continue;
        }
        let item = ItemId(*item);
        let Some(def) = open.items.get(item).filter(|d| d.food > 0 || d.heals > 0) else {
            continue;
        };
        let hungry = p.needs.food < MAX_FOOD || def.heals > 0;
        if !hungry || p.inventory.count(item) == 0 {
            continue;
        }
        p.needs.food = (p.needs.food + def.food as f32).min(MAX_FOOD);
        p.needs.health = (p.needs.health + def.heals as f32).min(MAX_HEALTH);
        // Keep our copy in step and take it from the player's game.
        if let Some(slot) = p
            .inventory
            .slots
            .iter_mut()
            .find(|s| s.is_some_and(|s| s.item == item))
        {
            let s = slot.as_mut().expect("found");
            s.count -= 1;
            if s.count == 0 {
                *slot = None;
            }
        }
        net.send(
            *client,
            Channel::Reliable,
            &ServerMessage::TakeItems(NetStack {
                item: item.0,
                count: 1,
                wear: 0,
            }),
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

fn update_needs(
    time: Res<Time>,
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
) {
    let dt = time.delta_secs();
    let peaceful = open.peaceful;
    for mut p in &mut players {
        if !applies(&p, &open) {
            p.needs = Needs::default();
            continue;
        }
        let p = &mut *p;
        let pos = p.physics.position;
        let block_at = |v: Vec3| {
            let b = v.floor().as_ivec3();
            world.get_block(b.x, b.y, b.z).unwrap_or_default()
        };
        let in_fluid = |v: Vec3, family: &str| {
            registry
                .0
                .fluid(block_at(v))
                .is_some_and(|f| f.family == family)
        };
        let needs = &mut p.needs;

        // Falling: remember the top, and on landing hurt by the distance.
        let soft = p.physics.flying || in_fluid(pos + Vec3::Y * 0.2, "core:water");
        if soft {
            needs.fall_from = None;
        } else if p.physics.on_ground {
            if let Some(top) = needs.fall_from.take() {
                let fall = top - pos.y;
                if fall > SAFE_FALL {
                    needs.hurt(((fall - SAFE_FALL) * 0.75).floor().max(1.0));
                }
            }
        } else if p.physics.velocity.y < 0.0 {
            needs.fall_from = Some(needs.fall_from.map_or(pos.y, |f| f.max(pos.y)));
        } else {
            needs.fall_from = None;
        }

        // Breath under water; half a heart a second once it runs out.
        if in_fluid(pos + Vec3::Y * EYE_HEIGHT, "core:water") {
            needs.air = (needs.air - dt).max(0.0);
            if needs.air <= 0.0 {
                needs.drown -= dt;
                if needs.drown <= 0.0 {
                    needs.drown = 1.0;
                    needs.hurt(1.0);
                }
            }
        } else {
            needs.air = (needs.air + 4.0 * dt).min(MAX_AIR);
            needs.drown = 0.0;
        }

        // Lava stings (but you can climb out).
        if in_fluid(pos + Vec3::Y * 0.3, "core:lava") {
            needs.burn -= dt;
            if needs.burn <= 0.0 {
                needs.burn = 0.5;
                needs.hurt(1.5);
            }
        } else {
            needs.burn = 0.0;
        }

        // Hunger: slowly, faster when running about.
        if !peaceful {
            let moving = p.physics.velocity.with_y(0.0).length();
            needs.exhaustion += dt * (0.02 + moving * 0.012);
            if needs.exhaustion >= 1.0 {
                needs.exhaustion -= 1.0;
                needs.food = (needs.food - 1.0).max(0.0);
            }
        } else {
            needs.food = MAX_FOOD;
        }

        // Healing on a full tummy (always, in peaceful).
        needs.regen -= dt;
        let can_heal = peaceful || needs.food >= 16.0;
        if needs.health < MAX_HEALTH && can_heal && needs.regen <= 0.0 {
            needs.regen = if peaceful { 1.0 } else { 3.0 };
            needs.health = (needs.health + 1.0).min(MAX_HEALTH);
            needs.exhaustion += 0.5;
        }
        // An empty tummy hurts a little, but never below one heart.
        if needs.food <= 0.0 {
            needs.starve -= dt;
            if needs.starve <= 0.0 {
                needs.starve = 5.0;
                if needs.health > 2.0 {
                    needs.hurt(1.0);
                }
            }
        }
    }
}

/// At zero health: wake up in bed (or at the world spawn), unhurt.
fn faint(
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
    mut net: ResMut<Network>,
) {
    for mut p in &mut players {
        if !applies(&p, &open) || p.needs.health > 0.0 {
            continue;
        }
        let name = p.name.clone().unwrap_or_default();
        let was_at = p.physics.position;
        // The bed still has to be there.
        let bed = open.beds.get(&name).copied().filter(|b| {
            world.get_block(b.x, b.y, b.z).is_some_and(|id| {
                registry.0.variant_kind(id) == Some(ourcraft_common::shapes::VariantKind::Bed)
            })
        });
        let wake = match bed {
            Some(b) => b.as_vec3() + Vec3::new(0.5, 0.6, 0.5),
            None => open.spawn.as_vec3() + Vec3::new(0.5, 1.0, 0.5),
        };
        p.physics.position = wake;
        p.physics.velocity = Vec3::ZERO;
        p.state.position = wake.to_array();
        let food = p.needs.food.max(10.0);
        p.needs = Needs::default();
        p.needs.food = food;
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Effect {
                kind: EffectKind::Poof,
                position: (was_at + Vec3::Y).to_array(),
                color: [255, 255, 255, 255],
            },
        );
        net.send(
            p.client,
            Channel::Reliable,
            &ServerMessage::Title {
                text: if bed.is_some() {
                    "Oops! You fainted... and woke up in bed".into()
                } else {
                    "Oops! You fainted... and woke up at the start".into()
                },
                seconds: 3.0,
            },
        );
    }
}

fn send_vitals(
    mut players: Query<&mut ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut net: ResMut<Network>,
) {
    for mut p in &mut players {
        if p.name.is_none() {
            continue;
        }
        let on = applies(&p, &open);
        let n = &p.needs;
        let now = if on {
            (
                n.health.ceil() as i32,
                n.food.ceil() as i32,
                n.air.ceil() as i32,
            )
        } else {
            (0, 0, 0)
        };
        if p.needs.sent == Some(now) {
            continue;
        }
        p.needs.sent = Some(now);
        net.send(
            p.client,
            Channel::Reliable,
            &ServerMessage::Vitals {
                health: now.0 as f32,
                food: now.1 as f32,
                air: now.2 as f32,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hurting_stops_at_zero() {
        let mut n = Needs::default();
        n.hurt(5.0);
        assert_eq!(n.health, 15.0);
        n.hurt(100.0);
        assert_eq!(n.health, 0.0);
    }
}

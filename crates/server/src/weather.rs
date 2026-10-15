//! Weather: now and then it rains (or snows, where it's cold: the players'
//! games decide what falls). `/weather rain` and `/weather clear` change
//! it on the spot.

use bevy::prelude::*;
use ourcraft_common::protocol::{Channel, ServerMessage};

use crate::net::Network;
use crate::world::OpenWorld;

/// How often everyone is reminded what the weather is (so players who just
/// joined know too).
const ANNOUNCE_EVERY: f32 = 5.0;

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Weather {
            raining: false,
            // The first rain comes after a while.
            change_in: 400.0,
        })
        .add_systems(Update, update_weather.after(crate::players::PlayersSet));
    }
}

#[derive(Resource, Debug)]
pub struct Weather {
    pub raining: bool,
    /// Seconds until it changes on its own.
    pub change_in: f32,
}

impl Weather {
    /// Make it rain (or stop), and pick how long until it changes again:
    /// rain lasts a couple of minutes, dry spells longer.
    pub fn set(&mut self, raining: bool, luck: u32) {
        self.raining = raining;
        let spread = (luck % 1000) as f32 / 1000.0;
        self.change_in = if raining {
            90.0 + spread * 120.0
        } else {
            360.0 + spread * 480.0
        };
    }
}

fn update_weather(
    time: Res<Time>,
    mut weather: ResMut<Weather>,
    open: Res<OpenWorld>,
    mut net: ResMut<Network>,
    mut announce: Local<f32>,
    mut luck: Local<u32>,
) {
    let dt = time.delta_secs();
    *luck = luck.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    // Game modes (Splat Squad) keep the sky clear.
    if open.meta.game_mode.is_some() {
        if weather.raining {
            weather.set(false, *luck);
        }
    } else {
        weather.change_in -= dt;
        if weather.change_in <= 0.0 {
            let raining = !weather.raining;
            weather.set(raining, *luck);
        }
    }
    *announce -= dt;
    if weather.is_changed() || *announce <= 0.0 {
        *announce = ANNOUNCE_EVERY;
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::Weather {
                raining: weather.raining,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rain_is_shorter_than_dry_spells() {
        let mut w = Weather {
            raining: false,
            change_in: 0.0,
        };
        w.set(true, 999);
        let longest_rain = w.change_in;
        w.set(false, 0);
        assert!(w.change_in > longest_rain);
    }
}

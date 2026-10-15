//! Sound effects, synthesized in code when the game starts: no audio files
//! to ship, and every sound is original.
//!
//! Each sound is a short mix of tones and noise with an envelope (how loud
//! it is over time), written as a WAV file in memory and handed to Bevy.
//!
//! Sounds come from local actions (placing and breaking blocks, menu clicks)
//! and from the server (`ServerMessage::Sound`, which mods use).

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::Arc;

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;
use ourcraft_common::protocol::ServerMessage;

use crate::net::{FromServer, NetReceiveSet};
use crate::player::PlayerCamera;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlaySound>()
            .add_message::<PlayAmbient>()
            .add_systems(Startup, create_sounds)
            .add_systems(PreUpdate, sounds_from_server.after(NetReceiveSet))
            .add_systems(
                Update,
                (
                    sounds_from_edits,
                    play_sounds,
                    play_ambient,
                    rain_loop,
                    music_player,
                )
                    .chain(),
            );
    }
}

/// Ask for a sound, optionally at a position (quieter when far away).
#[derive(Message, Debug, Clone)]
pub struct PlaySound {
    pub name: String,
    pub at: Option<Vec3>,
}

impl PlaySound {
    pub fn ui(name: &str) -> Self {
        Self {
            name: name.into(),
            at: None,
        }
    }
}

/// A background sound (footsteps, birds, wind…) at its own volume.
#[derive(Message, Debug, Clone)]
pub struct PlayAmbient {
    pub name: &'static str,
    /// 0..1, before the player's volume setting.
    pub gain: f32,
}

#[derive(Resource, Default)]
struct Sounds(HashMap<&'static str, Handle<AudioSource>>);

const RATE: u32 = 22_050;

/// A tiny deterministic noise source.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 % 20_000) as f32 / 10_000.0 - 1.0
    }
}

/// Build a sound of `secs` length from a function of time → sample.
fn synth(secs: f32, mut f: impl FnMut(f32) -> f32) -> Vec<f32> {
    let n = (secs * RATE as f32) as usize;
    (0..n).map(|i| f(i as f32 / RATE as f32)).collect()
}

/// Fast attack, exponential decay.
fn env(t: f32, decay: f32) -> f32 {
    let attack = (t / 0.004).min(1.0);
    attack * (-t * decay).exp()
}

fn generate(name: &str) -> Vec<f32> {
    let mut noise = Noise(0x2468_ace1);
    match name {
        // Soft thump: a falling low tone plus a little noise.
        "place" => synth(0.12, |t| {
            let f = 170.0 - 500.0 * t;
            ((TAU * f * t).sin() * 0.8 + noise.next() * 0.15) * env(t, 35.0)
        }),
        // Crunch: noise with a gritty buzz.
        "break" => synth(0.16, |t| {
            let grit = ((TAU * 90.0 * t).sin()).signum() * 0.25;
            (noise.next() * 0.7 + grit) * env(t, 25.0)
        }),
        // Cartoon "pew": a quick rising blip.
        "pop" => synth(0.09, |t| {
            let f = 500.0 + 9000.0 * t;
            (TAU * f * t).sin() * 0.6 * env(t, 30.0)
        }),
        // Squishy splat: low wobble plus wet noise.
        "splat" => synth(0.25, |t| {
            let wobble = (TAU * (120.0 + 40.0 * (TAU * 18.0 * t).sin()) * t).sin();
            (wobble * 0.6 + noise.next() * 0.4) * env(t, 14.0)
        }),
        // Two-note chime going up.
        "grab" => synth(0.3, |t| {
            let f = if t < 0.12 { 660.0 } else { 990.0 };
            let local = if t < 0.12 { t } else { t - 0.12 };
            (TAU * f * t).sin() * 0.5 * env(local, 12.0)
        }),
        // A happy arpeggio (C E G C).
        "cheer" => synth(0.6, |t| {
            let notes = [523.25, 659.25, 783.99, 1046.5];
            let i = ((t / 0.12) as usize).min(3);
            let local = t - i as f32 * 0.12;
            let f = notes[i];
            ((TAU * f * t).sin() + 0.3 * (TAU * 2.0 * f * t).sin()) * 0.4 * env(local, 6.0)
        }),
        // Animals: a buzzy "voice" (a few harmonics) shaped differently.
        // Baa: wobbly and nasal.
        "baa" => synth(0.55, |t| {
            let f = 300.0 + 25.0 * (TAU * 28.0 * t).sin() - 60.0 * t;
            voice(f * t, 7) * 0.45 * soft(t, 0.55)
        }),
        // Moo: low, long and sliding down.
        "moo" => synth(0.9, |t| {
            let f = 125.0 - 30.0 * t;
            voice(f * t, 8) * 0.5 * soft(t, 0.9)
        }),
        // Oink: two quick snorts.
        "oink" => synth(0.36, |t| {
            let local = if t < 0.16 { t } else { t - 0.2 };
            let on = (0.0..0.16).contains(&local);
            let f = 210.0 + 120.0 * local;
            if on {
                (voice(f * t, 5) * 0.5 + noise.next() * 0.15) * soft(local, 0.16)
            } else {
                0.0
            }
        }),
        // Cluck: three little clicks.
        "cluck" => synth(0.4, |t| {
            let local = t % 0.13;
            let f = 700.0 - 1500.0 * local;
            (TAU * f * t).sin() * 0.45 * env(local, 45.0)
        }),
        // Munch: a few crunchy bites.
        "munch" => synth(0.4, |t| {
            let local = t % 0.13;
            (noise.next() * 0.6) * env(local, 30.0) * if t < 0.39 { 1.0 } else { 0.0 }
        }),
        // Bonk: a cartoon knock on the head.
        "bonk" => synth(0.18, |t| {
            let f = 420.0 - 900.0 * t;
            ((TAU * f * t).sin() * 0.7 + noise.next() * 0.1) * env(t, 22.0)
        }),
        // Grumble: a low, grumpy hum.
        "grumble" => synth(0.6, |t| {
            let f = 95.0 + 12.0 * (TAU * 7.0 * t).sin();
            voice(f * t, 9) * 0.5 * soft(t, 0.6)
        }),
        // Boing: a springy rising wobble.
        "boing" => synth(0.35, |t| {
            let f = 180.0 + 500.0 * t + 40.0 * (TAU * 25.0 * t).sin();
            (TAU * f * t).sin() * 0.5 * env(t, 7.0)
        }),
        // ---- Footsteps: short and quiet. ----
        "step_grass" => {
            let mut low = 0.0;
            synth(0.09, |t| {
                low += (noise.next() - low) * 0.15;
                low * 2.2 * env(t, 40.0)
            })
        }
        "step_stone" => synth(0.06, |t| {
            ((TAU * 320.0 * t).sin() * 0.4 + noise.next() * 0.35) * env(t, 70.0)
        }),
        "step_wood" => synth(0.08, |t| {
            ((TAU * 170.0 * t).sin() * 0.6 + noise.next() * 0.15) * env(t, 45.0)
        }),
        "step_sand" => synth(0.11, |t| {
            let grit = (TAU * 60.0 * t).sin().abs();
            noise.next() * 0.45 * grit * env(t, 28.0)
        }),
        "step_snow" => {
            let mut low = 0.0;
            synth(0.12, |t| {
                low += (noise.next() - low) * 0.3;
                low * 1.6 * env(t, 25.0)
            })
        }
        // ---- Ambience ----
        // A little bird: two or three chirps.
        "birds" => synth(0.9, |t| {
            let chirps = [0.0, 0.18, 0.45];
            let mut v = 0.0;
            for (i, start) in chirps.iter().enumerate() {
                let local = t - start;
                if (0.0..0.09).contains(&local) {
                    let f = 2200.0 + 1600.0 * (local / 0.09) + i as f32 * 300.0;
                    v += (TAU * f * local).sin() * (std::f32::consts::PI * local / 0.09).sin();
                }
            }
            v * 0.35
        }),
        // Water: gentle bubbling.
        "water" => {
            let mut bubbles: Vec<(f32, f32)> = Vec::new();
            let mut n = Noise(0x1357_9bdf);
            for _ in 0..9 {
                bubbles.push((
                    (n.next() * 0.5 + 0.5) * 1.4,
                    300.0 + (n.next() * 0.5 + 0.5) * 500.0,
                ));
            }
            synth(1.6, |t| {
                bubbles
                    .iter()
                    .map(|(start, f)| {
                        let local = t - start;
                        if (0.0..0.12).contains(&local) {
                            (TAU * (f + 900.0 * local) * local).sin() * env(local, 30.0)
                        } else {
                            0.0
                        }
                    })
                    .sum::<f32>()
                    * 0.3
            })
        }
        // Wind high up: a soft swell of rushing air.
        "wind" => {
            let mut low = 0.0;
            synth(3.0, |t| {
                low += (noise.next() - low) * 0.04;
                low * 4.0 * soft(t, 3.0)
            })
        }
        // Rain: soft, even hiss (played in a loop).
        "rain" => {
            let mut low = 0.0;
            synth(2.0, |_| {
                low += (noise.next() - low) * 0.35;
                low * 0.9 + noise.next() * 0.05
            })
        }
        "music1" => music(1),
        "music2" => music(2),
        "music3" => music(3),
        // Menu click.
        _ => synth(0.03, |t| (TAU * 1800.0 * t).sin() * 0.4 * env(t, 120.0)),
    }
}

/// A calm little piece: a melody wandering over a pentatonic scale above
/// slow, soft chords. `seed` picks the tune.
fn music(seed: u32) -> Vec<f32> {
    let beat = 0.6;
    let bars = 8;
    let len = beat * 4.0 * bars as f32 + 3.0;
    let mut out = vec![0.0f32; (len * RATE as f32) as usize];
    let add_note = |start: f32, dur: f32, freq: f32, gain: f32, out: &mut Vec<f32>| {
        let s0 = (start * RATE as f32) as usize;
        let n = ((dur + 1.5) * RATE as f32) as usize;
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            let attack = (t / 0.03).min(1.0);
            let release = if t > dur {
                (-(t - dur) * 3.0).exp()
            } else {
                1.0
            };
            let v = ((TAU * freq * t).sin() + 0.25 * (TAU * 2.0 * freq * t).sin())
                * attack
                * release
                * (-t * 1.2).exp()
                * gain;
            if let Some(o) = out.get_mut(s0 + i) {
                *o += v;
            }
        }
    };
    // C major pentatonic, two octaves; chords I – vi – IV – V.
    let scale = [
        261.63, 293.66, 329.63, 392.0, 440.0, 523.25, 587.33, 659.25, 783.99,
    ];
    let chords: [[f32; 3]; 4] = [
        [130.81, 164.81, 196.0],
        [110.0, 130.81, 164.81],
        [87.31, 110.0, 130.81],
        [98.0, 123.47, 146.83],
    ];
    let mut rng = Noise(0x9e37_79b9 ^ seed.wrapping_mul(0x85eb_ca6b));
    let mut note = 4i32;
    for bar in 0..bars {
        let chord = chords[(bar + seed as usize) % 4];
        let t0 = bar as f32 * 4.0 * beat;
        for f in chord {
            add_note(t0, 4.0 * beat, f, 0.06, &mut out);
        }
        let mut b = 0.0;
        while b < 4.0 {
            // Mostly steps, sometimes a little leap; now and then a rest.
            let r = rng.next();
            let step = if r > 0.6 {
                2
            } else if r > 0.0 {
                1
            } else if r > -0.6 {
                -1
            } else {
                -2
            };
            note = (note + step).clamp(0, scale.len() as i32 - 1);
            let dur = if rng.next() > 0.3 { 1.0 } else { 2.0 };
            if rng.next() > -0.75 {
                add_note(
                    t0 + b * beat,
                    dur * beat,
                    scale[note as usize],
                    0.16,
                    &mut out,
                );
            }
            b += dur;
        }
    }
    // Keep it quiet and avoid clipping.
    let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-3);
    for v in &mut out {
        *v = *v / peak * 0.5;
    }
    out
}

/// A buzzy tone (`cycles` = frequency × time) made of `n` harmonics.
fn voice(cycles: f32, n: usize) -> f32 {
    (1..=n)
        .map(|k| (TAU * k as f32 * cycles).sin() / k as f32)
        .sum::<f32>()
        * 0.6
}

/// Fade in and out over a sound of `len` seconds.
fn soft(t: f32, len: f32) -> f32 {
    let x = (t / len).clamp(0.0, 1.0);
    (std::f32::consts::PI * x).sin().powf(0.6)
}

/// Encode samples (-1..1) as a 16-bit mono WAV file.
pub fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

pub const SOUND_NAMES: [&str; 27] = [
    "place",
    "break",
    "pop",
    "splat",
    "grab",
    "cheer",
    "click",
    "baa",
    "moo",
    "oink",
    "cluck",
    "munch",
    "bonk",
    "grumble",
    "boing",
    "step_grass",
    "step_stone",
    "step_wood",
    "step_sand",
    "step_snow",
    "birds",
    "water",
    "wind",
    "rain",
    "music1",
    "music2",
    "music3",
];

fn create_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut sounds = Sounds::default();
    for name in SOUND_NAMES {
        let bytes: Arc<[u8]> = wav_bytes(&generate(name)).into();
        sounds.0.insert(name, sources.add(AudioSource { bytes }));
    }
    commands.insert_resource(sounds);
}

fn sounds_from_server(mut messages: MessageReader<FromServer>, mut out: MessageWriter<PlaySound>) {
    for FromServer(msg) in messages.read() {
        if let ServerMessage::Sound { name, at } = msg {
            out.write(PlaySound {
                name: name.clone(),
                at: at.map(Vec3::from_array),
            });
        }
    }
}

fn sounds_from_edits(
    mut edits: MessageReader<crate::interaction::BlockEditRequest>,
    mut out: MessageWriter<PlaySound>,
) {
    for e in edits.read() {
        let name = if e.block.is_air() { "break" } else { "place" };
        out.write(PlaySound {
            name: name.into(),
            at: Some(e.pos.as_vec3() + Vec3::splat(0.5)),
        });
    }
}

fn play_sounds(
    mut commands: Commands,
    mut requests: MessageReader<PlaySound>,
    sounds: Option<Res<Sounds>>,
    settings: Res<crate::settings::ClientSettings>,
    camera: Option<Single<&GlobalTransform, With<PlayerCamera>>>,
) {
    let Some(sounds) = sounds else { return };
    let listener = camera.map(|c| c.translation());
    for req in requests.read() {
        let Some(handle) = sounds.0.get(req.name.as_str()) else {
            continue;
        };
        // Quieter with distance; silent beyond about 40 blocks.
        let distance = match (req.at, listener) {
            (Some(at), Some(ear)) => at.distance(ear),
            _ => 0.0,
        };
        let falloff = (1.0 - distance / 40.0).clamp(0.0, 1.0);
        let volume = settings.volume * falloff;
        if volume <= 0.01 {
            continue;
        }
        commands.spawn((
            AudioPlayer(handle.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(volume),
                ..PlaybackSettings::ONCE
            },
        ));
    }
}

fn play_ambient(
    mut commands: Commands,
    mut requests: MessageReader<PlayAmbient>,
    sounds: Option<Res<Sounds>>,
    settings: Res<crate::settings::ClientSettings>,
) {
    let Some(sounds) = sounds else { return };
    for req in requests.read() {
        let volume = settings.volume * req.gain;
        if let (Some(handle), true) = (sounds.0.get(req.name), volume > 0.01) {
            commands.spawn((
                AudioPlayer(handle.clone()),
                PlaybackSettings {
                    mode: PlaybackMode::Despawn,
                    volume: Volume::Linear(volume),
                    ..PlaybackSettings::ONCE
                },
            ));
        }
    }
}

/// The sound of rain, looping while it rains.
#[derive(Component)]
struct RainSound;

fn rain_loop(
    mut commands: Commands,
    weather: Res<crate::atmosphere::Weather>,
    sounds: Option<Res<Sounds>>,
    settings: Res<crate::settings::ClientSettings>,
    mut playing: Query<(Entity, &mut AudioSink), With<RainSound>>,
) {
    let want = weather.rain_amount * if weather.snow { 0.0 } else { 1.0 } * settings.volume * 0.35;
    match playing.single_mut() {
        Ok((e, mut sink)) => {
            if want < 0.01 {
                commands.entity(e).despawn();
            } else {
                sink.set_volume(Volume::Linear(want));
            }
        }
        Err(_) => {
            if want >= 0.01
                && let Some(h) = sounds.as_ref().and_then(|s| s.0.get("rain"))
            {
                commands.spawn((
                    RainSound,
                    AudioPlayer(h.clone()),
                    PlaybackSettings {
                        mode: PlaybackMode::Loop,
                        volume: Volume::Linear(want),
                        ..default()
                    },
                ));
            }
        }
    }
}

/// Calm music now and then (every few minutes), if it's switched on.
#[derive(Component)]
struct Music;

fn music_player(
    time: Res<Time>,
    mut commands: Commands,
    sounds: Option<Res<Sounds>>,
    settings: Res<crate::settings::ClientSettings>,
    playing: Query<Entity, With<Music>>,
    mut wait: Local<Option<f32>>,
    mut pick: Local<u32>,
) {
    if !settings.music {
        for e in &playing {
            commands.entity(e).despawn();
        }
        return;
    }
    if !playing.is_empty() {
        return;
    }
    // The first piece comes soon after starting, later ones every few minutes.
    let left = wait.get_or_insert(20.0);
    *left -= time.delta_secs();
    if *left > 0.0 {
        return;
    }
    *pick = pick.wrapping_add(1);
    *left = 150.0 + (*pick % 4) as f32 * 45.0;
    let name = ["music1", "music2", "music3"][*pick as usize % 3];
    if let Some(h) = sounds.as_ref().and_then(|s| s.0.get(name)) {
        commands.spawn((
            Music,
            AudioPlayer(h.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(settings.volume * 0.4),
                ..PlaybackSettings::ONCE
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_are_short_valid_wavs() {
        for name in SOUND_NAMES {
            let samples = generate(name);
            // Effects are short; ambience a few seconds; music under a minute.
            let max_secs = match name {
                n if n.starts_with("music") => 60,
                "wind" | "rain" | "water" => 4,
                _ => 1,
            };
            assert!(
                !samples.is_empty() && samples.len() < max_secs * RATE as usize,
                "{name}"
            );
            assert!(
                samples.iter().all(|s| s.is_finite() && s.abs() <= 1.5),
                "{name}"
            );
            let wav = wav_bytes(&samples);
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(wav.len(), 44 + samples.len() * 2);
        }
    }
}

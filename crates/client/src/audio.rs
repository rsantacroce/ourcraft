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
            .add_systems(Startup, create_sounds)
            .add_systems(PreUpdate, sounds_from_server.after(NetReceiveSet))
            .add_systems(Update, (sounds_from_edits, play_sounds).chain());
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
        // Menu click.
        _ => synth(0.03, |t| (TAU * 1800.0 * t).sin() * 0.4 * env(t, 120.0)),
    }
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

pub const SOUND_NAMES: [&str; 11] = [
    "place", "break", "pop", "splat", "grab", "cheer", "click", "baa", "moo", "oink", "cluck",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_are_short_valid_wavs() {
        for name in SOUND_NAMES {
            let samples = generate(name);
            assert!(
                !samples.is_empty() && samples.len() < RATE as usize,
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

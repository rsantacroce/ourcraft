//! Day/night cycle: time of day, sky colour, sun and moon, and the lighting
//! and fog values fed to the block shader.

use std::f32::consts::TAU;

use bevy::prelude::*;
use ourcraft_common::block::blocks;

use crate::player::PlayerCamera;
use crate::render::{VoxelMaterial, VoxelMaterialHandle};
use crate::settings::GraphicsSettings;
use crate::world::ClientWorld;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeOfDay>()
            .add_systems(Startup, spawn_sun_and_moon)
            .add_systems(PreUpdate, sync_time.after(crate::net::NetReceiveSet))
            .add_systems(
                Update,
                (advance_time, update_sky)
                    .chain()
                    .after(crate::player::PlayerUpdateSet),
            );
    }
}

pub use ourcraft_common::world::DAY_LENGTH_SECS;

/// Time of day as a fraction: 0 = midnight, 0.25 = sunrise, 0.5 = noon,
/// 0.75 = sunset.
#[derive(Resource, Debug, Clone, Copy)]
pub struct TimeOfDay {
    pub fraction: f32,
    /// Set false to freeze time (the server controls it in multiplayer).
    pub running: bool,
}

impl Default for TimeOfDay {
    fn default() -> Self {
        Self {
            fraction: 0.3, // early morning
            running: true,
        }
    }
}

impl TimeOfDay {
    /// Height of the sun: 1 at noon, 0 at the horizon, -1 at midnight.
    pub fn sun_height(&self) -> f32 {
        ((self.fraction - 0.25) * TAU).sin()
    }

    /// Strength of sunlight, 0 (night) to 1 (day), with a soft dawn/dusk.
    pub fn daylight(&self) -> f32 {
        let h = self.sun_height();
        let t = ((h + 0.15) / 0.35).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Clock time as `(hours, minutes)`.
    pub fn clock(&self) -> (u32, u32) {
        let minutes = (self.fraction * 24.0 * 60.0) as u32;
        (minutes / 60 % 24, minutes % 60)
    }

    /// Unit vector pointing at the sun. It rises in the east (+X) and sets in
    /// the west, tilted a little to the south so it's not straight overhead.
    pub fn sun_direction(&self) -> Vec3 {
        let angle = (self.fraction - 0.25) * TAU;
        Vec3::new(angle.cos(), angle.sin(), 0.3).normalize()
    }
}

/// The server keeps the real time; we run our own clock between its
/// updates and snap to it when one arrives.
fn sync_time(mut messages: MessageReader<crate::net::FromServer>, mut tod: ResMut<TimeOfDay>) {
    use ourcraft_common::protocol::ServerMessage;
    for crate::net::FromServer(msg) in messages.read() {
        match msg {
            ServerMessage::TimeOfDay(t) | ServerMessage::Welcome { time_of_day: t, .. } => {
                tod.fraction = t.rem_euclid(1.0);
            }
            _ => {}
        }
    }
}

fn advance_time(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<crate::input::UiFocus>,
    mut tod: ResMut<TimeOfDay>,
) {
    if tod.running {
        tod.fraction = (tod.fraction + time.delta_secs() / DAY_LENGTH_SECS).fract();
    }
    // T skips ahead an hour (handy for testing nights).
    if keys.just_pressed(KeyCode::KeyT) && !focus.chat_open {
        tod.fraction = (tod.fraction + 1.0 / 24.0).fract();
    }
}

#[derive(Component)]
struct Sun;

#[derive(Component)]
struct Moon;

fn spawn_sun_and_moon(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let disc = |color: Color| StandardMaterial {
        base_color: color,
        unlit: true,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    };
    commands.spawn((
        Name::new("Sun"),
        Sun,
        Mesh3d(meshes.add(Rectangle::new(60.0, 60.0))),
        MeshMaterial3d(materials.add(disc(Color::srgb(1.0, 0.93, 0.55)))),
        Transform::default(),
    ));
    commands.spawn((
        Name::new("Moon"),
        Moon,
        Mesh3d(meshes.add(Rectangle::new(40.0, 40.0))),
        MeshMaterial3d(materials.add(disc(Color::srgb(0.85, 0.88, 0.95)))),
        Transform::default(),
    ));
}

/// Mix two colours in linear space.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let (a, b) = (a.to_linear(), b.to_linear());
    Color::LinearRgba(LinearRgba::from_vec4(a.to_vec4().lerp(b.to_vec4(), t)))
}

#[allow(clippy::too_many_arguments)]
fn update_sky(
    tod: Res<TimeOfDay>,
    weather: Res<crate::atmosphere::Weather>,
    settings: Res<GraphicsSettings>,
    world: Res<ClientWorld>,
    camera: Single<&Transform, With<PlayerCamera>>,
    handles: Option<Res<VoxelMaterialHandle>>,
    effects: Res<crate::player::Effects>,
    dimension: Res<crate::net::CurrentDimension>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
    mut clear: ResMut<ClearColor>,
    mut sun: Single<&mut Transform, (With<Sun>, Without<PlayerCamera>, Without<Moon>)>,
    mut moon: Single<&mut Transform, (With<Moon>, Without<PlayerCamera>, Without<Sun>)>,
) {
    // Rain makes the day greyer and a little darker.
    let rain = weather.rain_amount;
    let dreamy = dimension.0 == ourcraft_common::dimension::Dimension::CloudIsles;
    let mut daylight = tod.daylight() * (1.0 - 0.35 * rain);
    // The Cloud Isles: a pale lilac sky, pink sunsets, and nights that are
    // never very dark.
    let (day, night, warm) = if dreamy {
        daylight = daylight.max(0.35);
        (
            Color::srgb(0.72, 0.78, 0.99),
            Color::srgb(0.13, 0.09, 0.26),
            Color::srgb(1.0, 0.5, 0.75),
        )
    } else {
        (
            mix(
                Color::srgb(0.53, 0.75, 0.96),
                Color::srgb(0.55, 0.6, 0.66),
                rain,
            ),
            Color::srgb(0.03, 0.04, 0.11),
            Color::srgb(1.0, 0.45, 0.25),
        )
    };
    let h = tod.sun_height();
    // Sunsets add a warm glow on top of the sky (instead of mixing orange
    // into a half-dark blue, which looks muddy brown).
    let glow = (-(h / 0.2).powi(2)).exp() * (1.0 - rain);
    let base = mix(night, day, daylight).to_linear();
    let warm = warm.to_linear();
    let mut sky = Color::LinearRgba(LinearRgba::new(
        base.red + warm.red * glow * 0.55,
        base.green + warm.green * glow * 0.35,
        base.blue + warm.blue * glow * 0.15,
        1.0,
    ));

    // Fog ends just before the edge of the loaded area.
    let mut fog_end = (settings.render_distance as f32 - 0.5) * 16.0;
    let mut fog_start = fog_end * 0.55;

    // Under water everything turns blue and murky.
    let eye = camera.translation.floor().as_ivec3();
    if world.block(eye.x, eye.y, eye.z) == blocks::WATER {
        sky = mix(
            Color::srgb(0.08, 0.22, 0.48),
            night,
            1.0 - daylight.max(0.3),
        );
        fog_start = 0.0;
        fog_end = 24.0;
    }
    clear.0 = sky;

    // Sun and moon sit far away in opposite directions, facing the camera.
    let dir = tod.sun_direction();
    let cam = camera.translation;
    **sun = Transform::from_translation(cam + dir * 400.0).looking_at(cam, Vec3::Y);
    **moon = Transform::from_translation(cam - dir * 400.0).looking_at(cam, Vec3::Y);

    let Some(handles) = handles else { return };
    for h in handles.both() {
        if let Some(mut mat) = materials.get_mut(h) {
            mat.params.fog_color = sky.to_linear().to_vec4();
            mat.params.fog_start = fog_start;
            mat.params.fog_end = fog_end;
            mat.params.daylight = daylight;
            // A glow potion: see in the dark.
            mat.params.min_light = if effects.has(ourcraft_common::magic::Effect::Glow) {
                0.85
            } else {
                0.3
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_is_bright_and_midnight_is_dark() {
        let at = |fraction| TimeOfDay {
            fraction,
            running: true,
        };
        assert!((at(0.5).daylight() - 1.0).abs() < 1e-4);
        assert!(at(0.0).daylight() < 1e-4);
        assert!(at(0.25).daylight() > 0.2 && at(0.25).daylight() < 0.8);
        assert_eq!(at(0.5).clock(), (12, 0));
        assert_eq!(at(0.75).clock(), (18, 0));
    }
}

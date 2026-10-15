//! A free-flying camera, like creative mode. Replaced by a walking player
//! with gravity and collision in M3.

use std::f32::consts::FRAC_PI_2;

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;

use crate::input::Actions;

pub struct FlyCameraPlugin;

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, fly);
    }
}

#[derive(Component, Debug)]
pub struct FlyCamera {
    /// Rotation around the vertical axis, in radians.
    pub yaw: f32,
    /// Up/down angle in radians, clamped so you can't flip over.
    pub pitch: f32,
    /// Blocks per second.
    pub speed: f32,
    pub sprint_multiplier: f32,
}

fn spawn_camera(mut commands: Commands) {
    // Start outside the chunk, looking at its middle.
    let eye = Vec3::new(-10.0, 16.0, -10.0);
    let target = Vec3::new(8.0, 5.0, 8.0);
    let transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);

    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            ..default()
        }),
        // Our block shader outputs final colours itself; no film-style
        // colour grading on top, which keeps the colours bright and cartoony.
        Tonemapping::None,
        transform,
        FlyCamera {
            yaw,
            pitch,
            speed: 10.0,
            sprint_multiplier: 3.0,
        },
    ));
}

fn fly(time: Res<Time>, actions: Res<Actions>, camera: Single<(&mut Transform, &mut FlyCamera)>) {
    let (mut transform, mut cam) = camera.into_inner();

    // Turn. Yaw is negated because Bevy's rotation is counter-clockwise when
    // seen from above, while "look right" is clockwise.
    const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
    cam.yaw -= actions.look.x;
    cam.pitch = (cam.pitch + actions.look.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);

    // Move horizontally relative to where we're facing (ignoring pitch, so
    // looking down while pressing W doesn't dive into the ground), and
    // vertically along world up.
    let yaw_rot = Quat::from_rotation_y(cam.yaw);
    let forward = yaw_rot * Vec3::NEG_Z;
    let right = yaw_rot * Vec3::X;
    let wish =
        right * actions.movement.x + Vec3::Y * actions.movement.y + forward * actions.movement.z;

    let mut speed = cam.speed;
    if actions.sprint {
        speed *= cam.sprint_multiplier;
    }
    // `clamp_length_max` stops diagonal movement from being faster.
    transform.translation += wish.clamp_length_max(1.0) * speed * time.delta_secs();
}

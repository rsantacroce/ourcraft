//! Camera views: first person, third person from behind, and third person
//! from the front (F5, the gamepad's D-pad down, or the touch View button
//! switches between them).
//!
//! In the third-person views you see your own body: the same avatar model
//! other players see, wearing your skin. The camera backs away from your
//! eyes along the view direction, but never into a wall: a few rays (one
//! from the middle and one from each corner of the camera's near plane)
//! find how far it can go, and it stops just short of that. It pulls in
//! instantly when something gets in the way and eases back out after.
//!
//! Aiming doesn't change: blocks are still broken and placed from your
//! eyes (see [`crate::player::Player::eyes`]), not from the camera.

use std::f32::consts::PI;

use bevy::prelude::*;
use ourcraft_common::physics::EYE_HEIGHT;
use ourcraft_common::raycast::clear_distance;

use crate::avatar::{AvatarArmour, AvatarLook, AvatarMeshes, AvatarPose, AvatarSet, spawn_avatar};
use crate::input::Actions;
use crate::player::{Asleep, Player, PlayerCamera, PlayerUpdateSet, follow_camera};
use crate::world::{Blocks, ClientWorld};

/// How far behind (or in front of) you the camera sits, in blocks.
const DISTANCE: f32 = 4.0;
/// How far the camera stays from walls.
const WALL_MARGIN: f32 = 0.25;
/// Half the size of the camera's near plane, for the corner rays.
const CAMERA_RADIUS: f32 = 0.2;
/// Closer than this, the camera would be inside your head: hide the body.
const MIN_BODY_DISTANCE: f32 = 0.7;

pub struct CameraViewPlugin;

impl Plugin for CameraViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraView>()
            .add_systems(
                Update,
                (
                    switch_view.before(follow_camera),
                    place_camera.after(follow_camera),
                )
                    .in_set(PlayerUpdateSet),
            )
            .add_systems(
                Update,
                (spawn_my_avatar, pose_my_avatar)
                    .chain()
                    .after(PlayerUpdateSet)
                    .before(AvatarSet),
            );
    }
}

/// Where the camera is, relative to you.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewMode {
    #[default]
    FirstPerson,
    /// Behind you, looking where you look.
    Behind,
    /// In front of you, looking back at your face.
    Front,
}

impl ViewMode {
    pub fn next(self) -> Self {
        match self {
            ViewMode::FirstPerson => ViewMode::Behind,
            ViewMode::Behind => ViewMode::Front,
            ViewMode::Front => ViewMode::FirstPerson,
        }
    }
}

#[derive(Resource, Default, Debug)]
pub struct CameraView {
    pub mode: ViewMode,
    /// How far the camera is from your eyes right now (0 in first person).
    pub distance: f32,
}

/// Your own body, seen in the third-person views.
#[derive(Component)]
struct MyAvatar;

fn switch_view(actions: Res<Actions>, mut view: ResMut<CameraView>) {
    if actions.cycle_view {
        view.mode = view.mode.next();
        info!("Camera view: {:?}", view.mode);
    }
}

/// How far the camera can back away from `eye` along `dir` without
/// entering a block. `right` and `up` span the camera's near plane, whose
/// corners must stay clear too.
fn camera_distance(
    eye: Vec3,
    dir: Vec3,
    right: Vec3,
    up: Vec3,
    clear: impl Fn(Vec3, Vec3) -> f32,
) -> f32 {
    let max = DISTANCE + WALL_MARGIN;
    let corners = [
        Vec3::ZERO,
        (right + up) * CAMERA_RADIUS,
        (right - up) * CAMERA_RADIUS,
        (-right + up) * CAMERA_RADIUS,
        (-right - up) * CAMERA_RADIUS,
    ];
    let nearest = corners
        .iter()
        .map(|offset| clear(eye + *offset, dir).min(max))
        .fold(max, f32::min);
    (nearest - WALL_MARGIN).clamp(0.0, DISTANCE)
}

/// Move the camera from your eyes to behind or in front of you.
fn place_camera(
    time: Res<Time>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    asleep: Res<Asleep>,
    player: Single<&Player>,
    mut view: ResMut<CameraView>,
    mut camera: Single<&mut Transform, With<PlayerCamera>>,
) {
    if view.mode == ViewMode::FirstPerson || asleep.0.is_some() {
        view.distance = 0.0;
        return;
    }
    let eye = camera.translation;
    let look = player.look_direction();
    let (dir, rotation) = match view.mode {
        ViewMode::Front => (
            look,
            Quat::from_euler(EulerRot::YXZ, player.yaw + PI, -player.pitch, 0.0),
        ),
        _ => (-look, camera.rotation),
    };
    let right = rotation * Vec3::X;
    let up = rotation * Vec3::Y;
    let wanted = camera_distance(eye, dir, right, up, |from, d| {
        clear_distance(&world.0, &blocks.0, from, d, DISTANCE + WALL_MARGIN)
    });
    // Snap in when something's in the way; glide back out.
    view.distance = if wanted < view.distance {
        wanted
    } else {
        let t = 1.0 - (-6.0 * time.delta_secs()).exp();
        view.distance + (wanted - view.distance) * t
    };
    camera.translation = eye + dir * view.distance;
    camera.rotation = rotation;
}

fn spawn_my_avatar(
    mut commands: Commands,
    meshes: Option<Res<AvatarMeshes>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<(), With<MyAvatar>>,
) {
    let Some(meshes) = meshes else { return };
    if existing.is_empty() {
        let avatar = spawn_avatar(&mut commands, &meshes, &mut materials);
        commands
            .entity(avatar)
            .insert((Name::new("My avatar"), MyAvatar));
    }
}

/// Keep your body where you are, dressed in your skin (and the team shirt
/// and armour the server says you have), and shown only in third person.
fn pose_my_avatar(
    view: Res<CameraView>,
    asleep: Res<Asleep>,
    mine: Res<crate::skins::MySkin>,
    others: Res<crate::remote_players::RemotePlayers>,
    conn: Option<Res<crate::net::Connection>>,
    player: Single<&Player>,
    mut avatar: Single<
        (
            &mut Transform,
            &mut Visibility,
            &mut AvatarPose,
            &mut AvatarLook,
            &mut AvatarArmour,
        ),
        With<MyAvatar>,
    >,
) {
    let (transform, vis, pose, look, armour) = &mut *avatar;
    let shown = view.mode != ViewMode::FirstPerson
        && asleep.0.is_none()
        && !player.frozen
        && view.distance > MIN_BODY_DISTANCE;
    vis.set_if_neq(if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if !shown {
        return;
    }
    transform.translation = player.eyes - Vec3::Y * EYE_HEIGHT;
    transform.rotation = Quat::from_rotation_y(player.yaw);
    pose.set(player.state.velocity, player.pitch, player.state.flying);

    // The server tells everyone (us included) about team colours and armour.
    let me = conn.and_then(|c| c.my_id);
    let team = me
        .and_then(|id| others.appearances.get(&id))
        .and_then(|(color, _)| *color)
        .map(|[r, g, b, a]| Color::srgba_u8(r, g, b, a));
    look.set_if_neq(AvatarLook {
        skin: mine.skin.clone(),
        team,
    });
    let worn = me.and_then(|id| others.armour.get(&id)).copied();
    armour.set_if_neq(AvatarArmour(worn.unwrap_or_default()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_cycle_through_all_three() {
        let mut m = ViewMode::default();
        assert_eq!(m, ViewMode::FirstPerson);
        m = m.next();
        assert_eq!(m, ViewMode::Behind);
        m = m.next();
        assert_eq!(m, ViewMode::Front);
        assert_eq!(m.next(), ViewMode::FirstPerson);
    }

    #[test]
    fn the_camera_keeps_out_of_walls() {
        let eye = Vec3::new(0.5, 10.0, 0.5);
        let (right, up) = (Vec3::X, Vec3::Y);
        // Open sky: the full distance.
        let d = camera_distance(eye, Vec3::Z, right, up, |_, _| f32::INFINITY);
        assert_eq!(d, DISTANCE);
        // A wall 2 blocks back: stop just short of it.
        let d = camera_distance(eye, Vec3::Z, right, up, |_, _| 2.0);
        assert!((d - (2.0 - WALL_MARGIN)).abs() < 1e-5);
        // Only one corner of the camera would touch something: that counts.
        let d = camera_distance(eye, Vec3::Z, right, up, |from, _| {
            if from.x > eye.x && from.y > eye.y {
                1.0
            } else {
                10.0
            }
        });
        assert!((d - (1.0 - WALL_MARGIN)).abs() < 1e-5);
        // Right against a wall: the camera stays at your eyes.
        let d = camera_distance(eye, Vec3::Z, right, up, |_, _| 0.1);
        assert_eq!(d, 0.0);
    }
}

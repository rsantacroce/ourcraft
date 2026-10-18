//! Other players: blocky avatars (see [`crate::avatar`]) wearing their
//! skins, with name tags.
//!
//! **Interpolation.** Position updates arrive about 20 times a second, a bit
//! unevenly. Snapping avatars to each update would look jerky. Instead we
//! show other players slightly in the past (100 ms) and blend smoothly
//! between the two updates around that moment. There's almost always a
//! newer update already waiting, so the motion is smooth even when packets
//! arrive late. (Your own player doesn't need this: it moves locally.)

use std::collections::{HashMap, VecDeque};
use std::f32::consts::PI;

use bevy::prelude::*;
use ourcraft_common::protocol::{NetPlayerState, PlayerId, PlayerInfo, ServerMessage};
use ourcraft_common::skin::NetSkin;

use crate::avatar::{AvatarArmour, AvatarLook, AvatarMeshes, AvatarPose, AvatarSet, spawn_avatar};
use crate::net::{Connection, FromServer, INTERPOLATION_DELAY, NetReceiveSet};
use crate::player::{PlayerCamera, PlayerUpdateSet};

pub struct RemotePlayersPlugin;

impl Plugin for RemotePlayersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemotePlayers>()
            .add_systems(PreUpdate, handle_player_messages.after(NetReceiveSet))
            .add_systems(
                PreUpdate,
                reset_players.run_if(on_message::<crate::net::SessionEnded>),
            )
            .add_systems(
                Update,
                (apply_appearances, interpolate, update_nameplates)
                    .chain()
                    .after(PlayerUpdateSet)
                    .before(AvatarSet),
            );
    }
}

/// Player id → avatar entity.
#[derive(Resource, Default)]
pub struct RemotePlayers {
    pub entities: HashMap<PlayerId, Entity>,
    /// Shirt colour and name tag prefix set by the game mode.
    pub appearances: HashMap<PlayerId, (Option<[u8; 4]>, Option<String>)>,
    /// What each player wears (item numbers, helmet first; 0 = nothing).
    pub armour: HashMap<PlayerId, [u16; 4]>,
    /// Each player's skin (players who haven't said get the default).
    pub skins: HashMap<PlayerId, NetSkin>,
}

impl RemotePlayers {
    pub fn count(&self) -> usize {
        self.entities.len()
    }
}

#[derive(Component)]
pub struct RemotePlayer {
    pub id: PlayerId,
    pub name: String,
    /// Recent updates: (arrival time in seconds, state).
    samples: VecDeque<(f64, NetPlayerState)>,
    /// Interpolated state shown this frame.
    pub shown: NetPlayerState,
    nameplate: Entity,
}

/// Another player's avatar, with a name tag (a UI node kept over its head).
fn spawn_remote(
    commands: &mut Commands,
    meshes: &AvatarMeshes,
    materials: &mut Assets<StandardMaterial>,
    info: &PlayerInfo,
) -> Entity {
    let nameplate = commands
        .spawn((
            Text::new(info.name.clone()),
            TextFont::from_font_size(15.0),
            TextColor(Color::WHITE),
            TextShadow {
                offset: Vec2::splat(1.0),
                color: Color::BLACK,
            },
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            Visibility::Hidden,
        ))
        .id();
    let avatar = spawn_avatar(commands, meshes, materials);
    commands.entity(avatar).insert((
        Name::new(format!("Avatar {}", info.name)),
        RemotePlayer {
            id: info.id,
            name: info.name.clone(),
            samples: VecDeque::new(),
            shown: NetPlayerState::default(),
            nameplate,
        },
    ));
    avatar
}

fn handle_player_messages(
    mut commands: Commands,
    mut messages: MessageReader<FromServer>,
    time: Res<Time<Real>>,
    conn: Option<Res<Connection>>,
    meshes: Res<AvatarMeshes>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut players: ResMut<RemotePlayers>,
    mut avatars: Query<&mut RemotePlayer>,
) {
    let my_id = conn.and_then(|c| c.my_id);
    for FromServer(msg) in messages.read() {
        match msg {
            ServerMessage::Welcome { players: list, .. } => {
                for info in list {
                    let e = spawn_remote(&mut commands, &meshes, &mut materials, info);
                    players.entities.insert(info.id, e);
                }
            }
            ServerMessage::PlayerJoined(info) => {
                if Some(info.id) != my_id && !players.entities.contains_key(&info.id) {
                    let e = spawn_remote(&mut commands, &meshes, &mut materials, info);
                    players.entities.insert(info.id, e);
                }
            }
            ServerMessage::Appearance { player, color, tag } => {
                players.appearances.insert(*player, (*color, tag.clone()));
            }
            ServerMessage::Armour { player, pieces } => {
                players.armour.insert(*player, *pieces);
            }
            ServerMessage::Skin { player, skin } => {
                players.skins.insert(*player, skin.clone());
            }
            ServerMessage::PlayerLeft(id) => {
                players.appearances.remove(id);
                players.armour.remove(id);
                players.skins.remove(id);
                if let Some(e) = players.entities.remove(id) {
                    if let Ok(p) = avatars.get(e) {
                        commands.entity(p.nameplate).despawn();
                    }
                    commands.entity(e).despawn();
                }
            }
            ServerMessage::PlayerStates {
                players: states, ..
            } => {
                let now = time.elapsed_secs_f64();
                for (id, state) in states {
                    if Some(*id) == my_id {
                        continue;
                    }
                    if let Some(&e) = players.entities.get(id)
                        && let Ok(mut p) = avatars.get_mut(e)
                    {
                        p.samples.push_back((now, *state));
                        // A second of history is plenty.
                        while p.samples.len() > 30 {
                            p.samples.pop_front();
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Dress avatars (skin, team shirt, armour) and update name tags when
/// the server tells us something changed.
fn apply_appearances(
    players: Res<RemotePlayers>,
    mut avatars: Query<(&RemotePlayer, &mut AvatarLook, &mut AvatarArmour)>,
    mut plates: Query<(&mut Text, &mut TextColor)>,
) {
    if !players.is_changed() {
        return;
    }
    for (p, mut look, mut armour) in &mut avatars {
        let (color, tag) = players.appearances.get(&p.id).cloned().unwrap_or_default();
        let team = color.map(|[r, g, b, a]| Color::srgba_u8(r, g, b, a));
        look.set_if_neq(AvatarLook {
            skin: players.skins.get(&p.id).cloned().unwrap_or_default(),
            team,
        });
        armour.set_if_neq(AvatarArmour(
            players.armour.get(&p.id).copied().unwrap_or_default(),
        ));
        if let Ok((mut text, mut text_color)) = plates.get_mut(p.nameplate) {
            text.0 = match &tag {
                Some(t) => format!("{t} {}", p.name),
                None => p.name.clone(),
            };
            text_color.0 = team.map_or(Color::WHITE, |c| c.lighter(0.15));
        }
    }
}

fn reset_players(
    mut commands: Commands,
    mut players: ResMut<RemotePlayers>,
    avatars: Query<&RemotePlayer>,
) {
    for (_, e) in players.entities.drain() {
        if let Ok(p) = avatars.get(e) {
            commands.entity(p.nameplate).despawn();
        }
        commands.entity(e).despawn();
    }
    players.appearances.clear();
    players.armour.clear();
    players.skins.clear();
}

/// Blend between the two updates around (now − delay).
fn sample_at(samples: &VecDeque<(f64, NetPlayerState)>, t: f64) -> Option<NetPlayerState> {
    let (first, last) = (samples.front()?, samples.back()?);
    if t <= first.0 {
        return Some(first.1);
    }
    if t >= last.0 {
        return Some(last.1);
    }
    let i = samples.iter().position(|(time, _)| *time > t)?;
    let (t0, a) = samples[i - 1];
    let (t1, b) = samples[i];
    let f = ((t - t0) / (t1 - t0)) as f32;
    let lerp3 =
        |x: [f32; 3], y: [f32; 3]| Vec3::from_array(x).lerp(Vec3::from_array(y), f).to_array();
    // Turn the short way round.
    let mut dyaw = (b.yaw - a.yaw) % (2.0 * PI);
    if dyaw > PI {
        dyaw -= 2.0 * PI;
    } else if dyaw < -PI {
        dyaw += 2.0 * PI;
    }
    Some(NetPlayerState {
        position: lerp3(a.position, b.position),
        velocity: lerp3(a.velocity, b.velocity),
        yaw: a.yaw + dyaw * f,
        pitch: a.pitch + (b.pitch - a.pitch) * f,
        flying: b.flying,
        on_ground: b.on_ground,
    })
}

fn interpolate(
    time: Res<Time<Real>>,
    mut avatars: Query<(
        &mut RemotePlayer,
        &mut Transform,
        &mut Visibility,
        &mut AvatarPose,
    )>,
) {
    let t = time.elapsed_secs_f64() - INTERPOLATION_DELAY.as_secs_f64();
    for (mut p, mut transform, mut vis, mut pose) in &mut avatars {
        let Some(state) = sample_at(&p.samples, t) else {
            continue;
        };
        p.shown = state;
        *vis = Visibility::Inherited;
        transform.translation = Vec3::from_array(state.position);
        transform.rotation = Quat::from_rotation_y(state.yaw);
        pose.set(Vec3::from_array(state.velocity), state.pitch, state.flying);
    }
}

/// Keep each name tag floating over its player's head.
fn update_nameplates(
    camera: Single<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    avatars: Query<(&RemotePlayer, &Transform)>,
    mut plates: Query<(&mut Node, &mut Visibility, &ComputedNode), Without<RemotePlayer>>,
) {
    let (camera, cam_transform) = *camera;
    for (p, transform) in &avatars {
        let Ok((mut node, mut vis, computed)) = plates.get_mut(p.nameplate) else {
            continue;
        };
        let head = transform.translation + Vec3::Y * 2.25;
        let near = head.distance(cam_transform.translation()) < 48.0;
        match camera.world_to_viewport(cam_transform, head) {
            Ok(screen) if near && !p.samples.is_empty() => {
                let size = computed.size() * computed.inverse_scale_factor();
                node.left = px(screen.x - size.x / 2.0);
                node.top = px(screen.y - size.y);
                *vis = Visibility::Inherited;
            }
            _ => *vis = Visibility::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(x: f32, yaw: f32) -> NetPlayerState {
        NetPlayerState {
            position: [x, 0.0, 0.0],
            yaw,
            ..Default::default()
        }
    }

    #[test]
    fn interpolates_between_samples() {
        let samples: VecDeque<_> = [(1.0, state(0.0, 0.0)), (2.0, state(10.0, 1.0))].into();
        let s = sample_at(&samples, 1.5).unwrap();
        assert!((s.position[0] - 5.0).abs() < 1e-4);
        assert!((s.yaw - 0.5).abs() < 1e-4);
        // Before the first / after the last sample: hold the nearest.
        assert_eq!(sample_at(&samples, 0.0).unwrap().position[0], 0.0);
        assert_eq!(sample_at(&samples, 9.0).unwrap().position[0], 10.0);
    }

    #[test]
    fn turns_the_short_way_round() {
        let samples: VecDeque<_> = [(0.0, state(0.0, 3.0)), (1.0, state(0.0, -3.0))].into();
        let s = sample_at(&samples, 0.5).unwrap();
        // From 3 to -3 radians the short way passes through ±π, not 0.
        assert!(s.yaw.abs() > 3.0, "yaw {}", s.yaw);
    }
}

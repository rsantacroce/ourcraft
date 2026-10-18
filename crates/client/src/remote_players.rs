//! Other players: blocky avatars with name tags.
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

use crate::net::{Connection, FromServer, INTERPOLATION_DELAY, NetReceiveSet};
use crate::player::{PlayerCamera, PlayerUpdateSet};
use crate::sky::TimeOfDay;

pub struct RemotePlayersPlugin;

impl Plugin for RemotePlayersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemotePlayers>()
            .add_systems(Startup, create_avatar_meshes)
            .add_systems(PreUpdate, handle_player_messages.after(NetReceiveSet))
            .add_systems(
                PreUpdate,
                reset_players.run_if(on_message::<crate::net::SessionEnded>),
            )
            .add_systems(
                PreUpdate,
                forget_positions
                    .run_if(on_message::<crate::net::DimensionChanged>)
                    .after(NetReceiveSet),
            )
            .add_systems(
                Update,
                (
                    apply_appearances,
                    apply_armour,
                    interpolate,
                    animate_limbs,
                    update_nameplates,
                    shade_avatars,
                )
                    .chain()
                    .after(PlayerUpdateSet),
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
    /// Walk animation phase, advanced by horizontal speed.
    stride: f32,
    nameplate: Entity,
}

#[derive(Component)]
struct AvatarHead;

/// A piece of armour on an avatar (0 helmet … 3 boots), shown when worn.
#[derive(Component)]
struct ArmourPiece(usize);

/// The colour of armour made of a material, from the item's name.
fn armour_colour(item: &str) -> Color {
    let material = item
        .trim_start_matches("core:")
        .split('_')
        .next()
        .unwrap_or("");
    match material {
        "wool" => Color::srgb(0.92, 0.9, 0.84),
        "iron" => Color::srgb(0.82, 0.83, 0.86),
        "gold" => Color::srgb(0.95, 0.78, 0.25),
        "diamond" => Color::srgb(0.35, 0.86, 0.86),
        _ => Color::srgb(0.6, 0.6, 0.6),
    }
}

/// The part of an avatar the game mode can recolour.
#[derive(Component)]
struct Shirt;

/// A leg or arm; swings around its top. `sign` alternates the swing.
#[derive(Component)]
struct Limb {
    sign: f32,
}

/// An avatar part's unlit colour before day/night shading.
#[derive(Component)]
struct AvatarColor(Color);

#[derive(Resource)]
struct AvatarMeshes {
    head: Handle<Mesh>,
    body: Handle<Mesh>,
    limb: Handle<Mesh>,
    eye: Handle<Mesh>,
    helmet: Handle<Mesh>,
    chestplate: Handle<Mesh>,
    legging: Handle<Mesh>,
    boot: Handle<Mesh>,
}

fn create_avatar_meshes(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(AvatarMeshes {
        head: meshes.add(Cuboid::new(0.5, 0.5, 0.5)),
        body: meshes.add(Cuboid::new(0.5, 0.7, 0.28)),
        limb: meshes.add(Cuboid::new(0.22, 0.7, 0.22)),
        eye: meshes.add(Cuboid::new(0.09, 0.09, 0.02)),
        helmet: meshes.add(Cuboid::new(0.56, 0.3, 0.56)),
        chestplate: meshes.add(Cuboid::new(0.56, 0.6, 0.34)),
        legging: meshes.add(Cuboid::new(0.26, 0.45, 0.26)),
        boot: meshes.add(Cuboid::new(0.26, 0.2, 0.28)),
    });
}

/// A cheerful colour picked from the player's id, so each player looks
/// different but always the same.
fn shirt_color(id: PlayerId) -> Color {
    let hue = (ourcraft_common::worldgen::splitmix64(id) % 360) as f32;
    Color::hsl(hue, 0.65, 0.55)
}

fn spawn_avatar(
    commands: &mut Commands,
    meshes: &AvatarMeshes,
    materials: &mut Assets<StandardMaterial>,
    info: &PlayerInfo,
) -> Entity {
    let mut part = |color: Color| {
        (
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                ..default()
            })),
            AvatarColor(color),
        )
    };
    let shirt = part(shirt_color(info.id));
    let skin = part(Color::srgb(0.96, 0.8, 0.62));
    let skin2 = part(Color::srgb(0.96, 0.8, 0.62));
    let skin3 = part(Color::srgb(0.96, 0.8, 0.62));
    let pants = part(Color::srgb(0.22, 0.3, 0.55));
    let pants2 = part(Color::srgb(0.22, 0.3, 0.55));
    let eye = part(Color::srgb(0.1, 0.1, 0.15));
    let eye2 = part(Color::srgb(0.1, 0.1, 0.15));
    // Armour, hidden until worn: (slot, mesh, where).
    let mut armour = |slot: usize, mesh: &Handle<Mesh>, at: Vec3| {
        (
            ArmourPiece(slot),
            Mesh3d(mesh.clone()),
            part(Color::WHITE),
            Transform::from_translation(at),
            Visibility::Hidden,
        )
    };
    let helmet = armour(0, &meshes.helmet, Vec3::new(0.0, 0.42, 0.0));
    let chestplate = armour(1, &meshes.chestplate, Vec3::new(0.0, 1.1, 0.0));
    let legs = [
        armour(2, &meshes.legging, Vec3::new(0.0, -0.2, 0.0)),
        armour(2, &meshes.legging, Vec3::new(0.0, -0.2, 0.0)),
    ];
    let boots = [
        armour(3, &meshes.boot, Vec3::new(0.0, -0.6, -0.01)),
        armour(3, &meshes.boot, Vec3::new(0.0, -0.6, -0.01)),
    ];

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

    commands
        .spawn((
            Name::new(format!("Avatar {}", info.name)),
            RemotePlayer {
                id: info.id,
                name: info.name.clone(),
                samples: VecDeque::new(),
                shown: NetPlayerState::default(),
                stride: 0.0,
                nameplate,
            },
            Transform::default(),
            Visibility::Hidden, // until the first position arrives
        ))
        .with_children(|root| {
            // Body and head. The model faces -Z, like the camera.
            root.spawn((
                Mesh3d(meshes.body.clone()),
                shirt,
                Shirt,
                Transform::from_xyz(0.0, 1.05, 0.0),
            ));
            root.spawn(chestplate);
            root.spawn((
                AvatarHead,
                Transform::from_xyz(0.0, 1.4, 0.0),
                Visibility::Inherited,
            ))
            .with_children(|head| {
                head.spawn((
                    Mesh3d(meshes.head.clone()),
                    skin,
                    Transform::from_xyz(0.0, 0.25, 0.0),
                ));
                head.spawn(helmet);
                head.spawn((
                    Mesh3d(meshes.eye.clone()),
                    eye,
                    Transform::from_xyz(-0.11, 0.3, -0.255),
                ));
                head.spawn((
                    Mesh3d(meshes.eye.clone()),
                    eye2,
                    Transform::from_xyz(0.11, 0.3, -0.255),
                ));
            });
            // Limbs pivot at the shoulder/hip, with the mesh hanging below.
            let limbs = [
                (Vec3::new(-0.36, 1.38, 0.0), skin2, 1.0),
                (Vec3::new(0.36, 1.38, 0.0), skin3, -1.0),
                (Vec3::new(-0.12, 0.7, 0.0), pants, -1.0),
                (Vec3::new(0.12, 0.7, 0.0), pants2, 1.0),
            ];
            let [leg_a, leg_b] = legs;
            let [boot_a, boot_b] = boots;
            let mut worn = [None, None, Some((leg_a, boot_a)), Some((leg_b, boot_b))];
            for (i, (pivot, material, sign)) in limbs.into_iter().enumerate() {
                let mut limb = root.spawn((
                    Limb { sign },
                    Transform::from_translation(pivot),
                    Visibility::Inherited,
                ));
                limb.with_child((
                    Mesh3d(meshes.limb.clone()),
                    material,
                    Transform::from_xyz(0.0, -0.33, 0.0),
                ));
                // Leggings and boots go on the legs.
                if let Some((leg, boot)) = worn[i].take() {
                    limb.with_child(leg);
                    limb.with_child(boot);
                }
            }
        })
        .id()
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
                    let e = spawn_avatar(&mut commands, &meshes, &mut materials, info);
                    players.entities.insert(info.id, e);
                }
            }
            ServerMessage::PlayerJoined(info) => {
                if Some(info.id) != my_id && !players.entities.contains_key(&info.id) {
                    let e = spawn_avatar(&mut commands, &meshes, &mut materials, info);
                    players.entities.insert(info.id, e);
                }
            }
            ServerMessage::Appearance { player, color, tag } => {
                players.appearances.insert(*player, (*color, tag.clone()));
            }
            ServerMessage::Armour { player, pieces } => {
                players.armour.insert(*player, *pieces);
            }
            ServerMessage::PlayerLeft(id) => {
                players.appearances.remove(id);
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
                // Players left out are somewhere else (another dimension):
                // hide them until they're back.
                for mut p in &mut avatars {
                    if !states.iter().any(|(id, _)| *id == p.id) {
                        p.samples.clear();
                    }
                }
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

/// Show the armour each player wears.
fn apply_armour(
    players: Res<RemotePlayers>,
    items: Res<crate::items::Items>,
    avatars: Query<(&RemotePlayer, Entity)>,
    children: Query<&Children>,
    mut pieces: Query<(&ArmourPiece, &mut Visibility, &mut AvatarColor)>,
) {
    if !players.is_changed() {
        return;
    }
    for (p, root) in &avatars {
        let worn = players.armour.get(&p.id).copied().unwrap_or_default();
        for e in children.iter_descendants(root) {
            let Ok((piece, mut vis, mut colour)) = pieces.get_mut(e) else {
                continue;
            };
            let item = worn[piece.0];
            *vis = if item == 0 {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            if item != 0 {
                colour.0 = armour_colour(items.name(ourcraft_common::item::ItemId(item)));
            }
        }
    }
}

/// Recolour shirts and update name tags when the game mode changes them.
fn apply_appearances(
    players: Res<RemotePlayers>,
    avatars: Query<(&RemotePlayer, &Children)>,
    mut shirts: Query<&mut AvatarColor, With<Shirt>>,
    mut plates: Query<(&mut Text, &mut TextColor)>,
) {
    if !players.is_changed() {
        return;
    }
    for (p, children) in &avatars {
        let (color, tag) = players.appearances.get(&p.id).cloned().unwrap_or_default();
        let shirt = color.map_or_else(
            || shirt_color(p.id),
            |[r, g, b, a]| Color::srgba_u8(r, g, b, a),
        );
        for child in children.iter() {
            if let Ok(mut c) = shirts.get_mut(child) {
                c.0 = shirt;
            }
        }
        if let Ok((mut text, mut text_color)) = plates.get_mut(p.nameplate) {
            text.0 = match &tag {
                Some(t) => format!("{t} {}", p.name),
                None => p.name.clone(),
            };
            text_color.0 = if color.is_some() {
                shirt.lighter(0.15)
            } else {
                Color::WHITE
            };
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
}

/// We've gone to another dimension: nobody is where we last saw them.
fn forget_positions(mut avatars: Query<&mut RemotePlayer>) {
    for mut p in &mut avatars {
        p.samples.clear();
    }
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
        &Children,
    )>,
    mut heads: Query<&mut Transform, (With<AvatarHead>, Without<RemotePlayer>)>,
) {
    let t = time.elapsed_secs_f64() - INTERPOLATION_DELAY.as_secs_f64();
    for (mut p, mut transform, mut vis, children) in &mut avatars {
        let Some(state) = sample_at(&p.samples, t) else {
            *vis = Visibility::Hidden;
            continue;
        };
        p.shown = state;
        *vis = Visibility::Inherited;
        transform.translation = Vec3::from_array(state.position);
        transform.rotation = Quat::from_rotation_y(state.yaw);
        for child in children.iter() {
            if let Ok(mut head) = heads.get_mut(child) {
                head.rotation = Quat::from_rotation_x(state.pitch.clamp(-1.2, 1.2));
            }
        }
    }
}

fn animate_limbs(
    time: Res<Time>,
    mut avatars: Query<(&mut RemotePlayer, &Children)>,
    mut limbs: Query<(&Limb, &mut Transform)>,
) {
    for (mut p, children) in &mut avatars {
        let v = Vec3::from_array(p.shown.velocity);
        let speed = Vec2::new(v.x, v.z).length();
        let walking = !p.shown.flying && speed > 0.3;
        p.stride += speed * time.delta_secs() * 2.2;
        let swing = if walking { p.stride.sin() * 0.7 } else { 0.0 };
        for child in children.iter() {
            if let Ok((limb, mut t)) = limbs.get_mut(child) {
                t.rotation = Quat::from_rotation_x(swing * limb.sign);
            }
        }
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

/// Avatars are unlit (no lights in our world), so darken them at night by
/// hand to match the terrain.
fn shade_avatars(
    tod: Res<TimeOfDay>,
    parts: Query<(&MeshMaterial3d<StandardMaterial>, &AvatarColor)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let light = tod.daylight().max(0.3);
    for (mat, color) in &parts {
        if let Some(mut m) = materials.get_mut(&mat.0) {
            let c = color.0.to_linear();
            m.base_color = Color::LinearRgba(LinearRgba::new(
                c.red * light,
                c.green * light,
                c.blue * light,
                1.0,
            ));
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

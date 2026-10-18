//! The blocky player model, shared by other players' avatars and your own
//! body (seen in the third-person views).
//!
//! The model is six boxes (head, body, two arms, two legs) with a slightly
//! bigger see-through box around each for the skin's outer layer (hat,
//! jacket, sleeves, trousers). Every box's faces are cut from the skin
//! picture in the classic pattern, so any 64×64 skin wraps on correctly.
//! Sizes are in skin pixels: the model is 32 pixels tall, like the
//! player's 1.8 block hitbox.
//!
//! An avatar's root entity carries:
//! - [`AvatarPose`]: where it looks and how fast it moves (the limbs swing
//!   from that);
//! - [`AvatarLook`]: its skin, and a team shirt colour from a game mode;
//! - [`AvatarArmour`]: what armour it wears.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use ourcraft_common::skin::{BodyPart, Face, SKIN_SIZE, SkinBox, face_rects};

use crate::skins::SkinImages;
use crate::sky::TimeOfDay;

/// Blocks per skin pixel (the model is 32 pixels tall).
const PIXEL: f32 = ourcraft_common::physics::PLAYER_HEIGHT / 32.0;

pub struct AvatarPlugin;

impl Plugin for AvatarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_avatar_meshes).add_systems(
            Update,
            (dress_avatars, show_armour, animate_avatars, shade_avatars)
                .chain()
                .in_set(AvatarSet),
        );
    }
}

/// Dressing, posing and shading avatars. Systems that move avatars run
/// before this.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct AvatarSet;

/// Where an avatar looks and how it moves, set each frame by whoever owns
/// it (network interpolation, or the local player).
#[derive(Component, Default, Debug, Clone)]
pub struct AvatarPose {
    pub velocity: Vec3,
    pub pitch: f32,
    pub flying: bool,
    /// Walk animation phase, advanced by horizontal speed.
    stride: f32,
}

impl AvatarPose {
    pub fn set(&mut self, velocity: Vec3, pitch: f32, flying: bool) {
        self.velocity = velocity;
        self.pitch = pitch;
        self.flying = flying;
    }
}

/// How an avatar looks.
#[derive(Component, Default, Debug, Clone, PartialEq)]
pub struct AvatarLook {
    pub skin: ourcraft_common::skin::NetSkin,
    /// A game mode's team colour: the shirt is painted plainly in it so
    /// teams are easy to tell apart.
    pub team: Option<Color>,
}

/// What an avatar wears (item numbers, helmet first; 0 = nothing).
#[derive(Component, Default, Debug, Clone, PartialEq)]
pub struct AvatarArmour(pub [u16; 4]);

/// The avatar's own materials: the skin, its see-through outer layer, and
/// the shirt (separate so a team colour can replace it).
#[derive(Component)]
struct AvatarMaterials {
    skin: Handle<StandardMaterial>,
    outer: Handle<StandardMaterial>,
    shirt: Handle<StandardMaterial>,
}

#[derive(Component)]
struct AvatarHead;

/// The outer layer of the body (a jacket), hidden under a team shirt.
#[derive(Component)]
struct Jacket;

/// A leg or arm; swings around its top. `sign` alternates the swing.
#[derive(Component)]
struct Limb {
    sign: f32,
}

/// A piece of armour on an avatar (0 helmet … 3 boots), shown when worn.
#[derive(Component)]
struct ArmourPiece(usize);

/// A part's colour before day/night shading (white for skinned parts:
/// the texture gives the colour).
#[derive(Component, Clone)]
struct AvatarColor(Color);

/// The model's meshes (the same for every avatar; skins differ by texture).
#[derive(Resource)]
pub struct AvatarMeshes {
    /// Inner and outer box for each [`BodyPart`], in that order.
    parts: [(Handle<Mesh>, Handle<Mesh>); 6],
    helmet: Handle<Mesh>,
    chestplate: Handle<Mesh>,
    legging: Handle<Mesh>,
    boot: Handle<Mesh>,
}

/// A part's size in skin pixels (width, height, depth).
fn part_size(part: BodyPart) -> Vec3 {
    let b = part.inner();
    Vec3::new(b.w as f32, b.h as f32, b.d as f32)
}

fn create_avatar_meshes(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let parts = BodyPart::ALL.map(|part| {
        // The hat sits half a pixel out; the other outer layers a quarter.
        let grow = if part == BodyPart::Head { 0.5 } else { 0.25 };
        let size = part_size(part) * PIXEL;
        (
            meshes.add(skin_box_mesh(size, part.inner())),
            meshes.add(skin_box_mesh(
                size + Vec3::splat(2.0 * grow * PIXEL),
                part.outer(),
            )),
        )
    });
    commands.insert_resource(AvatarMeshes {
        parts,
        helmet: meshes.add(Cuboid::new(0.54, 0.28, 0.54)),
        chestplate: meshes.add(Cuboid::new(0.52, 0.6, 0.3)),
        legging: meshes.add(Cuboid::new(0.26, 0.45, 0.26)),
        boot: meshes.add(Cuboid::new(0.26, 0.2, 0.28)),
    });
}

/// A box of `size` (in blocks) centred on the origin, with each face cut
/// from the skin pattern `b`. The model faces -Z, and the player's right is
/// +X.
pub fn skin_box_mesh(size: Vec3, b: SkinBox) -> Mesh {
    let h = size / 2.0;
    let (x, y, z) = (h.x, h.y, h.z);
    // Each face's corners as seen from outside: top left, top right,
    // bottom right, bottom left of its picture.
    let corners = |face: Face| -> ([[f32; 3]; 4], [f32; 3]) {
        match face {
            Face::Front => (
                [[x, y, -z], [-x, y, -z], [-x, -y, -z], [x, -y, -z]],
                [0., 0., -1.],
            ),
            Face::Back => (
                [[-x, y, z], [x, y, z], [x, -y, z], [-x, -y, z]],
                [0., 0., 1.],
            ),
            Face::Right => (
                [[x, y, z], [x, y, -z], [x, -y, -z], [x, -y, z]],
                [1., 0., 0.],
            ),
            Face::Left => (
                [[-x, y, -z], [-x, y, z], [-x, -y, z], [-x, -y, -z]],
                [-1., 0., 0.],
            ),
            Face::Top => (
                [[x, y, z], [-x, y, z], [-x, y, -z], [x, y, -z]],
                [0., 1., 0.],
            ),
            Face::Bottom => (
                [[x, -y, -z], [-x, -y, -z], [-x, -y, z], [x, -y, z]],
                [0., -1., 0.],
            ),
        }
    };
    let mut positions = Vec::with_capacity(24);
    let mut normals = Vec::with_capacity(24);
    let mut uvs = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    let s = SKIN_SIZE as f32;
    for (face, (fx, fy, fw, fh)) in Face::ALL.into_iter().zip(face_rects(b)) {
        let (quad, normal) = corners(face);
        let base = positions.len() as u32;
        let (u0, v0) = (fx as f32 / s, fy as f32 / s);
        let (u1, v1) = ((fx + fw) as f32 / s, (fy + fh) as f32 / s);
        positions.extend(quad);
        normals.extend([normal; 4]);
        uvs.extend([[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
        // Counter-clockwise seen from outside.
        indices.extend([base, base + 3, base + 2, base, base + 2, base + 1]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Spawn an avatar model (hidden until its owner shows it). The caller
/// adds its own components to the returned root entity.
pub fn spawn_avatar(
    commands: &mut Commands,
    meshes: &AvatarMeshes,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let skinned = |materials: &mut Assets<StandardMaterial>, outer: bool| {
        materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            alpha_mode: if outer {
                AlphaMode::Mask(0.5)
            } else {
                AlphaMode::Opaque
            },
            // The outer layer is seen from inside too (the far side of a hat).
            cull_mode: if outer {
                None
            } else {
                Some(bevy::render::render_resource::Face::Back)
            },
            ..default()
        })
    };
    let mats = AvatarMaterials {
        skin: skinned(materials, false),
        outer: skinned(materials, true),
        shirt: skinned(materials, false),
    };
    let skin = (MeshMaterial3d(mats.skin.clone()), AvatarColor(Color::WHITE));
    let outer = (
        MeshMaterial3d(mats.outer.clone()),
        AvatarColor(Color::WHITE),
    );
    let shirt = (
        MeshMaterial3d(mats.shirt.clone()),
        AvatarColor(Color::WHITE),
    );
    let mesh = |part: BodyPart| meshes.parts[part as usize].clone();

    // Armour, hidden until worn: (slot, mesh, where).
    let mut armour = |slot: usize, mesh: &Handle<Mesh>, at: Vec3| {
        let color = Color::WHITE;
        (
            ArmourPiece(slot),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                ..default()
            })),
            AvatarColor(color),
            Transform::from_translation(at),
            Visibility::Hidden,
        )
    };
    let leg_len = 12.0 * PIXEL;
    let helmet = armour(0, &meshes.helmet, Vec3::new(0.0, 0.36, 0.0));
    let chestplate = armour(1, &meshes.chestplate, Vec3::new(0.0, leg_len + 0.33, 0.0));
    let legs = [
        armour(2, &meshes.legging, Vec3::new(0.0, -0.2, 0.0)),
        armour(2, &meshes.legging, Vec3::new(0.0, -0.2, 0.0)),
    ];
    let boots = [
        armour(3, &meshes.boot, Vec3::new(0.0, -0.58, -0.01)),
        armour(3, &meshes.boot, Vec3::new(0.0, -0.58, -0.01)),
    ];

    commands
        .spawn((
            AvatarPose::default(),
            AvatarLook::default(),
            AvatarArmour::default(),
            Transform::default(),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            // The body. The model faces -Z, like the camera.
            let (inner, jacket) = mesh(BodyPart::Body);
            let body_at = Vec3::new(0.0, leg_len + 6.0 * PIXEL, 0.0);
            root.spawn((Mesh3d(inner), shirt, Transform::from_translation(body_at)));
            root.spawn((
                Mesh3d(jacket),
                outer.clone(),
                Jacket,
                Transform::from_translation(body_at),
            ));
            root.spawn(chestplate);
            // The head turns around the neck.
            let (inner, hat) = mesh(BodyPart::Head);
            root.spawn((
                AvatarHead,
                Transform::from_xyz(0.0, leg_len + 12.0 * PIXEL, 0.0),
                Visibility::Inherited,
            ))
            .with_children(|head| {
                let at = Transform::from_xyz(0.0, 4.0 * PIXEL, 0.0);
                head.spawn((Mesh3d(inner), skin.clone(), at));
                head.spawn((Mesh3d(hat), outer.clone(), at));
                head.spawn(helmet);
            });
            // Arms pivot at the shoulder and legs at the hip, with the box
            // hanging below. The player's right is +X.
            let shoulder = leg_len + 10.0 * PIXEL;
            let limbs = [
                (
                    BodyPart::RightArm,
                    Vec3::new(6.0 * PIXEL, shoulder, 0.0),
                    -1.0,
                    4.0,
                ),
                (
                    BodyPart::LeftArm,
                    Vec3::new(-6.0 * PIXEL, shoulder, 0.0),
                    1.0,
                    4.0,
                ),
                (
                    BodyPart::RightLeg,
                    Vec3::new(2.0 * PIXEL, leg_len, 0.0),
                    1.0,
                    6.0,
                ),
                (
                    BodyPart::LeftLeg,
                    Vec3::new(-2.0 * PIXEL, leg_len, 0.0),
                    -1.0,
                    6.0,
                ),
            ];
            let [leg_a, leg_b] = legs;
            let [boot_a, boot_b] = boots;
            let mut worn = [None, None, Some((leg_a, boot_a)), Some((leg_b, boot_b))];
            for (i, (part, pivot, sign, drop)) in limbs.into_iter().enumerate() {
                let (inner, sleeve) = mesh(part);
                let at = Transform::from_xyz(0.0, -drop * PIXEL, 0.0);
                let mut limb = root.spawn((
                    Limb { sign },
                    Transform::from_translation(pivot),
                    Visibility::Inherited,
                ));
                limb.with_child((Mesh3d(inner), skin.clone(), at));
                limb.with_child((Mesh3d(sleeve), outer.clone(), at));
                // Leggings and boots go on the legs.
                if let Some((leg, boot)) = worn[i].take() {
                    limb.with_child(leg);
                    limb.with_child(boot);
                }
            }
        })
        .insert(mats)
        .id()
}

/// Put each avatar's skin on, and paint team shirts.
fn dress_avatars(
    avatars: Query<(&AvatarLook, &AvatarMaterials, &Children), Changed<AvatarLook>>,
    mut skins: ResMut<SkinImages>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut parts: Query<&mut AvatarColor>,
    mut jackets: Query<&mut Visibility, With<Jacket>>,
    shirts: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    for (look, mats, children) in &avatars {
        let texture = skins.get(&look.skin, &mut images);
        for m in [&mats.skin, &mats.outer] {
            if let Some(mut m) = materials.get_mut(m) {
                m.base_color_texture = Some(texture.clone());
            }
        }
        if let Some(mut m) = materials.get_mut(&mats.shirt) {
            m.base_color_texture = look.team.is_none().then(|| texture.clone());
        }
        for child in children.iter() {
            if let Ok(mut vis) = jackets.get_mut(child) {
                *vis = if look.team.is_some() {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                };
            }
            if shirts.get(child).is_ok_and(|m| m.0 == mats.shirt)
                && let Ok(mut c) = parts.get_mut(child)
            {
                c.0 = look.team.unwrap_or(Color::WHITE);
            }
        }
    }
}

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

/// Show the armour each avatar wears.
fn show_armour(
    items: Res<crate::items::Items>,
    avatars: Query<(Entity, &AvatarArmour), Changed<AvatarArmour>>,
    children: Query<&Children>,
    mut pieces: Query<(&ArmourPiece, &mut Visibility, &mut AvatarColor)>,
) {
    for (root, worn) in &avatars {
        for e in children.iter_descendants(root) {
            let Ok((piece, mut vis, mut colour)) = pieces.get_mut(e) else {
                continue;
            };
            let item = worn.0[piece.0];
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

/// Turn heads to where avatars look, and swing arms and legs as they walk.
fn animate_avatars(
    time: Res<Time>,
    mut avatars: Query<(&mut AvatarPose, &Children)>,
    mut limbs: Query<(&Limb, &mut Transform), Without<AvatarHead>>,
    mut heads: Query<&mut Transform, With<AvatarHead>>,
) {
    for (mut pose, children) in &mut avatars {
        let speed = Vec2::new(pose.velocity.x, pose.velocity.z).length();
        let walking = !pose.flying && speed > 0.3;
        pose.stride += speed * time.delta_secs() * 2.2;
        let swing = if walking {
            pose.stride.sin() * 0.7
        } else {
            0.0
        };
        for child in children.iter() {
            if let Ok((limb, mut t)) = limbs.get_mut(child) {
                t.rotation = Quat::from_rotation_x(swing * limb.sign);
            }
            if let Ok(mut head) = heads.get_mut(child) {
                head.rotation = Quat::from_rotation_x(pose.pitch.clamp(-1.2, 1.2));
            }
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
        let c = color.0.to_linear();
        let shaded = Color::LinearRgba(LinearRgba::new(
            c.red * light,
            c.green * light,
            c.blue * light,
            1.0,
        ));
        // Several parts share a material; only touch it when it changes
        // (each change sends the material to the GPU again).
        if materials
            .get(&mat.0)
            .is_some_and(|m| m.base_color != shaded)
            && let Some(mut m) = materials.get_mut(&mat.0)
        {
            m.base_color = shaded;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn the_face_is_on_the_front() {
        let mesh = skin_box_mesh(Vec3::splat(1.0), BodyPart::Head.inner());
        let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            panic!("no uvs");
        };
        assert_eq!(pos.len(), 24);
        // The front face (towards -Z) shows the face picture (8,8)-(16,16),
        // with the player's right (+X) on the picture's left.
        let front = Face::Front as usize * 4;
        assert!(pos[front..front + 4].iter().all(|p| p[2] < 0.0));
        assert_eq!(uv[front], [8.0 / 64.0, 8.0 / 64.0]);
        assert!(pos[front][0] > 0.0 && pos[front][1] > 0.0);
        assert_eq!(uv[front + 2], [16.0 / 64.0, 16.0 / 64.0]);
    }

    #[test]
    fn the_model_is_as_tall_as_the_player() {
        let legs = BodyPart::RightLeg.inner().h;
        let body = BodyPart::Body.inner().h;
        let head = BodyPart::Head.inner().h;
        let height = (legs + body + head) as f32 * PIXEL;
        assert!((height - ourcraft_common::physics::PLAYER_HEIGHT).abs() < 1e-4);
    }
}

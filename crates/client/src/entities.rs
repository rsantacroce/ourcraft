//! Entities from game-mode mods (projectiles, flags…) and particle effects.
//!
//! Entity *models* are lists of coloured boxes sent in the server's content
//! pack. Projectiles are moved here every frame with the same gravity the
//! server uses, so they fly smoothly; the server just says when they appear
//! and disappear.

use std::collections::HashMap;

use bevy::prelude::*;
use ourcraft_common::item::ItemId;
use ourcraft_common::mobs::{BABY_SCALE, MobDef, parse_mob_model};
use ourcraft_common::protocol::ServerMessage;
use ourcraft_mod_api::{EffectKind, ModelSpec};

use crate::net::{FromServer, NetReceiveSet};

pub struct EntitiesPlugin;

impl Plugin for EntitiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Models>()
            .init_resource::<EntityMap>()
            .init_resource::<LitVariants>()
            .insert_resource(MobDefs(
                ourcraft_common::mobs::builtin_mobs()
                    .into_iter()
                    .map(|m| (m.name.clone(), m))
                    .collect(),
            ))
            .add_systems(Startup, setup_particle_mesh)
            .add_systems(PreUpdate, handle_messages.after(NetReceiveSet))
            .add_systems(
                Update,
                (
                    move_entities,
                    animate_legs,
                    bob_items,
                    update_particles,
                    light_entities,
                )
                    .chain(),
            )
            .add_systems(
                PreUpdate,
                reset_entities.run_if(on_message::<crate::net::SessionEnded>),
            );
    }
}

/// Models from the server, by name.
#[derive(Resource, Default)]
struct Models(HashMap<String, ModelSpec>);

/// Every kind of animal, by name (from the server's content pack).
#[derive(Resource, Default)]
pub struct MobDefs(pub HashMap<String, MobDef>);

/// Server entity id → our entity.
#[derive(Resource, Default)]
struct EntityMap(HashMap<u64, Entity>);

#[derive(Component)]
struct NetEntity {
    velocity: Vec3,
    /// Projectiles fall with this gravity; `None` = placed by the server.
    gravity: Option<f32>,
    /// Where the server last said it is (for smoothing fixed entities).
    target: Vec3,
    /// Which way the server says it faces.
    yaw: f32,
    spin: bool,
    age: f32,
    /// How fast it moved this frame (blocks per second), for walking legs.
    speed: f32,
    /// How far along the walking cycle the legs are.
    stride: f32,
}

/// An animal: lets the player aim at it and use it.
#[derive(Component, Debug, Clone)]
pub struct Mob {
    /// The server's entity id.
    pub id: u64,
    pub kind: String,
    /// Width and height (already scaled for babies).
    pub size: [f32; 2],
    pub baby: bool,
    pub sheared: bool,
}

/// A leg (or anything else) that swings while the animal walks.
#[derive(Component)]
struct MobLeg {
    swing: f32,
}

/// One box of an animal, ready to spawn.
#[derive(Clone)]
struct PartLook {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    /// Where the box's centre is (or, for legs, where its hip is).
    at: Vec3,
    /// For legs: from the hip down to the box centre.
    hang: Vec3,
    swing: f32,
}

/// How brightly an entity is lit where it stands (like the terrain
/// around it: darker at night, in caves and under roofs, warm near
/// torches). Brightness steps go from 0 (darkest) to [`LIGHT_STEPS`].
#[derive(Component, Default)]
struct EntityLight {
    step: Option<u8>,
    /// Seconds until it's worked out again.
    next: f32,
}

const LIGHT_STEPS: u8 = 16;

/// A box of an entity, drawn with `base` made brighter or darker.
#[derive(Component)]
struct Lit(Handle<StandardMaterial>);

/// Darker and lighter copies of each entity material, by brightness step.
#[derive(Resource, Default)]
struct LitVariants(HashMap<(AssetId<StandardMaterial>, u8), Handle<StandardMaterial>>);

/// Brightness (0–1) for sky and block light levels, like the terrain
/// shader (`voxel.wgsl`).
fn brightness(sky: u8, block: u8, daylight: f32) -> f32 {
    let level = |l: u8| 0.82f32.powf(15.0 - l as f32);
    (level(sky) * daylight.max(0.12))
        .max(level(block))
        .max(0.06)
}

#[allow(clippy::too_many_arguments)]
fn light_entities(
    time: Res<Time>,
    tod: Res<crate::sky::TimeOfDay>,
    world: Res<crate::world::ClientWorld>,
    streaming: Res<crate::world::ChunkStreaming>,
    blocks: Res<crate::world::Blocks>,
    mut roots: Query<(Entity, &Transform, &mut EntityLight)>,
    mut parts: Query<(&Lit, &mut MeshMaterial3d<StandardMaterial>, &ChildOf)>,
    parents: Query<&ChildOf>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut variants: ResMut<LitVariants>,
) {
    let dt = time.delta_secs();
    let mut changed: HashMap<Entity, u8> = HashMap::new();
    for (e, t, mut light) in &mut roots {
        light.next -= dt;
        if light.next > 0.0 && light.step.is_some() {
            continue;
        }
        light.next = 0.4;
        let at = t.translation + Vec3::Y * 0.4;
        let (sky, block) = crate::light::light_near(&world.0, &streaming, &blocks.0, at);
        let b = brightness(sky, block, tod.daylight());
        let step = (b * LIGHT_STEPS as f32).round() as u8;
        if light.step != Some(step) {
            light.step = Some(step);
            changed.insert(e, step);
        }
    }
    if changed.is_empty() {
        return;
    }
    for (lit, mut material, child_of) in &mut parts {
        // Parts hang off the entity directly, or off a leg.
        let mut up = child_of.parent();
        let mut step = changed.get(&up);
        if step.is_none()
            && let Ok(c) = parents.get(up)
        {
            up = c.parent();
            step = changed.get(&up);
        }
        let Some(&step) = step else { continue };
        let key = (lit.0.id(), step);
        let handle = match variants.0.get(&key) {
            Some(h) => h.clone(),
            None => {
                let Some(base) = materials.get(&lit.0).cloned() else {
                    continue;
                };
                let f = step as f32 / LIGHT_STEPS as f32;
                let c = base.base_color.to_linear();
                let h = materials.add(StandardMaterial {
                    base_color: Color::LinearRgba(LinearRgba::new(
                        c.red * f,
                        c.green * f,
                        c.blue * f,
                        c.alpha,
                    )),
                    ..base
                });
                variants.0.insert(key, h.clone());
                h
            }
        };
        material.0 = handle;
    }
}

/// A box whose faces are shaded like the terrain's: tops brightest, sides
/// a bit darker, bottoms darkest (we have no lights, so this gives shape).
pub fn shaded_cuboid(size: Vec3) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::from_size(size));
    let normals: Vec<[f32; 3]> = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) => n.clone(),
        _ => return mesh,
    };
    let colors: Vec<[f32; 4]> = normals
        .iter()
        .map(|n| {
            let shade = if n[1] > 0.5 {
                1.0
            } else if n[1] < -0.5 {
                0.5
            } else if n[0].abs() > 0.5 {
                0.8
            } else {
                0.68
            };
            [shade, shade, shade, 1.0]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh
}

/// Build the boxes for an animal kind (with or without wool).
fn mob_look(
    def: &MobDef,
    sheared: bool,
    colour: u8,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Vec<PartLook> {
    def.parts
        .iter()
        .map(|p| {
            let (mut min, mut max) = (Vec3::from_array(p.min), Vec3::from_array(p.max));
            let mut rgba = p.color;
            if p.wool && sheared {
                // Without wool: thinner, and skin-coloured.
                min += Vec3::splat(0.05);
                max -= Vec3::splat(0.05);
                rgba = def.skin;
            } else if p.wool && colour > 0 {
                let [r, g, b] = ourcraft_common::colours::COLOURS[colour as usize % 16].2;
                rgba = [r, g, b, 255];
            }
            let size = (max - min).abs().max(Vec3::splat(0.01));
            let center = (min + max) / 2.0;
            let (at, hang) = if p.leg != 0.0 {
                let hip = Vec3::new(center.x, max.y, center.z);
                (hip, center - hip)
            } else {
                (center, Vec3::ZERO)
            };
            PartLook {
                mesh: meshes.add(shaded_cuboid(size)),
                material: materials.add(StandardMaterial {
                    base_color: color(rgba),
                    // No lights in our world: shaded by hand
                    // (`light_entities`).
                    unlit: true,
                    ..default()
                }),
                at,
                hang,
                swing: p.leg,
            }
        })
        .collect()
}

/// The visible part of a dropped item, which bobs up and down.
#[derive(Component)]
struct ItemBob;

#[derive(Component)]
struct Particle {
    velocity: Vec3,
    life: f32,
    max_life: f32,
    gravity: f32,
}

#[derive(Resource)]
struct ParticleMesh(Handle<Mesh>);

fn setup_particle_mesh(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(ParticleMesh(meshes.add(Cuboid::new(0.12, 0.12, 0.12))));
}

fn color(rgba: [u8; 4]) -> Color {
    Color::srgba_u8(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// A tiny random number generator for particle scatter.
fn rand(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed % 10_000) as f32 / 10_000.0
}

/// The mesh and material of a dropped item: a little cube with the block's
/// texture, or a flat picture for other items.
fn item_look(
    item: ItemId,
    items: &ourcraft_common::item::ItemRegistry,
    blocks: &ourcraft_common::block::BlockRegistry,
    textures: &crate::render::ModTextures,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> (Handle<Mesh>, Handle<StandardMaterial>) {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::ImageSampler;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let image = |pixels: Vec<u8>, size: u32| {
        let mut img = Image::new(
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = ImageSampler::nearest();
        img
    };
    let block_tex = items
        .get(item)
        .and_then(|d| d.block())
        .filter(|b| blocks.get(*b).shape.is_cube())
        .and_then(|b| blocks.get(b).textures.clone());
    if let Some(tex) = block_tex {
        let pixels = textures.pixels(&tex.side);
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image(pixels, 16))),
            alpha_mode: AlphaMode::Mask(0.5),
            unlit: true,
            ..default()
        });
        return (meshes.add(shaded_cuboid(Vec3::splat(0.25))), material);
    }
    let pixels = crate::inventory_ui::item_icon_pixels(items, blocks, textures, item)
        .unwrap_or_else(|| vec![255; 32 * 32 * 4]);
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(image(pixels, 32))),
        alpha_mode: AlphaMode::Mask(0.5),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    (meshes.add(Rectangle::new(0.4, 0.4)), material)
}

/// Things that dropped items need to look right.
#[derive(bevy::ecs::system::SystemParam)]
struct ItemLooks<'w, 's> {
    items: Res<'w, crate::items::Items>,
    blocks: Res<'w, crate::world::Blocks>,
    textures: Option<Res<'w, crate::render::ModTextures>>,
    images: ResMut<'w, Assets<Image>>,
    cache: Local<'s, HashMap<ItemId, (Handle<Mesh>, Handle<StandardMaterial>)>>,
    mobs: ResMut<'w, MobDefs>,
    mob_cache: Local<'s, HashMap<(String, bool, u8), Vec<PartLook>>>,
    variants: ResMut<'w, LitVariants>,
}

#[allow(clippy::too_many_arguments)]
fn handle_messages(
    mut commands: Commands,
    mut messages: MessageReader<FromServer>,
    mut models: ResMut<Models>,
    mut map: ResMut<EntityMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    particle_mesh: Res<ParticleMesh>,
    mut entities: Query<(&mut NetEntity, &mut Transform)>,
    mut looks: ItemLooks,
    mut seed: Local<u32>,
) {
    if *seed == 0 {
        *seed = 0x1234_5678;
    }
    for FromServer(msg) in messages.read() {
        match msg {
            ServerMessage::Welcome { content, .. } => {
                for (_, e) in map.0.drain() {
                    commands.entity(e).despawn();
                }
                looks.cache.clear();
                looks.mob_cache.clear();
                looks.variants.0.clear();
                looks.mobs.0 = content
                    .mobs
                    .iter()
                    .map(|m| (m.name.clone(), m.clone()))
                    .collect();
                models.0 = content
                    .models
                    .iter()
                    .map(|m| (m.name.clone(), m.clone()))
                    .collect();
            }
            ServerMessage::EntitySpawned {
                id,
                model,
                position,
                velocity,
                gravity,
                yaw,
            } => {
                let pos = Vec3::from_array(*position);
                let new_net = |gravity: Option<f32>, velocity: Vec3, spin: bool| NetEntity {
                    velocity,
                    gravity,
                    target: pos,
                    yaw: *yaw,
                    spin,
                    age: 0.0,
                    speed: 0.0,
                    stride: 0.0,
                };
                // An animal ("@mob:<name>[:baby][:sheared]").
                if let Some((name, baby, sheared)) = parse_mob_model(model) {
                    let looks = &mut looks;
                    let Some(def) = looks.mobs.0.get(name).cloned() else {
                        continue;
                    };
                    let colour = ourcraft_common::mobs::mob_colour(model);
                    let parts = looks
                        .mob_cache
                        .entry((name.to_owned(), sheared, colour))
                        .or_insert_with(|| {
                            mob_look(&def, sheared, colour, &mut meshes, &mut materials)
                        })
                        .clone();
                    let scale = if baby { BABY_SCALE } else { 1.0 };
                    let e = commands
                        .spawn((
                            new_net(None, Vec3::ZERO, false),
                            EntityLight::default(),
                            Mob {
                                id: *id,
                                kind: name.to_owned(),
                                size: [def.size[0] * scale, def.size[1] * scale],
                                baby,
                                sheared,
                            },
                            Transform::from_translation(pos)
                                .with_rotation(Quat::from_rotation_y(*yaw))
                                .with_scale(Vec3::splat(scale)),
                            Visibility::Inherited,
                        ))
                        .with_children(|parent| {
                            for p in parts {
                                if p.swing != 0.0 {
                                    parent
                                        .spawn((
                                            MobLeg { swing: p.swing },
                                            Transform::from_translation(p.at),
                                            Visibility::Inherited,
                                        ))
                                        .with_child((
                                            Mesh3d(p.mesh),
                                            Lit(p.material.clone()),
                                            MeshMaterial3d(p.material),
                                            Transform::from_translation(p.hang),
                                        ));
                                } else {
                                    parent.spawn((
                                        Mesh3d(p.mesh),
                                        Lit(p.material.clone()),
                                        MeshMaterial3d(p.material),
                                        Transform::from_translation(p.at),
                                    ));
                                }
                            }
                        })
                        .id();
                    if let Some(old) = map.0.insert(*id, e) {
                        commands.entity(old).despawn();
                    }
                    continue;
                }
                // A dropped item ("@item:<number>"): drawn from the item list.
                if let Some(item) = model
                    .strip_prefix("@item:")
                    .and_then(|n| n.parse::<u16>().ok())
                    .map(ItemId)
                {
                    let looks = &mut looks;
                    let textures = looks.textures.as_deref().cloned().unwrap_or_default();
                    let (mesh, material) = looks
                        .cache
                        .entry(item)
                        .or_insert_with(|| {
                            item_look(
                                item,
                                &looks.items,
                                &looks.blocks.0,
                                &textures,
                                &mut meshes,
                                &mut materials,
                                &mut looks.images,
                            )
                        })
                        .clone();
                    let e = commands
                        .spawn((
                            new_net(None, Vec3::ZERO, true),
                            EntityLight::default(),
                            Transform::from_translation(pos),
                            Visibility::Inherited,
                        ))
                        .with_child((
                            Mesh3d(mesh),
                            Lit(material.clone()),
                            MeshMaterial3d(material),
                            Transform::default(),
                            ItemBob,
                        ))
                        .id();
                    if let Some(old) = map.0.insert(*id, e) {
                        commands.entity(old).despawn();
                    }
                    continue;
                }
                let spec = models.0.get(model);
                let e = commands
                    .spawn((
                        new_net(
                            *gravity,
                            Vec3::from_array(*velocity),
                            spec.is_some_and(|s| s.spin),
                        ),
                        EntityLight::default(),
                        Transform::from_translation(pos),
                        Visibility::Inherited,
                    ))
                    .with_children(|parent| {
                        let Some(spec) = spec else {
                            // Unknown model: a small magenta cube.
                            parent.spawn((
                                Mesh3d(meshes.add(Cuboid::new(0.3, 0.3, 0.3))),
                                MeshMaterial3d(materials.add(StandardMaterial {
                                    base_color: Color::srgb(1.0, 0.0, 1.0),
                                    unlit: true,
                                    ..default()
                                })),
                            ));
                            return;
                        };
                        for b in &spec.boxes {
                            let (min, max) = (Vec3::from_array(b.min), Vec3::from_array(b.max));
                            let size = (max - min).abs().max(Vec3::splat(0.01));
                            let material = materials.add(StandardMaterial {
                                base_color: color(b.color),
                                unlit: true,
                                ..default()
                            });
                            parent.spawn((
                                Mesh3d(meshes.add(Cuboid::from_size(size))),
                                Lit(material.clone()),
                                MeshMaterial3d(material),
                                Transform::from_translation((min + max) / 2.0),
                            ));
                        }
                    })
                    .id();
                if let Some(old) = map.0.insert(*id, e) {
                    commands.entity(old).despawn();
                }
            }
            ServerMessage::EntitiesMoved(list) => {
                for (id, pos, yaw) in list {
                    if let Some(&e) = map.0.get(id)
                        && let Ok((mut ne, _)) = entities.get_mut(e)
                    {
                        ne.target = Vec3::from_array(*pos);
                        ne.yaw = *yaw;
                    }
                }
            }
            ServerMessage::EntityRemoved(id) => {
                if let Some(e) = map.0.remove(id) {
                    commands.entity(e).despawn();
                }
            }
            ServerMessage::Effect {
                kind,
                position,
                color: rgba,
            } => {
                let (count, speed, life) = match kind {
                    EffectKind::Splat => (14, 4.0, 0.5),
                    EffectKind::Confetti => (40, 7.0, 1.2),
                    EffectKind::Poof => (24, 2.5, 0.8),
                    EffectKind::Hearts => (7, 1.2, 1.2),
                };
                let at = Vec3::from_array(*position);
                for i in 0..count {
                    let dir = Vec3::new(
                        rand(&mut seed) * 2.0 - 1.0,
                        rand(&mut seed) * 1.5,
                        rand(&mut seed) * 2.0 - 1.0,
                    )
                    .normalize_or_zero();
                    // Confetti is every colour of the rainbow; others use the
                    // given colour (poof is white-ish).
                    let c = match kind {
                        EffectKind::Confetti => Color::hsl((i * 37 % 360) as f32, 0.8, 0.6),
                        EffectKind::Poof => Color::srgba(0.95, 0.95, 0.95, 1.0),
                        EffectKind::Splat | EffectKind::Hearts => color(*rgba),
                    };
                    // Hearts float gently upwards instead of falling.
                    let (dir, gravity) = if *kind == EffectKind::Hearts {
                        (Vec3::new(dir.x * 0.6, 1.5, dir.z * 0.6), -1.0)
                    } else {
                        (dir, 12.0)
                    };
                    commands.spawn((
                        Particle {
                            velocity: dir * speed * (0.5 + rand(&mut seed)),
                            life,
                            max_life: life,
                            gravity,
                        },
                        Mesh3d(particle_mesh.0.clone()),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: c,
                            unlit: true,
                            ..default()
                        })),
                        Transform::from_translation(at),
                    ));
                }
            }
            _ => {}
        }
    }
}

fn reset_entities(
    mut commands: Commands,
    mut map: ResMut<EntityMap>,
    particles: Query<Entity, With<Particle>>,
) {
    for (_, e) in map.0.drain() {
        commands.entity(e).despawn();
    }
    for e in &particles {
        commands.entity(e).despawn();
    }
}

fn move_entities(time: Res<Time>, mut q: Query<(&mut NetEntity, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut e, mut t) in &mut q {
        e.age += dt;
        match e.gravity {
            Some(g) => {
                e.velocity.y -= g * dt;
                let v = e.velocity;
                t.translation += v * dt;
            }
            None => {
                // Glide towards the latest position from the server.
                let blend = 1.0 - (-15.0 * dt).exp();
                let before = t.translation;
                t.translation = t.translation.lerp(e.target, blend);
                let moved = (t.translation - before).with_y(0.0).length();
                e.speed = if dt > 0.0 { moved / dt } else { 0.0 };
            }
        }
        if e.spin {
            t.rotation = Quat::from_rotation_y(e.age * 1.5);
        } else if e.gravity.is_none() {
            // Turn smoothly to face where the server says.
            let target = Quat::from_rotation_y(e.yaw);
            t.rotation = t.rotation.slerp(target, 1.0 - (-10.0 * dt).exp());
        }
    }
}

/// Swing legs while walking (faster walking, faster swinging).
fn animate_legs(
    time: Res<Time>,
    mut animals: Query<(&mut NetEntity, &Children), With<Mob>>,
    mut legs: Query<(&MobLeg, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut e, children) in &mut animals {
        let walking = (e.speed / 1.2).min(1.0);
        e.stride += dt * 9.0 * walking.max(0.0);
        let angle = e.stride.sin() * 0.6 * walking;
        for child in children.iter() {
            if let Ok((leg, mut t)) = legs.get_mut(child) {
                t.rotation = Quat::from_rotation_x(angle * leg.swing);
            }
        }
    }
}

fn bob_items(time: Res<Time>, mut q: Query<&mut Transform, With<ItemBob>>) {
    let t = time.elapsed_secs();
    for mut tr in &mut q {
        tr.translation.y = (t * 2.5).sin() * 0.06 + 0.06;
    }
}

fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Particle, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut p, mut t) in &mut q {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        p.velocity.y -= p.gravity * dt;
        let v = p.velocity;
        t.translation += v * dt;
        t.scale = Vec3::splat((p.life / p.max_life).max(0.2));
    }
}

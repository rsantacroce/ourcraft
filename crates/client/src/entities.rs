//! Entities from game-mode mods (projectiles, flags…) and particle effects.
//!
//! Entity *models* are lists of coloured boxes sent in the server's content
//! pack. Projectiles are moved here every frame with the same gravity the
//! server uses, so they fly smoothly; the server just says when they appear
//! and disappear.

use std::collections::HashMap;

use bevy::prelude::*;
use ourcraft_common::item::ItemId;
use ourcraft_common::protocol::ServerMessage;
use ourcraft_mod_api::{EffectKind, ModelSpec};

use crate::net::{FromServer, NetReceiveSet};

pub struct EntitiesPlugin;

impl Plugin for EntitiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Models>()
            .init_resource::<EntityMap>()
            .add_systems(Startup, setup_particle_mesh)
            .add_systems(PreUpdate, handle_messages.after(NetReceiveSet))
            .add_systems(Update, (move_entities, bob_items, update_particles))
            .add_systems(
                PreUpdate,
                reset_entities.run_if(on_message::<crate::net::SessionEnded>),
            );
    }
}

/// Models from the server, by name.
#[derive(Resource, Default)]
struct Models(HashMap<String, ModelSpec>);

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
    spin: bool,
    age: f32,
}

/// The visible part of a dropped item, which bobs up and down.
#[derive(Component)]
struct ItemBob;

#[derive(Component)]
struct Particle {
    velocity: Vec3,
    life: f32,
    max_life: f32,
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
        .and_then(|b| blocks.get(b).textures.clone());
    if let Some(tex) = block_tex {
        let pixels = textures.pixels(&tex.side);
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image(pixels, 16))),
            alpha_mode: AlphaMode::Mask(0.5),
            unlit: true,
            ..default()
        });
        return (meshes.add(Cuboid::new(0.25, 0.25, 0.25)), material);
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
            } => {
                let pos = Vec3::from_array(*position);
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
                            NetEntity {
                                velocity: Vec3::ZERO,
                                gravity: None,
                                target: pos,
                                spin: true,
                                age: 0.0,
                            },
                            Transform::from_translation(pos),
                            Visibility::Inherited,
                        ))
                        .with_child((
                            Mesh3d(mesh),
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
                        NetEntity {
                            velocity: Vec3::from_array(*velocity),
                            gravity: *gravity,
                            target: pos,
                            spin: spec.is_some_and(|s| s.spin),
                            age: 0.0,
                        },
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
                            parent.spawn((
                                Mesh3d(meshes.add(Cuboid::from_size(size))),
                                MeshMaterial3d(materials.add(StandardMaterial {
                                    base_color: color(b.color),
                                    unlit: true,
                                    ..default()
                                })),
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
                for (id, pos) in list {
                    if let Some(&e) = map.0.get(id)
                        && let Ok((mut ne, _)) = entities.get_mut(e)
                    {
                        ne.target = Vec3::from_array(*pos);
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
                        EffectKind::Splat => color(*rgba),
                    };
                    commands.spawn((
                        Particle {
                            velocity: dir * speed * (0.5 + rand(&mut seed)),
                            life,
                            max_life: life,
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
                t.translation = t.translation.lerp(e.target, blend);
            }
        }
        if e.spin {
            t.rotation = Quat::from_rotation_y(e.age * 1.5);
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
        p.velocity.y -= 12.0 * dt;
        let v = p.velocity;
        t.translation += v * dt;
        t.scale = Vec3::splat((p.life / p.max_life).max(0.2));
    }
}

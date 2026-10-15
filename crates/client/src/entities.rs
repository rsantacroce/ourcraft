//! Entities from game-mode mods (projectiles, flags…) and particle effects.
//!
//! Entity *models* are lists of coloured boxes sent in the server's content
//! pack. Projectiles are moved here every frame with the same gravity the
//! server uses, so they fly smoothly; the server just says when they appear
//! and disappear.

use std::collections::HashMap;

use bevy::prelude::*;
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
            .add_systems(Update, (move_entities, update_particles));
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

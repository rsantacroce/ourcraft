//! Atmosphere: clouds, rain and snow, footsteps, birds and wind, and bits
//! flying off blocks you break.
//!
//! - **Clouds** are flat white blocks drifting high over the world, drawn
//!   around the player and moved along by the wind.
//! - **Weather** comes from the server (rain or not). Where the ground is
//!   snowy it snows instead. Rain dims the sky (see `sky`) and is quiet
//!   indoors: drops only fall where the sky is open.
//! - **Sounds**: footsteps that match the ground, birds by day, water
//!   nearby, wind up high.

use bevy::prelude::*;
use ourcraft_common::block::{BlockId, blocks};
use ourcraft_common::protocol::ServerMessage;
use ourcraft_common::world::WORLD_HEIGHT;

use crate::audio::PlayAmbient;
use crate::net::{FromServer, NetReceiveSet};
use crate::player::{Player, PlayerCamera};
use crate::sky::TimeOfDay;
use crate::world::{Blocks, ClientWorld};

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Weather>()
            .add_message::<BlockBroken>()
            .add_systems(Startup, setup)
            .add_systems(PreUpdate, receive_weather.after(NetReceiveSet))
            .add_systems(
                Update,
                (
                    update_weather,
                    drift_clouds,
                    precipitation,
                    footsteps,
                    ambience,
                    break_bits,
                    move_bits,
                ),
            );
    }
}

/// What the sky is doing, as seen by this player.
#[derive(Resource, Default, Debug)]
pub struct Weather {
    /// What the server says.
    pub raining: bool,
    /// 0 (dry) to 1 (pouring), easing in and out.
    pub rain_amount: f32,
    /// It's snowy where we are: snowflakes instead of rain.
    pub snow: bool,
}

/// A block just broke (for the flying bits).
#[derive(Message, Debug, Clone, Copy)]
pub struct BlockBroken {
    pub pos: IVec3,
    pub block: BlockId,
}

const CLOUD_HEIGHT: f32 = 172.0;
const CLOUD_CELL: f32 = 12.0;
/// Clouds reach this many cells out from the player.
const CLOUD_RANGE: i32 = 26;
/// Wind speed (blocks per second).
const WIND: f32 = 1.2;
const DROPS: usize = 260;

#[derive(Component)]
struct Clouds {
    /// The cell the mesh was built around, and the wind offset then.
    built_at: Option<(IVec2, f32)>,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Drop {
    speed: f32,
}

#[derive(Component)]
struct Bit {
    velocity: Vec3,
    life: f32,
}

#[derive(Resource)]
struct Looks {
    drop_mesh: Handle<Mesh>,
    flake_mesh: Handle<Mesh>,
    rain: Handle<StandardMaterial>,
    snow: Handle<StandardMaterial>,
    bit_mesh: Handle<Mesh>,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.85),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        Name::new("Clouds"),
        Clouds {
            built_at: None,
            material: material.clone(),
        },
        Mesh3d(meshes.add(Mesh::from(Cuboid::new(0.0, 0.0, 0.0)))),
        MeshMaterial3d(material),
        Transform::default(),
    ));
    let looks = Looks {
        drop_mesh: meshes.add(Cuboid::new(0.03, 0.5, 0.03)),
        flake_mesh: meshes.add(Cuboid::new(0.1, 0.1, 0.1)),
        rain: materials.add(StandardMaterial {
            base_color: Color::srgba(0.6, 0.7, 0.95, 0.6),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        snow: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, 0.9),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
        bit_mesh: meshes.add(Cuboid::new(0.1, 0.1, 0.1)),
    };
    for i in 0..DROPS {
        commands.spawn((
            Drop {
                speed: 14.0 + (i % 7) as f32,
            },
            Mesh3d(looks.drop_mesh.clone()),
            MeshMaterial3d(looks.rain.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
    commands.insert_resource(looks);
}

fn receive_weather(mut messages: MessageReader<FromServer>, mut weather: ResMut<Weather>) {
    for FromServer(msg) in messages.read() {
        if let ServerMessage::Weather { raining } = msg {
            weather.raining = *raining;
        }
    }
}

/// The highest block of a column, scanning down from the top.
fn top_block(world: &ClientWorld, x: i32, z: i32) -> Option<(i32, BlockId)> {
    (0..WORLD_HEIGHT)
        .rev()
        .map(|y| (y, world.block(x, y, z)))
        .find(|(_, b)| !b.is_air())
}

fn update_weather(
    time: Res<Time>,
    mut weather: ResMut<Weather>,
    world: Res<ClientWorld>,
    camera: Single<&Transform, With<PlayerCamera>>,
    mut check: Local<f32>,
) {
    let target = if weather.raining { 1.0 } else { 0.0 };
    let step = time.delta_secs() / 6.0; // fades over a few seconds
    weather.rain_amount += (target - weather.rain_amount).clamp(-step, step);
    *check -= time.delta_secs();
    if *check <= 0.0 {
        *check = 2.0;
        let p = camera.translation.floor().as_ivec3();
        let snowy = top_block(&world, p.x, p.z).is_some_and(|(_, b)| b == blocks::SNOW);
        if weather.snow != snowy {
            weather.snow = snowy;
        }
    }
}

/// A pseudo-random number in 0..1 for a cloud cell (the same every time).
fn cell_noise(x: i32, z: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (z as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    ((h ^ (h >> 15)) % 1000) as f32 / 1000.0
}

/// Is there cloud over cell (x, z)? Smoothed noise, so clouds come in
/// clumps rather than speckles.
fn cloudy(x: i32, z: i32, cover: f32) -> bool {
    let mut sum = 0.0;
    for dz in -1..=1 {
        for dx in -1..=1 {
            sum += cell_noise((x + dx).div_euclid(2), (z + dz).div_euclid(2));
        }
    }
    sum / 9.0 > 1.0 - cover
}

/// Rebuild the cloud mesh when we've moved a cell (or the wind has), and
/// slide it along in between.
#[allow(clippy::too_many_arguments)]
fn drift_clouds(
    time: Res<Time>,
    weather: Res<Weather>,
    tod: Res<TimeOfDay>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Clouds>)>,
    mut clouds: Single<(&mut Clouds, &mut Mesh3d, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let wind = time.elapsed_secs() * WIND;
    let (state, mesh, transform) = &mut *clouds;
    let cam = camera.translation;
    let cell = IVec2::new(
        ((cam.x + wind) / CLOUD_CELL).floor() as i32,
        (cam.z / CLOUD_CELL).floor() as i32,
    );
    let cover = 0.32 + 0.3 * weather.rain_amount;
    let rebuild = state.built_at.is_none_or(|(c, _)| c != cell) || weather.is_changed();
    if rebuild {
        let mut builder = Vec::new();
        for dz in -CLOUD_RANGE..=CLOUD_RANGE {
            for dx in -CLOUD_RANGE..=CLOUD_RANGE {
                let (cx, cz) = (cell.x + dx, cell.y + dz);
                if !cloudy(cx, cz, cover) {
                    continue;
                }
                // Cloud cells sit at their wind-free spot; the transform
                // slides them along.
                let min = Vec3::new(
                    cx as f32 * CLOUD_CELL - wind,
                    CLOUD_HEIGHT,
                    cz as f32 * CLOUD_CELL,
                );
                builder.push(min);
            }
        }
        let size = Vec3::new(CLOUD_CELL, 4.0, CLOUD_CELL);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        for min in builder {
            let cuboid = Mesh::from(Cuboid::from_size(size));
            let base = positions.len() as u32;
            if let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) =
                cuboid.attribute(Mesh::ATTRIBUTE_POSITION)
            {
                positions.extend(
                    p.iter()
                        .map(|v| (Vec3::from(*v) + min + size / 2.0).to_array()),
                );
            }
            if let Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) =
                cuboid.attribute(Mesh::ATTRIBUTE_NORMAL)
            {
                normals.extend(n.iter().copied());
            }
            if let Some(i) = cuboid.indices() {
                indices.extend(i.iter().map(|i| base + i as u32));
            }
        }
        let mut m = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        m.insert_indices(bevy::mesh::Indices::U32(indices));
        mesh.0 = meshes.add(m);
        state.built_at = Some((cell, wind));
    }
    let built_wind = state.built_at.map_or(wind, |(_, w)| w);
    transform.translation = Vec3::new(-(wind - built_wind), 0.0, 0.0);

    // White by day, grey in the rain, dark at night.
    if let Some(mut m) = materials.get_mut(&state.material) {
        let light = tod.daylight().max(0.15) * (1.0 - 0.4 * weather.rain_amount);
        m.base_color = Color::srgba(light, light, light * 1.02, 0.85);
    }
}

/// Raindrops (or snowflakes) falling around the player, only where the
/// sky is open.
#[allow(clippy::too_many_arguments)]
fn precipitation(
    time: Res<Time>,
    weather: Res<Weather>,
    looks: Res<Looks>,
    world: Res<ClientWorld>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Drop>)>,
    mut drops: Query<(
        &Drop,
        &mut Transform,
        &mut Visibility,
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    mut seed: Local<u32>,
) {
    if *seed == 0 {
        *seed = 0x2545_f491;
    }
    let mut rand = || {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        (*seed % 10_000) as f32 / 10_000.0
    };
    let active = (DROPS as f32 * weather.rain_amount) as usize;
    let cam = camera.translation;
    let dt = time.delta_secs();
    for (i, (drop, mut t, mut vis, mut mesh, mut mat)) in drops.iter_mut().enumerate() {
        if i >= active {
            *vis = Visibility::Hidden;
            continue;
        }
        let (want_mesh, want_mat, speed) = if weather.snow {
            (&looks.flake_mesh, &looks.snow, drop.speed * 0.12)
        } else {
            (&looks.drop_mesh, &looks.rain, drop.speed)
        };
        if mesh.0 != *want_mesh {
            mesh.0 = want_mesh.clone();
            mat.0 = want_mat.clone();
        }
        t.translation.y -= speed * dt;
        if weather.snow {
            t.translation.x += (time.elapsed_secs() * 1.3 + i as f32).sin() * 0.4 * dt;
        }
        // Respawn above the player when it falls out of view or lands.
        let below = t.translation.floor().as_ivec3();
        let landed = !world.block(below.x, below.y, below.z).is_air();
        if *vis == Visibility::Hidden || landed || t.translation.y < cam.y - 10.0 {
            let x = cam.x + (rand() - 0.5) * 32.0;
            let z = cam.z + (rand() - 0.5) * 32.0;
            let y = cam.y + 4.0 + rand() * 12.0;
            // Not under a roof.
            let open = top_block(&world, x.floor() as i32, z.floor() as i32)
                .is_none_or(|(top, _)| (top as f32) < y);
            t.translation = Vec3::new(x, y, z);
            *vis = if open {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// Which footstep sound a block makes.
fn step_sound(registry: &ourcraft_common::block::BlockRegistry, b: BlockId) -> &'static str {
    let name = &registry.get(b).name;
    if name.contains("snow") || name.contains("wool") {
        "step_snow"
    } else if name.contains("sand") || name.contains("gravel") {
        "step_sand"
    } else if name.contains("plank")
        || name.contains("log")
        || name.contains("door")
        || name.contains("chest")
        || name.contains("table")
    {
        "step_wood"
    } else if name.contains("grass")
        || name.contains("dirt")
        || name.contains("leaves")
        || name.contains("clay")
    {
        "step_grass"
    } else {
        "step_stone"
    }
}

/// A footstep every so often while walking on the ground.
fn footsteps(
    player: Single<&Player>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    mut sounds: MessageWriter<PlayAmbient>,
    mut last: Local<Option<Vec3>>,
    mut walked: Local<f32>,
) {
    let s = &player.state;
    let here = s.position;
    let moved = last.map_or(0.0, |l: Vec3| (here - l).with_y(0.0).length());
    *last = Some(here);
    if !s.on_ground || s.flying || s.in_water || moved > 2.0 {
        return;
    }
    *walked += moved;
    if *walked < 1.7 {
        return;
    }
    *walked = 0.0;
    let under = (here - Vec3::Y * 0.1).floor().as_ivec3();
    let b = world.block(under.x, under.y, under.z);
    if !b.is_air() {
        sounds.write(PlayAmbient {
            name: step_sound(&blocks.0, b),
            gain: 0.3,
        });
    }
}

/// Birds by day where there are trees, water nearby, wind up high.
fn ambience(
    time: Res<Time>,
    tod: Res<TimeOfDay>,
    weather: Res<Weather>,
    world: Res<ClientWorld>,
    camera: Single<&Transform, With<PlayerCamera>>,
    mut sounds: MessageWriter<PlayAmbient>,
    mut wait: Local<f32>,
    mut turn: Local<u32>,
) {
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    *turn = turn.wrapping_add(1);
    *wait = 7.0 + (*turn % 5) as f32 * 3.0;
    let p = camera.translation.floor().as_ivec3();
    let near = |want: &dyn Fn(BlockId) -> bool, r: i32| {
        (-r..=r).step_by(2).any(|dx| {
            (-r..=r)
                .step_by(2)
                .any(|dz| (-3..=3).any(|dy| want(world.block(p.x + dx, p.y + dy, p.z + dz))))
        })
    };
    let outdoors = top_block(&world, p.x, p.z).is_none_or(|(top, _)| top <= p.y);
    if p.y > 110 && outdoors {
        sounds.write(PlayAmbient {
            name: "wind",
            gain: 0.35,
        });
    } else if near(&|b| b == blocks::WATER, 6) {
        sounds.write(PlayAmbient {
            name: "water",
            gain: 0.3,
        });
    } else if tod.daylight() > 0.5
        && weather.rain_amount < 0.3
        && outdoors
        && near(&|b| b == blocks::LEAVES, 8)
    {
        sounds.write(PlayAmbient {
            name: "birds",
            gain: 0.25,
        });
    }
}

/// The colour of a block, from the middle of its side texture.
fn block_colour(
    registry: &ourcraft_common::block::BlockRegistry,
    textures: &crate::render::ModTextures,
    b: BlockId,
) -> Color {
    let Some(tex) = &registry.get(b).textures else {
        return Color::WHITE;
    };
    let px = textures.pixels(&tex.side);
    let (mut r, mut g, mut bl, mut n) = (0u32, 0u32, 0u32, 0u32);
    for p in px.chunks(4).filter(|p| p[3] > 100) {
        r += p[0] as u32;
        g += p[1] as u32;
        bl += p[2] as u32;
        n += 1;
    }
    if n == 0 {
        return Color::WHITE;
    }
    Color::srgb_u8((r / n) as u8, (g / n) as u8, (bl / n) as u8)
}

/// Little bits fly off a broken block.
#[allow(clippy::too_many_arguments)]
fn break_bits(
    mut commands: Commands,
    mut broken: MessageReader<BlockBroken>,
    looks: Res<Looks>,
    blocks: Res<Blocks>,
    textures: Option<Res<crate::render::ModTextures>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut colours: Local<std::collections::HashMap<BlockId, Handle<StandardMaterial>>>,
    mut seed: Local<u32>,
) {
    if *seed == 0 {
        *seed = 0x6a09_e667;
    }
    let textures = textures.map(|t| t.clone()).unwrap_or_default();
    for e in broken.read() {
        let mat = colours
            .entry(e.block)
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: block_colour(&blocks.0, &textures, e.block),
                    unlit: true,
                    ..default()
                })
            })
            .clone();
        let centre = e.pos.as_vec3() + Vec3::splat(0.5);
        for _ in 0..10 {
            let mut r = || {
                *seed ^= *seed << 13;
                *seed ^= *seed >> 17;
                *seed ^= *seed << 5;
                (*seed % 1000) as f32 / 1000.0 - 0.5
            };
            let dir = Vec3::new(r(), r() + 0.6, r());
            let start = centre + Vec3::new(r(), r(), r()) * 0.6;
            commands.spawn((
                Bit {
                    velocity: dir * 4.0,
                    life: 0.6 + r() * 0.3,
                },
                Mesh3d(looks.bit_mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(start),
            ));
        }
    }
}

fn move_bits(
    mut commands: Commands,
    time: Res<Time>,
    mut bits: Query<(Entity, &mut Bit, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut b, mut t) in &mut bits {
        b.life -= dt;
        if b.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        b.velocity.y -= 18.0 * dt;
        let v = b.velocity;
        t.translation += v * dt;
        t.scale = Vec3::splat((b.life / 0.6).clamp(0.3, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clouds_come_in_clumps() {
        let cover = 0.35;
        let cells: Vec<bool> = (0..64)
            .flat_map(|z| (0..64).map(move |x| cloudy(x, z, cover)))
            .collect();
        let n = cells.iter().filter(|c| **c).count();
        assert!(n > 64 * 64 / 10 && n < 64 * 64 * 6 / 10, "{n} cloudy cells");
    }

    #[test]
    fn footsteps_sound_like_the_ground() {
        let reg = ourcraft_common::block::BlockRegistry::with_builtin_blocks();
        assert_eq!(step_sound(&reg, blocks::GRASS), "step_grass");
        assert_eq!(step_sound(&reg, blocks::STONE), "step_stone");
        assert_eq!(step_sound(&reg, blocks::PLANKS), "step_wood");
        assert_eq!(step_sound(&reg, blocks::SAND), "step_sand");
        assert_eq!(step_sound(&reg, blocks::SNOW), "step_snow");
    }
}

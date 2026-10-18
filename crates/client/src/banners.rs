//! Banner patterns: a picture drawn over a banner's cloth (both sides of
//! a standing one), made from its colour and the pattern layers the
//! server tells us about.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::colours::COLOURS;
use ourcraft_common::decoration::pattern_covers;
use ourcraft_common::protocol::ServerMessage;
use ourcraft_common::shapes::{Dir4, VariantKind};

use crate::net::{FromServer, NetReceiveSet};
use crate::world::{Blocks, ClientWorld};

pub struct BannersPlugin;

impl Plugin for BannersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Banners>()
            .add_systems(PreUpdate, receive_patterns.after(NetReceiveSet))
            .add_systems(Update, show_patterns)
            .add_systems(
                PreUpdate,
                forget_patterns
                    .run_if(
                        on_message::<crate::net::SessionEnded>
                            .or_eager(on_message::<crate::net::DimensionChanged>),
                    )
                    .after(crate::net::NetReceiveSet),
            );
    }
}

/// Every patterned banner we know of, and what's showing on it.
#[derive(Resource, Default)]
struct Banners {
    layers: HashMap<IVec3, Vec<(u8, u8)>>,
    /// The cloth pictures shown: (entities, layers, block) at each spot.
    shown: HashMap<IVec3, (Vec<Entity>, Vec<(u8, u8)>, u16)>,
}

fn receive_patterns(mut messages: MessageReader<FromServer>, mut banners: ResMut<Banners>) {
    for FromServer(msg) in messages.read() {
        if let ServerMessage::BannerPattern { pos, layers } = msg {
            let pos = IVec3::from_array(*pos);
            if layers.is_empty() {
                banners.layers.remove(&pos);
            } else {
                banners.layers.insert(pos, layers.clone());
            }
        }
    }
}

fn forget_patterns(mut commands: Commands, mut banners: ResMut<Banners>) {
    for (_, (es, _, _)) in banners.shown.drain() {
        for e in es {
            commands.entity(e).despawn();
        }
    }
    banners.layers.clear();
}

fn rgb(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

/// The cloth: its own colour, then each layer painted over it.
pub fn cloth_picture(base: u8, layers: &[(u8, u8)]) -> Vec<u8> {
    let mut px = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let mut c = rgb(COLOURS[base as usize % 16].2);
            for (pattern, colour) in layers {
                if pattern_covers(*pattern, x, y) {
                    c = rgb(COLOURS[*colour as usize % 16].2);
                }
            }
            px.extend_from_slice(&c);
        }
    }
    px
}

/// The banner's own colour, from its family name (`core:red_banner`).
fn base_colour(family: &str) -> Option<u8> {
    let key = family.strip_prefix("core:")?.strip_suffix("_banner")?;
    COLOURS.iter().position(|c| c.0 == key).map(|i| i as u8)
}

#[allow(clippy::too_many_arguments)]
fn show_patterns(
    mut commands: Commands,
    mut banners: ResMut<Banners>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut quad: Local<Option<Handle<Mesh>>>,
) {
    let banners = &mut *banners;
    let gone: Vec<IVec3> = banners
        .shown
        .iter()
        .filter(|(pos, (_, layers, block))| {
            world.block(pos.x, pos.y, pos.z).0 != *block || banners.layers.get(pos) != Some(layers)
        })
        .map(|(pos, _)| *pos)
        .collect();
    for pos in gone {
        if let Some((es, _, _)) = banners.shown.remove(&pos) {
            for e in es {
                commands.entity(e).despawn();
            }
        }
    }
    let quad = quad
        .get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0)))
        .clone();
    for (pos, layers) in &banners.layers {
        if banners.shown.contains_key(pos) {
            continue;
        }
        let block = world.block(pos.x, pos.y, pos.z);
        let Some(v) = blocks.0.get(block).variant.as_ref() else {
            continue;
        };
        let Some(base) = base_colour(&v.family) else {
            continue;
        };
        // The cloth's faces for a banner facing north (centre, size), and
        // whether it has a back to draw on too.
        let s = 1.0 / 16.0;
        let (centre, size, back) = match v.kind {
            VariantKind::Banner => (
                Vec3::new(0.0, 9.5 * s - 0.5, 8.0 * s - 0.5 - 0.01),
                Vec2::new(14.0 * s, 13.0 * s),
                Some(Vec3::new(0.0, 9.5 * s - 0.5, 9.0 * s - 0.5 + 0.01)),
            ),
            VariantKind::WallBanner => (
                Vec3::new(0.0, 7.0 * s - 0.5, 15.0 * s - 0.5 - 0.01),
                Vec2::new(14.0 * s, 14.0 * s),
                None,
            ),
            _ => continue,
        };
        let mut image = Image::new(
            Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            cloth_picture(base, layers),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::nearest();
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            unlit: true,
            ..default()
        });
        let turns = match v.facing {
            Dir4::North => 0.0,
            Dir4::East => 1.0,
            Dir4::South => 2.0,
            Dir4::West => 3.0,
        };
        let spin = Quat::from_rotation_y(-turns * std::f32::consts::FRAC_PI_2);
        let middle = pos.as_vec3() + Vec3::splat(0.5);
        let mut es = Vec::new();
        // The quad faces +z: turned round to face north for the front,
        // left as it is for the back.
        let faces = [(centre, std::f32::consts::PI)]
            .into_iter()
            .chain(back.map(|b| (b, 0.0)));
        for (at, turn) in faces {
            es.push(
                commands
                    .spawn((
                        Mesh3d(quad.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_translation(middle + spin * at)
                            .with_rotation(spin * Quat::from_rotation_y(turn))
                            .with_scale(size.extend(1.0)),
                    ))
                    .id(),
            );
        }
        banners.shown.insert(*pos, (es, layers.clone(), block.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_paint_the_cloth() {
        let plain = cloth_picture(0, &[]);
        let striped = cloth_picture(0, &[(0, 14)]);
        assert_ne!(plain, striped);
        // The stripe goes across the middle, not the top.
        let at =
            |px: &[u8], x: usize, y: usize| px[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].to_vec();
        assert_eq!(at(&striped, 3, 0), at(&plain, 3, 0));
        assert_ne!(at(&striped, 3, 7), at(&plain, 3, 7));
        assert_eq!(base_colour("core:light_gray_banner"), Some(1));
    }
}

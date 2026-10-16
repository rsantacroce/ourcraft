//! Item frames (milestone 21): the server says what's in each frame, and
//! we show the item's picture on the frame's front.

use std::collections::HashMap;

use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::item::ItemId;
use ourcraft_common::protocol::ServerMessage;
use ourcraft_common::shapes::Dir4;

use crate::inventory_ui::ItemIcons;
use crate::net::{FromServer, NetReceiveSet};
use crate::world::{Blocks, ClientWorld};

pub struct FramesPlugin;

impl Plugin for FramesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Frames>()
            .add_systems(PreUpdate, hear_frames.after(NetReceiveSet))
            .add_systems(Update, show_frames)
            .add_systems(
                PreUpdate,
                forget_frames.run_if(on_message::<crate::net::SessionEnded>),
            );
    }
}

/// What's in every frame we know of, and the picture showing it: (the
/// entity, the item, the frame block it was made for).
#[derive(Resource, Default)]
struct Frames {
    items: HashMap<IVec3, ItemId>,
    shown: HashMap<IVec3, (Entity, ItemId, BlockId)>,
}

fn hear_frames(mut messages: MessageReader<FromServer>, mut frames: ResMut<Frames>) {
    for FromServer(msg) in messages.read() {
        if let ServerMessage::FrameItem { pos, item } = msg {
            let pos = IVec3::from_array(*pos);
            if *item == 0 {
                frames.items.remove(&pos);
            } else {
                frames.items.insert(pos, ItemId(*item));
            }
        }
    }
}

fn forget_frames(mut commands: Commands, mut frames: ResMut<Frames>) {
    for (_, (e, _, _)) in frames.shown.drain() {
        commands.entity(e).despawn();
    }
    frames.items.clear();
}

#[allow(clippy::too_many_arguments)]
fn show_frames(
    mut commands: Commands,
    mut frames: ResMut<Frames>,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    icons: Option<Res<ItemIcons>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    mut made: Local<HashMap<ItemId, Handle<StandardMaterial>>>,
) {
    let Some(icons) = icons else { return };
    let frames = &mut *frames;
    // Take down pictures whose frame changed, went away, or got a new item.
    let gone: Vec<IVec3> = frames
        .shown
        .iter()
        .filter(|(pos, (_, item, block))| {
            world.0.block(pos.x, pos.y, pos.z) != *block || frames.items.get(pos) != Some(item)
        })
        .map(|(pos, _)| *pos)
        .collect();
    for pos in gone {
        if let Some((e, _, _)) = frames.shown.remove(&pos) {
            commands.entity(e).despawn();
        }
    }
    let quad = quad
        .get_or_insert_with(|| meshes.add(Rectangle::new(0.6, 0.6)))
        .clone();
    for (pos, item) in &frames.items {
        if frames.shown.contains_key(pos) {
            continue;
        }
        let block = world.0.block(pos.x, pos.y, pos.z);
        let Some(v) = blocks.0.get(block).variant.as_ref() else {
            continue;
        };
        if v.family != "core:item_frame" {
            continue;
        }
        let Some(icon) = icons.0.get(item) else {
            continue;
        };
        let material = made
            .entry(*item)
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color_texture: Some(icon.clone()),
                    alpha_mode: AlphaMode::Mask(0.5),
                    unlit: true,
                    ..default()
                })
            })
            .clone();
        // The frame's front, for one facing north; then turned its way.
        let turns = match v.facing {
            Dir4::North => 0.0,
            Dir4::East => 1.0,
            Dir4::South => 2.0,
            Dir4::West => 3.0,
        };
        let spin = Quat::from_rotation_y(-turns * std::f32::consts::FRAC_PI_2);
        let front = Vec3::new(0.0, 0.0, 15.0 / 16.0 - 0.5 - 0.02);
        let e = commands
            .spawn((
                Mesh3d(quad.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(pos.as_vec3() + Vec3::splat(0.5) + spin * front)
                    .with_rotation(spin * Quat::from_rotation_y(std::f32::consts::PI)),
            ))
            .id();
        frames.shown.insert(*pos, (e, *item, block));
    }
}

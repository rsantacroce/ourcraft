//! The (for now tiny) world: one chunk at the origin.

use bevy::prelude::*;
use ourcraft_common::block::{BlockId, BlockRegistry};
use ourcraft_common::chunk::{Chunk, ChunkPos};
use ourcraft_common::demo::demo_chunk;

use crate::meshing::mesh_chunk;
use crate::render::{BlockTextureLayers, VoxelMaterialHandle, build_mesh};

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Blocks(BlockRegistry::with_builtin_blocks()))
            // Runs after the render plugin's startup system has built the
            // texture array, because meshing needs to know texture layers.
            .add_systems(
                Startup,
                spawn_demo_chunk.after(crate::render::setup_block_textures),
            );
    }
}

/// The block registry, shared by meshing and (later) gameplay.
#[derive(Resource)]
pub struct Blocks(pub BlockRegistry);

/// A chunk in the world. Its entity's `Transform` places it.
#[derive(Component)]
pub struct ChunkData {
    pub pos: ChunkPos,
    pub chunk: Chunk,
}

fn spawn_demo_chunk(
    mut commands: Commands,
    blocks: Res<Blocks>,
    layers: Res<BlockTextureLayers>,
    material: Res<VoxelMaterialHandle>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let pos = ChunkPos::new(0, 0, 0);
    let chunk = demo_chunk();

    // Blocks outside this single chunk count as air, so its edges get faces.
    let data = mesh_chunk(
        |x, y, z| chunk.get_checked(x, y, z).unwrap_or(BlockId::AIR),
        &blocks.0,
        &layers.0,
    );
    info!(
        "Demo chunk: {} solid blocks, {} faces",
        chunk.solid_count(),
        data.face_count()
    );

    let [x, y, z] = pos.min_block();
    commands.spawn((
        Name::new("Chunk (0, 0, 0)"),
        Mesh3d(meshes.add(build_mesh(data))),
        MeshMaterial3d(material.0.clone()),
        Transform::from_xyz(x as f32, y as f32, z as f32),
        ChunkData { pos, chunk },
    ));
}

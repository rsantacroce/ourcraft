//! The client's copy of the world and the machinery that streams chunks in
//! and out around the player.
//!
//! Every frame:
//! 1. **Request** columns near the player that we don't have yet. For now
//!    they're generated locally on background threads; in M6 they arrive
//!    from the server instead.
//! 2. **Receive** finished columns and mark their chunks (and neighbours,
//!    whose border faces may change) as needing a new mesh.
//! 3. **Unload** columns that are now too far away.
//! 4. **Queue meshing** for dirty chunks, nearest first, on background
//!    threads. A chunk is only meshed once all its horizontal neighbours are
//!    loaded, so border faces come out right the first time.
//! 5. **Apply** finished meshes: create or update the chunk's entity.
//!
//! Bevy culls chunk entities outside the camera's view (frustum culling)
//! automatically, using the bounding box it computes for each mesh.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use ourcraft_common::block::{BlockId, BlockRegistry};
use ourcraft_common::chunk::{Chunk, ChunkPos};
use ourcraft_common::world::{ColumnPos, VoxelWorld, WORLD_HEIGHT_CHUNKS};
use ourcraft_common::worldgen::WorldGenerator;

use crate::meshing::{ChunkMeshData, ChunkMeshes, PaddedChunk, mesh_chunk};
use crate::render::{BlockTextureLayers, VoxelMaterial, VoxelMaterialHandle, build_mesh};
use crate::settings::GraphicsSettings;

/// Seed for the world until saves (M5) let you pick one.
pub const DEFAULT_SEED: u64 = 20_261_015;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Blocks(Arc::new(BlockRegistry::with_builtin_blocks())))
            .insert_resource(TerrainGenerator(Arc::new(WorldGenerator::new(
                DEFAULT_SEED,
            ))))
            .init_resource::<ClientWorld>()
            .init_resource::<ChunkStreaming>()
            .add_systems(
                Update,
                (
                    apply_block_edits,
                    request_columns,
                    receive_columns,
                    unload_far_columns,
                    queue_meshing,
                    apply_meshes,
                )
                    .chain()
                    .in_set(WorldSystems),
            );
    }
}

/// System set for world streaming, so other systems can order around it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldSystems;

/// The block registry, shared by meshing and gameplay.
#[derive(Resource, Clone)]
pub struct Blocks(pub Arc<BlockRegistry>);

/// Generates terrain locally.
#[derive(Resource, Clone)]
pub struct TerrainGenerator(pub Arc<WorldGenerator>);

/// The client's copy of all loaded chunks.
#[derive(Resource, Default, Deref, DerefMut)]
pub struct ClientWorld(pub VoxelWorld);

/// Marks the entity the world loads around (the camera, later the player).
#[derive(Component)]
pub struct ChunkViewer;

/// Marks a chunk's mesh entity.
#[derive(Component)]
pub struct ChunkMesh(pub ChunkPos);

/// The (up to) two entities drawing a chunk: solid and translucent.
#[derive(Default, Clone, Copy)]
struct ChunkEntities {
    solid: Option<Entity>,
    translucent: Option<Entity>,
}

impl ChunkEntities {
    fn despawn(self, commands: &mut Commands) {
        for e in [self.solid, self.translucent].into_iter().flatten() {
            commands.entity(e).despawn();
        }
    }
}

/// Bookkeeping for streaming and meshing.
#[derive(Resource, Default)]
pub struct ChunkStreaming {
    generating: HashMap<ColumnPos, Task<Vec<Chunk>>>,
    meshing: HashMap<ChunkPos, Task<ChunkMeshes>>,
    entities: HashMap<ChunkPos, ChunkEntities>,
    dirty: HashSet<ChunkPos>,
    /// Column the viewer was in last frame.
    pub center: ColumnPos,
}

impl ChunkStreaming {
    /// Mark a chunk as needing a new mesh.
    pub fn mark_dirty(&mut self, pos: ChunkPos) {
        if (0..WORLD_HEIGHT_CHUNKS).contains(&pos.y) {
            self.dirty.insert(pos);
        }
    }

    /// Mark the chunk containing a block dirty, plus any neighbouring chunk
    /// whose border faces or shading that block affects.
    pub fn mark_block_dirty(&mut self, x: i32, y: i32, z: i32) {
        // A block also changes the AO of blocks diagonally next to it, so
        // check all 27 neighbours' chunks (it's usually just one chunk).
        for dy in -1..=1 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    self.mark_dirty(ChunkPos::from_block(x + dx, y + dy, z + dz));
                }
            }
        }
    }

    pub fn pending_generation(&self) -> usize {
        self.generating.len()
    }

    pub fn pending_meshes(&self) -> usize {
        self.meshing.len()
    }

    pub fn mesh_count(&self) -> usize {
        self.entities.len()
    }
}

/// Change a block in the client world and schedule the remeshing it needs.
/// Returns the previous block, or `None` if the chunk isn't loaded.
pub fn set_block(
    world: &mut ClientWorld,
    streaming: &mut ChunkStreaming,
    pos: IVec3,
    block: BlockId,
) -> Option<BlockId> {
    let old = world.set_block(pos.x, pos.y, pos.z, block)?;
    if old != block {
        streaming.mark_block_dirty(pos.x, pos.y, pos.z);
    }
    Some(old)
}

/// Single player: apply block edits straight to the local world. (In
/// multiplayer the server does this and tells everyone.)
fn apply_block_edits(
    mut edits: MessageReader<crate::interaction::BlockEditRequest>,
    mut world: ResMut<ClientWorld>,
    mut streaming: ResMut<ChunkStreaming>,
) {
    for edit in edits.read() {
        set_block(&mut world, &mut streaming, edit.pos, edit.block);
    }
}

/// Max columns generating at once. Enough to keep all cores busy without
/// queueing work for columns we might walk away from.
fn max_generation_tasks() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get()) * 2
}

fn viewer_column(viewer: &Transform) -> ColumnPos {
    ColumnPos::from_world(viewer.translation.x, viewer.translation.z)
}

fn request_columns(
    viewer: Single<&Transform, With<ChunkViewer>>,
    settings: Res<GraphicsSettings>,
    world: Res<ClientWorld>,
    generator: Res<TerrainGenerator>,
    mut streaming: ResMut<ChunkStreaming>,
) {
    let center = viewer_column(&viewer);
    streaming.center = center;
    let budget = max_generation_tasks().saturating_sub(streaming.generating.len());
    if budget == 0 {
        return;
    }

    // All missing columns within range, nearest first. One extra ring is
    // loaded (but not drawn), because meshing a column needs its neighbours.
    let r = settings.render_distance + 1;
    let mut wanted: Vec<ColumnPos> = (-r..=r)
        .flat_map(|dz| (-r..=r).map(move |dx| ColumnPos::new(center.x + dx, center.z + dz)))
        .filter(|p| !world.has_column(*p) && !streaming.generating.contains_key(p))
        .collect();
    wanted.sort_by_key(|p| (p.x - center.x).pow(2) + (p.z - center.z).pow(2));

    let pool = AsyncComputeTaskPool::get();
    for pos in wanted.into_iter().take(budget) {
        let generator = generator.0.clone();
        let task = pool.spawn(async move { generator.generate_column(pos) });
        streaming.generating.insert(pos, task);
    }
}

fn receive_columns(mut world: ResMut<ClientWorld>, mut streaming: ResMut<ChunkStreaming>) {
    let mut done = Vec::new();
    streaming
        .generating
        .retain(|pos, task| match check_ready(task) {
            Some(chunks) => {
                done.push((*pos, chunks));
                false
            }
            None => true,
        });
    for (pos, chunks) in done {
        world.insert_column(pos, chunks);
        // This column and its neighbours may now be ready to mesh.
        for dz in -1..=1 {
            for dx in -1..=1 {
                let col = ColumnPos::new(pos.x + dx, pos.z + dz);
                if world.has_column(col) {
                    for cy in 0..WORLD_HEIGHT_CHUNKS {
                        streaming.mark_dirty(col.chunk(cy));
                    }
                }
            }
        }
    }
}

fn unload_far_columns(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    mut world: ResMut<ClientWorld>,
    mut streaming: ResMut<ChunkStreaming>,
) {
    let center = streaming.center;
    // Keep one more ring than we load so walking back and forth over a
    // border doesn't reload the same columns over and over.
    let keep = settings.render_distance + 2;
    let far: Vec<ColumnPos> = world
        .columns()
        .filter(|p| p.chebyshev_distance(center) > keep)
        .collect();
    for pos in far {
        world.remove_column(pos);
        for cy in 0..WORLD_HEIGHT_CHUNKS {
            let cpos = pos.chunk(cy);
            streaming.dirty.remove(&cpos);
            streaming.meshing.remove(&cpos); // dropping a task cancels it
            if let Some(e) = streaming.entities.remove(&cpos) {
                e.despawn(&mut commands);
            }
        }
    }
    // Also cancel generation that's no longer wanted.
    streaming
        .generating
        .retain(|p, _| p.chebyshev_distance(center) <= keep);
}

/// True if all 8 columns around `pos` are loaded.
fn neighbours_loaded(world: &VoxelWorld, pos: ColumnPos) -> bool {
    (-1..=1).all(|dz| (-1..=1).all(|dx| world.has_column(ColumnPos::new(pos.x + dx, pos.z + dz))))
}

/// A chunk that is solid stone (say) surrounded by more solid stone has no
/// visible faces; skip meshing it entirely.
fn fully_hidden(world: &VoxelWorld, registry: &BlockRegistry, pos: ChunkPos) -> bool {
    let opaque_uniform = |p: ChunkPos| {
        world
            .chunk(p)
            .and_then(|c| c.uniform_block())
            .is_some_and(|b| registry.is_opaque(b))
    };
    opaque_uniform(pos)
        && [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ]
        .iter()
        .all(|(dx, dy, dz)| {
            let n = ChunkPos::new(pos.x + dx, pos.y + dy, pos.z + dz);
            // Below the world counts as hidden.
            n.y < 0 || opaque_uniform(n)
        })
}

/// Chunks meshed per frame at most. Copying neighbour data happens on the
/// main thread, so this bounds the per-frame cost.
const MESHES_PER_FRAME: usize = 24;

fn queue_meshing(
    mut commands: Commands,
    world: Res<ClientWorld>,
    blocks: Res<Blocks>,
    layers: Option<Res<BlockTextureLayers>>,
    mut streaming: ResMut<ChunkStreaming>,
) {
    let Some(layers) = layers else { return };
    let center = streaming.center;
    let mut ready: Vec<ChunkPos> = streaming
        .dirty
        .iter()
        .copied()
        .filter(|p| neighbours_loaded(&world, ColumnPos::from(*p)))
        .collect();
    let dist = |p: &ChunkPos| (p.x - center.x).pow(2) + (p.z - center.z).pow(2);
    ready.sort_by_key(dist);

    let pool = AsyncComputeTaskPool::get();
    for pos in ready.into_iter().take(MESHES_PER_FRAME) {
        streaming.dirty.remove(&pos);
        let Some(chunk) = world.chunk(pos) else {
            continue;
        };
        if chunk.is_empty() || fully_hidden(&world, &blocks.0, pos) {
            streaming.meshing.remove(&pos);
            if let Some(e) = streaming.entities.remove(&pos) {
                e.despawn(&mut commands);
            }
            continue;
        }
        let padded = PaddedChunk::from_world(&world, pos);
        let registry = blocks.0.clone();
        let layers = layers.0.clone();
        let task = pool.spawn(async move { mesh_chunk(&padded, &registry, &layers) });
        // Replacing an in-flight task for the same chunk cancels the old one.
        streaming.meshing.insert(pos, task);
    }
}

fn apply_meshes(
    mut commands: Commands,
    material: Option<Res<VoxelMaterialHandle>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut streaming: ResMut<ChunkStreaming>,
) {
    let Some(material) = material else { return };
    let mut done = Vec::new();
    streaming
        .meshing
        .retain(|pos, task| match check_ready(task) {
            Some(data) => {
                done.push((*pos, data));
                false
            }
            None => true,
        });
    for (pos, data) in done {
        let mut ents = streaming.entities.remove(&pos).unwrap_or_default();
        let [x, y, z] = pos.min_block();
        let transform = Transform::from_xyz(x as f32, y as f32, z as f32);
        let mut update = |slot: &mut Option<Entity>,
                          data: ChunkMeshData,
                          mat: &Handle<VoxelMaterial>| {
            if data.is_empty() {
                if let Some(e) = slot.take() {
                    commands.entity(e).despawn();
                }
                return;
            }
            let mesh = Mesh3d(meshes.add(build_mesh(data)));
            match slot {
                Some(e) => {
                    commands.entity(*e).insert(mesh);
                }
                None => {
                    *slot = Some(
                        commands
                            .spawn((ChunkMesh(pos), mesh, MeshMaterial3d(mat.clone()), transform))
                            .id(),
                    );
                }
            }
        };
        update(&mut ents.solid, data.solid, &material.solid);
        update(
            &mut ents.translucent,
            data.translucent,
            &material.translucent,
        );
        if ents.solid.is_some() || ents.translucent.is_some() {
            streaming.entities.insert(pos, ents);
        }
    }
}

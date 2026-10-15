//! The client's copy of the world and the machinery that streams chunks in
//! and out around the player.
//!
//! The server decides which columns we get (based on where we are and our
//! view distance) and sends them as compressed bytes. Every frame:
//! 1. **Decode** newly arrived columns on background threads.
//! 2. **Receive** decoded columns and mark their chunks (and neighbours,
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
use ourcraft_common::protocol::{Channel, ClientMessage, ServerMessage};
use ourcraft_common::save::decode_column;
use ourcraft_common::world::{ColumnPos, VoxelWorld, WORLD_HEIGHT_CHUNKS};

use crate::light::{Heights, Neighbourhood, column_heights, compute_light, height_after_edit};
use crate::meshing::{ChunkMeshData, ChunkMeshes, PaddedChunk, mesh_chunk};
use crate::net::{Connection, FromServer, NetReceiveSet};
use crate::render::{BlockTextureLayers, VoxelMaterial, VoxelMaterialHandle, build_mesh};
use crate::settings::GraphicsSettings;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Blocks(Arc::new(BlockRegistry::with_builtin_blocks())))
            .init_resource::<ClientWorld>()
            .init_resource::<ChunkStreaming>()
            .add_systems(PreUpdate, handle_world_messages.after(NetReceiveSet))
            .add_systems(
                PreUpdate,
                reset_world.run_if(on_message::<crate::net::SessionEnded>),
            )
            .add_systems(
                Update,
                (
                    apply_block_edits,
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
    /// Columns being decoded on background threads (with their heights).
    decoding: HashMap<ColumnPos, Task<Option<(Vec<Chunk>, Box<Heights>)>>>,
    /// The highest block of every x, z, per loaded column (for sky light).
    heights: HashMap<ColumnPos, Arc<Heights>>,
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

    pub fn pending_decodes(&self) -> usize {
        self.decoding.len()
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
        // Light reaches 15 blocks, so chunks that far away may look
        // different now. If the top of the column moved, sunlight changes
        // all the way between the old and new tops.
        let col = ColumnPos::from_world(pos.x as f32, pos.z as f32);
        let (mut low, mut high) = (pos.y, pos.y);
        if let Some(h) = streaming.heights.get_mut(&col) {
            let i = (pos.x.rem_euclid(16) + pos.z.rem_euclid(16) * 16) as usize;
            let before = h[i];
            let after = height_after_edit(world, pos.x, pos.z, pos.y, before, block);
            if after != before {
                Arc::make_mut(h)[i] = after;
                low = low.min(after as i32).min(before as i32);
                high = high.max(after as i32).max(before as i32);
            }
        }
        for cy in (low - 15).div_euclid(16)..=(high + 15).div_euclid(16) {
            for dz in [-15, 0, 15] {
                for dx in [-15, 0, 15] {
                    streaming.mark_dirty(ChunkPos::from_block(pos.x + dx, cy * 16, pos.z + dz));
                }
            }
        }
    }
    Some(old)
}

/// Apply our own edits straight away (so they feel instant) and ask the
/// server to make them real. If the server says no, it sends back the
/// correct block, which overwrites our guess.
fn apply_block_edits(
    mut edits: MessageReader<crate::interaction::BlockEditRequest>,
    mut world: ResMut<ClientWorld>,
    mut streaming: ResMut<ChunkStreaming>,
    blocks: Res<Blocks>,
    mut broken: MessageWriter<crate::atmosphere::BlockBroken>,
    conn: Option<ResMut<Connection>>,
) {
    let Some(mut conn) = conn else { return };
    for edit in edits.read() {
        let old = set_block(&mut world, &mut streaming, edit.pos, edit.block);
        report_break(&blocks, &mut broken, edit.pos, old, edit.block);
        conn.send(
            Channel::Reliable,
            &ClientMessage::SetBlock {
                pos: edit.pos.to_array(),
                block: edit.block.0,
                held: edit.held.0,
            },
        );
    }
}

/// World data from the server: new columns and changed blocks.
/// Tell the atmosphere a block broke (for the flying bits), unless it was
/// just water or lava drying up.
fn report_break(
    blocks: &Blocks,
    broken: &mut MessageWriter<crate::atmosphere::BlockBroken>,
    pos: IVec3,
    old: Option<BlockId>,
    new: BlockId,
) {
    if let Some(old) = old
        && new.is_air()
        && !old.is_air()
        && !blocks.0.is_liquid(old)
    {
        broken.write(crate::atmosphere::BlockBroken { pos, block: old });
    }
}

fn handle_world_messages(
    mut messages: MessageReader<FromServer>,
    mut world: ResMut<ClientWorld>,
    mut streaming: ResMut<ChunkStreaming>,
    blocks: Res<Blocks>,
    mut broken: MessageWriter<crate::atmosphere::BlockBroken>,
) {
    let pool = AsyncComputeTaskPool::get();
    for FromServer(msg) in messages.read() {
        match msg {
            ServerMessage::Column { x, z, data } => {
                let pos = ColumnPos::new(*x, *z);
                let data = data.clone();
                // Decompressing takes a moment; keep it off the main thread.
                let task = pool.spawn(async move {
                    let chunks = decode_column(&data, None).ok()?;
                    let heights = column_heights(&chunks);
                    Some((chunks, heights))
                });
                streaming.decoding.insert(pos, task);
            }
            ServerMessage::BlockChanged { pos, block } => {
                let pos = IVec3::from_array(*pos);
                let old = set_block(&mut world, &mut streaming, pos, BlockId(*block));
                report_break(&blocks, &mut broken, pos, old, BlockId(*block));
            }
            _ => {}
        }
    }
}

/// Leaving a game: forget its world.
fn reset_world(
    mut commands: Commands,
    mut world: ResMut<ClientWorld>,
    mut streaming: ResMut<ChunkStreaming>,
) {
    for (_, e) in streaming.entities.drain() {
        e.despawn(&mut commands);
    }
    *streaming = ChunkStreaming::default();
    world.0 = VoxelWorld::new();
}

fn viewer_column(viewer: &Transform) -> ColumnPos {
    ColumnPos::from_world(viewer.translation.x, viewer.translation.z)
}

fn receive_columns(mut world: ResMut<ClientWorld>, mut streaming: ResMut<ChunkStreaming>) {
    let mut done = Vec::new();
    streaming
        .decoding
        .retain(|pos, task| match check_ready(task) {
            Some(Some((chunks, heights))) => {
                done.push((*pos, chunks, heights));
                false
            }
            Some(None) => {
                warn!("Received a corrupt column {pos:?}");
                false
            }
            None => true,
        });
    for (pos, chunks, heights) in done {
        world.insert_column(pos, chunks);
        streaming.heights.insert(pos, Arc::from(heights));
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
    viewer: Option<Single<&Transform, With<ChunkViewer>>>,
    mut world: ResMut<ClientWorld>,
    mut streaming: ResMut<ChunkStreaming>,
) {
    if let Some(v) = viewer {
        streaming.center = viewer_column(&v);
    }
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
        streaming.heights.remove(&pos);
        for cy in 0..WORLD_HEIGHT_CHUNKS {
            let cpos = pos.chunk(cy);
            streaming.dirty.remove(&cpos);
            streaming.meshing.remove(&cpos); // dropping a task cancels it
            if let Some(e) = streaming.entities.remove(&cpos) {
                e.despawn(&mut commands);
            }
        }
    }
    // Also drop decodes for columns that are now out of range.
    streaming
        .decoding
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
        let heights = &streaming.heights;
        let near = Neighbourhood::from_world(&world, &|c| heights.get(&c).cloned(), pos);
        let registry = blocks.0.clone();
        let layers = layers.0.clone();
        let task = pool.spawn(async move {
            let light = compute_light(&near, &registry);
            let padded = PaddedChunk::from_neighbourhood(&near, &light);
            mesh_chunk(&padded, &registry, &layers)
        });
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

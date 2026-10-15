//! Turning blocks into triangles.
//!
//! A GPU can't draw "a chunk of blocks"; it draws triangles. Meshing builds
//! the triangle list for a chunk. Three tricks keep that list short and good
//! looking:
//!
//! 1. **Face culling.** A face between two opaque blocks can never be seen,
//!    so we skip it.
//! 2. **Greedy meshing.** Neighbouring visible faces that look identical are
//!    merged into one big rectangle. We sweep the chunk one 16×16 slice at a
//!    time, per face direction; in each slice we take the first unmerged face,
//!    grow it as far right as possible, then as far down as possible, emit one
//!    quad, and repeat. A flat field of grass becomes a single quad.
//! 3. **Ambient occlusion (AO).** Corners tucked against other blocks get
//!    darker, which makes the shapes much easier to read. Each vertex looks at
//!    the three blocks touching it in front of the face. Faces only merge if
//!    their corner darkness matches, so AO and greedy meshing work together.
//!
//! This module is plain Rust with no Bevy types, so it runs on background
//! threads and is easy to test.

use ourcraft_common::block::{BlockId, BlockRegistry, Face, RenderKind};
use ourcraft_common::chunk::{CHUNK_SIZE, CHUNK_SIZE_I32, Chunk, ChunkPos};
use ourcraft_common::world::{VoxelWorld, WORLD_HEIGHT_CHUNKS};

/// Texture-array layer for each face of each block type, indexed as
/// `layers[block_id][face as usize]`.
pub type FaceLayers = [[u32; 6]];

/// Shade value that marks a vertex as glowing (ignores daylight and AO).
pub const EMISSIVE_SHADE: f32 = 2.0;

/// Vertex data for one chunk, ready to upload.
#[derive(Default, Debug, Clone)]
pub struct ChunkMeshData {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Texture-array layer per vertex (the same for all 4 corners of a face).
    pub layers: Vec<u32>,
    /// Brightness per vertex, `0..=1`, or [`EMISSIVE_SHADE`].
    pub shades: Vec<f32>,
    pub indices: Vec<u32>,
}

/// A chunk's two meshes: everything solid or cut-out, and the translucent
/// faces (water), which must be drawn in a separate blended pass.
#[derive(Default, Debug, Clone)]
pub struct ChunkMeshes {
    pub solid: ChunkMeshData,
    pub translucent: ChunkMeshData,
}

impl ChunkMeshes {
    pub fn is_empty(&self) -> bool {
        self.solid.is_empty() && self.translucent.is_empty()
    }

    pub fn face_count(&self) -> usize {
        self.solid.face_count() + self.translucent.face_count()
    }
}

impl ChunkMeshData {
    pub fn face_count(&self) -> usize {
        self.positions.len() / 4
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// Edge length of [`PaddedChunk`]: the chunk plus one block on each side.
const P: usize = CHUNK_SIZE + 2;

/// A copy of one chunk plus a one-block border taken from its neighbours.
///
/// Meshing needs to see across the chunk edge (is the face on the border
/// hidden by the next chunk?), and AO needs the blocks diagonally next to it.
/// Copying those into one array up front means the background thread doesn't
/// need access to the whole world.
pub struct PaddedChunk {
    blocks: Box<[BlockId; P * P * P]>,
}

impl PaddedChunk {
    #[inline]
    fn idx(x: i32, y: i32, z: i32) -> usize {
        (x + 1) as usize + (z + 1) as usize * P + (y + 1) as usize * P * P
    }

    /// Block at chunk-local coordinates, valid for `-1..=16` on each axis.
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        self.blocks[Self::idx(x, y, z)]
    }

    /// Build from a function of chunk-local coordinates (`-1..=16`).
    pub fn from_fn(f: impl Fn(i32, i32, i32) -> BlockId) -> Self {
        let mut blocks = Box::new([BlockId::AIR; P * P * P]);
        for y in -1..=CHUNK_SIZE_I32 {
            for z in -1..=CHUNK_SIZE_I32 {
                for x in -1..=CHUNK_SIZE_I32 {
                    blocks[Self::idx(x, y, z)] = f(x, y, z);
                }
            }
        }
        Self { blocks }
    }

    /// A chunk surrounded by air.
    pub fn isolated(chunk: &Chunk) -> Self {
        Self::from_fn(|x, y, z| chunk.get_checked(x, y, z).unwrap_or(BlockId::AIR))
    }

    /// Copy a chunk and its border from the world. Missing neighbours above
    /// the world count as air; below the world as bedrock (so the underside
    /// of the world isn't drawn); missing horizontal neighbours as air.
    pub fn from_world(world: &VoxelWorld, pos: ChunkPos) -> Self {
        // Look up the 27 chunks around `pos` once instead of per block.
        let mut near: [Option<&Chunk>; 27] = [None; 27];
        for dy in -1..=1 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let i = (dx + 1 + (dz + 1) * 3 + (dy + 1) * 9) as usize;
                    near[i] = world.chunk(ChunkPos::new(pos.x + dx, pos.y + dy, pos.z + dz));
                }
            }
        }
        let n = CHUNK_SIZE_I32;
        let side = |v: i32| {
            if v < 0 {
                -1
            } else if v >= n {
                1
            } else {
                0
            }
        };
        Self::from_fn(|x, y, z| {
            let (ox, oy, oz) = (side(x), side(y), side(z));
            match near[(ox + 1 + (oz + 1) * 3 + (oy + 1) * 9) as usize] {
                Some(c) => c.get(
                    (x - ox * n) as usize,
                    (y - oy * n) as usize,
                    (z - oz * n) as usize,
                ),
                None if pos.y + oy < 0 => ourcraft_common::block::blocks::BEDROCK,
                None if pos.y + oy >= WORLD_HEIGHT_CHUNKS => BlockId::AIR,
                None => BlockId::AIR,
            }
        })
    }
}

/// Simple fixed lighting: the sun is overhead and a bit to the side, so each
/// face direction gets its own brightness. Cheap and reads clearly.
fn face_shade(face: Face) -> f32 {
    match face {
        Face::PosY => 1.0,
        Face::NegY => 0.5,
        Face::PosX | Face::NegX => 0.8,
        Face::PosZ | Face::NegZ => 0.65,
    }
}

/// Brightness for AO levels 0 (corner fully enclosed) to 3 (open).
const AO_CURVE: [f32; 4] = [0.45, 0.65, 0.82, 1.0];

/// How a face direction maps onto the three axes: `d` is the axis the face
/// points along; `u` and `v` span the face (`v` is "up" on side faces, so
/// textures stand upright).
struct FaceAxes {
    d: usize,
    u: usize,
    v: usize,
    /// True for faces pointing towards +d.
    positive: bool,
    /// Whether corners listed in (u, v) order need reversing to wind
    /// counter-clockwise when seen from outside. Comes from u × v pointing
    /// against the face normal.
    reverse: bool,
}

fn face_axes(face: Face) -> FaceAxes {
    let (d, u, v, positive, reverse) = match face {
        Face::PosX => (0, 2, 1, true, true),
        Face::NegX => (0, 2, 1, false, false),
        Face::PosY => (1, 0, 2, true, true),
        Face::NegY => (1, 0, 2, false, false),
        Face::PosZ => (2, 0, 1, true, false),
        Face::NegZ => (2, 0, 1, false, true),
    };
    FaceAxes {
        d,
        u,
        v,
        positive,
        reverse,
    }
}

/// Should `face` of `block` be drawn, given the block in front of it?
fn face_visible(registry: &BlockRegistry, block: BlockId, neighbour: BlockId) -> bool {
    match registry.render_kind(block) {
        RenderKind::Invisible => false,
        RenderKind::Opaque => !registry.is_opaque(neighbour),
        // Glass next to glass (or water next to water) shows no inner faces.
        RenderKind::Cutout | RenderKind::Translucent => {
            !registry.is_opaque(neighbour) && neighbour != block
        }
    }
}

/// Build the mesh for one chunk.
pub fn mesh_chunk(
    chunk: &PaddedChunk,
    registry: &BlockRegistry,
    layers: &FaceLayers,
) -> ChunkMeshes {
    let mut mesh = ChunkMeshes::default();
    let n = CHUNK_SIZE;
    // One entry per block in the current slice: 0 = no face, otherwise a key
    // describing exactly how the face looks. Equal keys can merge.
    let mut mask = [0u32; CHUNK_SIZE * CHUNK_SIZE];

    for face in Face::ALL {
        let axes = face_axes(face);
        let normal = face.normal();
        for slice in 0..n {
            // 1. Fill the mask for this slice.
            for j in 0..n {
                for i in 0..n {
                    let mut p = [0i32; 3];
                    p[axes.d] = slice as i32;
                    p[axes.u] = i as i32;
                    p[axes.v] = j as i32;
                    let block = chunk.get(p[0], p[1], p[2]);
                    let front = [p[0] + normal[0], p[1] + normal[1], p[2] + normal[2]];
                    let neighbour = chunk.get(front[0], front[1], front[2]);
                    mask[i + j * n] = if face_visible(registry, block, neighbour) {
                        face_key(chunk, registry, layers, block, face, &axes, front)
                    } else {
                        0
                    };
                }
            }

            // 2. Greedily merge equal keys into rectangles.
            for j in 0..n {
                let mut i = 0;
                while i < n {
                    let key = mask[i + j * n];
                    if key == 0 {
                        i += 1;
                        continue;
                    }
                    let mut w = 1;
                    while i + w < n && mask[i + w + j * n] == key {
                        w += 1;
                    }
                    let mut h = 1;
                    'grow: while j + h < n {
                        for k in 0..w {
                            if mask[i + k + (j + h) * n] != key {
                                break 'grow;
                            }
                        }
                        h += 1;
                    }
                    for dj in 0..h {
                        for k in 0..w {
                            mask[i + k + (j + dj) * n] = 0;
                        }
                    }
                    push_quad(&mut mesh, face, &axes, slice, i, j, w, h, key);
                    i += w;
                }
            }
        }
    }
    mesh
}

// Layout of a face key, from the low bits up.
const KEY_AO_BITS: u32 = 8; // 2 bits of AO per corner
const KEY_EMISSIVE: u32 = 1 << 8;
const KEY_TRANSLUCENT: u32 = 1 << 9;
const KEY_LAYER_SHIFT: u32 = 10;

/// Pack everything that affects how a face looks into one non-zero number:
/// texture layer, glow and translucency flags and the AO level of each of
/// its 4 corners.
fn face_key(
    chunk: &PaddedChunk,
    registry: &BlockRegistry,
    layers: &FaceLayers,
    block: BlockId,
    face: Face,
    axes: &FaceAxes,
    front: [i32; 3],
) -> u32 {
    let layer = layers.get(block.0 as usize).map_or(0, |l| l[face as usize]);
    let emissive = registry.get(block).light > 0;
    let mut ao_bits = 0u32;
    if !emissive {
        let occludes = |p: [i32; 3]| registry.is_opaque(chunk.get(p[0], p[1], p[2])) as u8;
        // Corners in (u, v) order: (0,0), (1,0), (1,1), (0,1).
        for (c, (cu, cv)) in [(0, 0), (1, 0), (1, 1), (0, 1)].into_iter().enumerate() {
            let su = if cu == 0 { -1 } else { 1 };
            let sv = if cv == 0 { -1 } else { 1 };
            let mut side1 = front;
            side1[axes.u] += su;
            let mut side2 = front;
            side2[axes.v] += sv;
            let mut corner = front;
            corner[axes.u] += su;
            corner[axes.v] += sv;
            let (s1, s2, cn) = (occludes(side1), occludes(side2), occludes(corner));
            let ao = if s1 == 1 && s2 == 1 {
                0
            } else {
                3 - (s1 + s2 + cn)
            };
            ao_bits |= (ao as u32) << (c * 2);
        }
    } else {
        ao_bits = (1 << KEY_AO_BITS) - 1; // all corners fully lit
    }
    let translucent = registry.render_kind(block) == RenderKind::Translucent;
    ((layer + 1) << KEY_LAYER_SHIFT)
        | if translucent { KEY_TRANSLUCENT } else { 0 }
        | if emissive { KEY_EMISSIVE } else { 0 }
        | ao_bits
}

#[allow(clippy::too_many_arguments)]
fn push_quad(
    meshes: &mut ChunkMeshes,
    face: Face,
    axes: &FaceAxes,
    slice: usize,
    i: usize,
    j: usize,
    w: usize,
    h: usize,
    key: u32,
) {
    let layer = (key >> KEY_LAYER_SHIFT) - 1;
    let emissive = key & KEY_EMISSIVE != 0;
    let mesh = if key & KEY_TRANSLUCENT != 0 {
        &mut meshes.translucent
    } else {
        &mut meshes.solid
    };
    let ao = |c: usize| ((key >> (c * 2)) & 3) as usize;
    let base = mesh.positions.len() as u32;
    let d_coord = (slice + axes.positive as usize) as f32;

    for (c, (cu, cv)) in [(0, 0), (1, 0), (1, 1), (0, 1)].into_iter().enumerate() {
        let mut p = [0f32; 3];
        p[axes.d] = d_coord;
        p[axes.u] = (i + cu * w) as f32;
        p[axes.v] = (j + cv * h) as f32;
        mesh.positions.push(p);
        // UVs in block units; the sampler repeats the texture once per block.
        // On side faces v points down the texture, so it decreases with height.
        let uv = if axes.d == 1 {
            [p[axes.u], p[axes.v]]
        } else {
            [p[axes.u], -p[axes.v]]
        };
        mesh.uvs.push(uv);
        mesh.layers.push(layer);
        mesh.shades.push(if emissive {
            EMISSIVE_SHADE
        } else {
            face_shade(face) * AO_CURVE[ao(c)]
        });
    }

    // Two triangles. Split along whichever diagonal keeps a dark corner from
    // smearing across the whole quad (the classic AO "anisotropy" fix).
    let flip = ao(0) + ao(2) < ao(1) + ao(3);
    let tris: [u32; 6] = match (axes.reverse, flip) {
        (false, false) => [0, 1, 2, 0, 2, 3],
        (false, true) => [0, 1, 3, 1, 2, 3],
        (true, false) => [0, 2, 1, 0, 3, 2],
        (true, true) => [0, 3, 1, 1, 3, 2],
    };
    mesh.indices.extend(tris.iter().map(|t| base + t));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ourcraft_common::block::blocks;
    use ourcraft_common::chunk::{CHUNK_SIZE, Chunk};

    fn setup() -> (BlockRegistry, Vec<[u32; 6]>) {
        let reg = BlockRegistry::with_builtin_blocks();
        // Give every block/face a distinct layer so we can check lookups.
        let layers = (0..reg.len() as u32)
            .map(|b| std::array::from_fn(|f| b * 10 + f as u32))
            .collect();
        (reg, layers)
    }

    fn mesh(c: &Chunk) -> ChunkMeshData {
        let (reg, layers) = setup();
        let m = mesh_chunk(&PaddedChunk::isolated(c), &reg, &layers);
        assert!(m.translucent.is_empty());
        m.solid
    }

    fn check_lengths(m: &ChunkMeshData) {
        let v = m.positions.len();
        assert_eq!(m.uvs.len(), v);
        assert_eq!(m.layers.len(), v);
        assert_eq!(m.shades.len(), v);
        assert_eq!(m.indices.len(), v / 4 * 6);
    }

    #[test]
    fn empty_chunk_has_no_faces() {
        assert!(mesh(&Chunk::empty()).is_empty());
    }

    #[test]
    fn single_block_has_six_faces() {
        let mut c = Chunk::empty();
        c.set(5, 5, 5, blocks::STONE);
        let m = mesh(&c);
        assert_eq!(m.face_count(), 6);
        check_lengths(&m);
    }

    #[test]
    fn identical_neighbours_merge() {
        // A 1×1×3 bar of stone: greedy meshing gives 6 quads, not 14 faces.
        let mut c = Chunk::empty();
        for x in 3..6 {
            c.set(x, 5, 5, blocks::STONE);
        }
        assert_eq!(mesh(&c).face_count(), 6);
    }

    #[test]
    fn different_blocks_do_not_merge() {
        let mut c = Chunk::empty();
        c.set(5, 5, 5, blocks::STONE);
        c.set(6, 5, 5, blocks::DIRT);
        // 12 faces, minus the 2 that touch.
        assert_eq!(mesh(&c).face_count(), 10);
    }

    #[test]
    fn full_chunk_is_six_big_quads() {
        let m = mesh(&Chunk::filled(blocks::STONE));
        assert_eq!(m.face_count(), 6);
        // Each quad spans the whole 16×16 side.
        let xs: Vec<f32> = m.positions.iter().map(|p| p[0]).collect();
        assert_eq!(xs.iter().cloned().fold(f32::MAX, f32::min), 0.0);
        assert_eq!(
            xs.iter().cloned().fold(f32::MIN, f32::max),
            CHUNK_SIZE as f32
        );
    }

    #[test]
    fn neighbour_lookup_crosses_chunk_border() {
        let (reg, layers) = setup();
        // A full chunk surrounded by stone on all sides draws nothing.
        let padded = PaddedChunk::from_fn(|_, _, _| blocks::STONE);
        assert!(mesh_chunk(&padded, &reg, &layers).is_empty());
    }

    #[test]
    fn uses_per_face_texture_layers() {
        let mut c = Chunk::empty();
        c.set(0, 0, 0, blocks::GRASS);
        let m = mesh(&c);
        let g = blocks::GRASS.0 as u32 * 10;
        let mut seen: Vec<u32> = m.layers.chunks(4).map(|q| q[0]).collect();
        seen.sort();
        assert_eq!(seen, (0..6).map(|f| g + f).collect::<Vec<_>>());
    }

    #[test]
    fn cutout_blocks_hide_only_their_own_kind() {
        // Two glass blocks: the face between them is hidden...
        let mut c = Chunk::empty();
        c.set(5, 5, 5, blocks::GLASS);
        c.set(6, 5, 5, blocks::GLASS);
        assert_eq!(mesh(&c).face_count(), 6);
        // ...but stone behind glass keeps its face (you can see it through
        // the glass), and so does the glass.
        let mut c = Chunk::empty();
        c.set(5, 5, 5, blocks::GLASS);
        c.set(6, 5, 5, blocks::STONE);
        assert_eq!(mesh(&c).face_count(), 11);
    }

    #[test]
    fn corners_against_walls_are_darker() {
        // A floor with one block on it: the floor's top face is darker at
        // the corners touching that block.
        let mut c = Chunk::empty();
        for x in 0..3 {
            for z in 0..3 {
                c.set(x, 0, z, blocks::STONE);
            }
        }
        c.set(1, 1, 1, blocks::DIRT);
        let m = mesh(&c);
        let min_top_shade = m
            .positions
            .iter()
            .zip(&m.shades)
            .filter(|(p, _)| p[1] == 1.0)
            .map(|(_, s)| *s)
            .fold(f32::MAX, f32::min);
        assert!(min_top_shade < 1.0, "expected some occluded corners");
    }

    #[test]
    fn water_goes_in_the_translucent_mesh() {
        let (reg, layers) = setup();
        let mut c = Chunk::empty();
        // A 2×1×1 pool of water on a stone floor.
        c.set(5, 5, 5, blocks::WATER);
        c.set(6, 5, 5, blocks::WATER);
        c.set(5, 4, 5, blocks::STONE);
        let m = mesh_chunk(&PaddedChunk::isolated(&c), &reg, &layers);
        // Water: top, two long sides and two ends merge into 5 quads, plus
        // the bottom of the block not resting on stone. No face between the
        // two water blocks.
        assert_eq!(m.translucent.face_count(), 6);
        // The stone's top face is drawn (seen through the water).
        assert_eq!(m.solid.face_count(), 6);
        check_lengths(&m.translucent);
    }

    #[test]
    fn glowing_blocks_are_emissive() {
        let mut c = Chunk::empty();
        c.set(5, 5, 5, blocks::LAMP);
        let m = mesh(&c);
        assert!(m.shades.iter().all(|&s| s == EMISSIVE_SHADE));
    }

    #[test]
    fn faces_wind_counter_clockwise_from_outside() {
        // For a single block, every triangle's normal (by the right-hand
        // rule) must point away from the block centre, or the GPU would cull
        // the side we want to see.
        let mut c = Chunk::empty();
        c.set(5, 5, 5, blocks::STONE);
        let m = mesh(&c);
        let centre = [5.5f32, 5.5, 5.5];
        for tri in m.indices.chunks(3) {
            let [a, b, cc] = [0, 1, 2].map(|k| m.positions[tri[k] as usize]);
            let sub = |p: [f32; 3], q: [f32; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
            let (e1, e2) = (sub(b, a), sub(cc, a));
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let out = sub(a, centre);
            let dot = n[0] * out[0] + n[1] * out[1] + n[2] * out[2];
            assert!(dot > 0.0, "triangle {tri:?} faces inwards");
        }
    }

    #[test]
    fn padded_chunk_reads_neighbours_from_world() {
        use ourcraft_common::world::{ColumnPos, VoxelWorld};
        let mut w = VoxelWorld::new();
        let col = |b| (0..WORLD_HEIGHT_CHUNKS).map(|_| Chunk::filled(b)).collect();
        w.insert_column(ColumnPos::new(0, 0), col(BlockId::AIR));
        w.insert_column(ColumnPos::new(1, 0), col(blocks::STONE));
        let p = PaddedChunk::from_world(&w, ChunkPos::new(0, 0, 0));
        assert_eq!(p.get(15, 0, 0), BlockId::AIR);
        assert_eq!(p.get(16, 0, 0), blocks::STONE); // from the +X neighbour
        assert_eq!(p.get(0, -1, 0), blocks::BEDROCK); // below the world
        assert_eq!(p.get(-1, 0, 0), BlockId::AIR); // unloaded neighbour
    }
}

//! Turning blocks into triangles.
//!
//! A GPU can't draw "a chunk of blocks"; it draws triangles. Meshing builds
//! the triangle list for a chunk. The key trick is face culling: a face
//! between two opaque blocks can never be seen, so we skip it. For solid
//! ground that removes over 90% of faces. (M2 adds greedy meshing, which also
//! merges neighbouring faces into bigger rectangles.)
//!
//! This module is plain Rust with no Bevy types, so it can run on background
//! threads and is easy to test.

use ourcraft_common::block::{BlockId, BlockRegistry, Face};
use ourcraft_common::chunk::{CHUNK_SIZE_I32, Chunk};

/// Texture-array layer for each face of each block type, indexed as
/// `layers[block_id][face as usize]`.
pub type FaceLayers = [[u32; 6]];

/// Vertex data for one chunk, ready to upload.
#[derive(Default, Debug, Clone)]
pub struct ChunkMeshData {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Texture-array layer per vertex (the same for all 4 corners of a face).
    pub layers: Vec<u32>,
    /// Brightness per vertex, in `0..=1`.
    pub shades: Vec<f32>,
    pub indices: Vec<u32>,
}

impl ChunkMeshData {
    pub fn face_count(&self) -> usize {
        self.positions.len() / 4
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
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

/// The four corners of a unit face, as offsets from the block's minimum
/// corner. Ordered bottom-left, bottom-right, top-right, top-left as seen from
/// outside the block, which makes the triangles counter-clockwise: the GPU
/// uses that winding to tell front faces from back faces and skips drawing
/// the backs.
fn face_corners(face: Face) -> [[f32; 3]; 4] {
    match face {
        Face::PosX => [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
        Face::NegX => [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
        Face::PosY => [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]],
        Face::NegY => [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
        Face::PosZ => [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
        Face::NegZ => [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
    }
}

/// Texture coordinates matching [`face_corners`]. `v = 0` is the top row of
/// the texture, so side textures (like grass) appear the right way up.
const FACE_UVS: [[f32; 2]; 4] = [[0., 1.], [1., 1.], [1., 0.], [0., 0.]];

/// Build the mesh for one chunk.
///
/// `get(x, y, z)` returns the block at chunk-local coordinates. It's called
/// for `-1..=16` on each axis so faces on the chunk border can look at the
/// neighbouring chunk.
pub fn mesh_chunk(
    get: impl Fn(i32, i32, i32) -> BlockId,
    registry: &BlockRegistry,
    layers: &FaceLayers,
) -> ChunkMeshData {
    let mut mesh = ChunkMeshData::default();
    let n = CHUNK_SIZE_I32;

    for y in 0..n {
        for z in 0..n {
            for x in 0..n {
                let block = get(x, y, z);
                if block.is_air() {
                    continue;
                }
                for face in Face::ALL {
                    let [dx, dy, dz] = face.normal();
                    if registry.is_opaque(get(x + dx, y + dy, z + dz)) {
                        continue; // hidden behind a neighbour
                    }
                    let layer = layers.get(block.0 as usize).map_or(0, |l| l[face as usize]);
                    push_face(&mut mesh, [x, y, z], face, layer);
                }
            }
        }
    }
    mesh
}

fn push_face(mesh: &mut ChunkMeshData, pos: [i32; 3], face: Face, layer: u32) {
    let base = mesh.positions.len() as u32;
    let shade = face_shade(face);
    for (corner, uv) in face_corners(face).into_iter().zip(FACE_UVS) {
        mesh.positions.push([
            pos[0] as f32 + corner[0],
            pos[1] as f32 + corner[1],
            pos[2] as f32 + corner[2],
        ]);
        mesh.uvs.push(uv);
        mesh.layers.push(layer);
        mesh.shades.push(shade);
    }
    // Two triangles per square face.
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Convenience for meshing a chunk on its own (everything outside is air).
pub fn mesh_isolated_chunk(
    chunk: &Chunk,
    registry: &BlockRegistry,
    layers: &FaceLayers,
) -> ChunkMeshData {
    mesh_chunk(
        |x, y, z| chunk.get_checked(x, y, z).unwrap_or(BlockId::AIR),
        registry,
        layers,
    )
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

    #[test]
    fn empty_chunk_has_no_faces() {
        let (reg, layers) = setup();
        assert!(mesh_isolated_chunk(&Chunk::empty(), &reg, &layers).is_empty());
    }

    #[test]
    fn single_block_has_six_faces() {
        let (reg, layers) = setup();
        let mut c = Chunk::empty();
        c.set(5, 5, 5, blocks::STONE);
        let m = mesh_isolated_chunk(&c, &reg, &layers);
        assert_eq!(m.face_count(), 6);
        assert_eq!(m.positions.len(), 24);
        assert_eq!(m.indices.len(), 36);
        assert_eq!(m.uvs.len(), 24);
        assert_eq!(m.layers.len(), 24);
        assert_eq!(m.shades.len(), 24);
    }

    #[test]
    fn touching_blocks_hide_shared_faces() {
        let (reg, layers) = setup();
        let mut c = Chunk::empty();
        c.set(5, 5, 5, blocks::STONE);
        c.set(6, 5, 5, blocks::DIRT);
        // 12 faces minus the 2 that touch.
        assert_eq!(mesh_isolated_chunk(&c, &reg, &layers).face_count(), 10);
    }

    #[test]
    fn full_chunk_only_draws_its_surface() {
        let (reg, layers) = setup();
        let c = Chunk::filled(blocks::STONE);
        let faces = 6 * CHUNK_SIZE * CHUNK_SIZE;
        assert_eq!(mesh_isolated_chunk(&c, &reg, &layers).face_count(), faces);
    }

    #[test]
    fn neighbour_lookup_crosses_chunk_border() {
        let (reg, layers) = setup();
        // A full chunk surrounded by stone on all sides draws nothing.
        let m = mesh_chunk(|_, _, _| blocks::STONE, &reg, &layers);
        assert!(m.is_empty());
    }

    #[test]
    fn uses_per_face_texture_layers() {
        let (reg, layers) = setup();
        let mut c = Chunk::empty();
        c.set(0, 0, 0, blocks::GRASS);
        let m = mesh_isolated_chunk(&c, &reg, &layers);
        let g = blocks::GRASS.0 as u32 * 10;
        let mut seen: Vec<u32> = m.layers.chunks(4).map(|q| q[0]).collect();
        seen.sort();
        assert_eq!(seen, (0..6).map(|f| g + f).collect::<Vec<_>>());
    }

    #[test]
    fn faces_wind_counter_clockwise_from_outside() {
        // The triangle normal (by the right-hand rule) must point the same
        // way as the face, or the GPU would cull the face we want to see.
        for face in Face::ALL {
            let c = face_corners(face);
            let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
            let (e1, e2) = (sub(c[1], c[0]), sub(c[2], c[0]));
            let cross = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let n = face.normal().map(|v| v as f32);
            assert_eq!(cross, n, "{face:?}");
        }
    }
}

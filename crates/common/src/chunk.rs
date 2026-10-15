//! Chunk storage.
//!
//! The world is split into 16×16×16 cubes called chunks. Each chunk is
//! generated, meshed, saved and sent over the network as a unit, so the game
//! only has to deal with the chunks near the player.

use crate::block::BlockId;

/// Width, height and depth of a chunk, in blocks.
pub const CHUNK_SIZE: usize = 16;
/// `CHUNK_SIZE` as an `i32`, for coordinate maths.
pub const CHUNK_SIZE_I32: i32 = CHUNK_SIZE as i32;
/// Number of blocks in a chunk.
pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/// A 16×16×16 block of the world.
///
/// Stored densely as one `BlockId` per block (8 KiB per chunk). When we have
/// thousands of chunks loaded in M2 we'll switch to a palette, without
/// changing this API.
#[derive(Clone, PartialEq, Eq)]
pub struct Chunk {
    blocks: Box<[BlockId; CHUNK_VOLUME]>,
    /// Number of non-air blocks, so we can skip empty chunks cheaply.
    solid_count: u16,
}

impl Chunk {
    /// A chunk full of air.
    pub fn empty() -> Self {
        Self::filled(BlockId::AIR)
    }

    /// A chunk where every block is `block`.
    pub fn filled(block: BlockId) -> Self {
        Self {
            blocks: Box::new([block; CHUNK_VOLUME]),
            solid_count: if block.is_air() {
                0
            } else {
                CHUNK_VOLUME as u16
            },
        }
    }

    /// Position inside the flat array. `x` varies fastest, then `z`, then `y`,
    /// so a horizontal layer of the chunk is contiguous in memory.
    #[inline]
    pub fn index(x: usize, y: usize, z: usize) -> usize {
        debug_assert!(x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE);
        x + z * CHUNK_SIZE + y * CHUNK_SIZE * CHUNK_SIZE
    }

    /// Inverse of [`Chunk::index`].
    #[inline]
    pub fn coords(index: usize) -> (usize, usize, usize) {
        let x = index % CHUNK_SIZE;
        let z = (index / CHUNK_SIZE) % CHUNK_SIZE;
        let y = index / (CHUNK_SIZE * CHUNK_SIZE);
        (x, y, z)
    }

    /// Get the block at local coordinates (each in `0..16`).
    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
        self.blocks[Self::index(x, y, z)]
    }

    /// Like [`Chunk::get`] but takes signed coordinates and returns `None`
    /// outside the chunk. Handy for looking at neighbours.
    #[inline]
    pub fn get_checked(&self, x: i32, y: i32, z: i32) -> Option<BlockId> {
        let range = 0..CHUNK_SIZE_I32;
        if range.contains(&x) && range.contains(&y) && range.contains(&z) {
            Some(self.get(x as usize, y as usize, z as usize))
        } else {
            None
        }
    }

    /// Set the block at local coordinates and return the block that was there.
    pub fn set(&mut self, x: usize, y: usize, z: usize, block: BlockId) -> BlockId {
        let slot = &mut self.blocks[Self::index(x, y, z)];
        let old = std::mem::replace(slot, block);
        match (old.is_air(), block.is_air()) {
            (true, false) => self.solid_count += 1,
            (false, true) => self.solid_count -= 1,
            _ => {}
        }
        old
    }

    /// True if the chunk contains only air.
    pub fn is_empty(&self) -> bool {
        self.solid_count == 0
    }

    /// Number of non-air blocks.
    pub fn solid_count(&self) -> usize {
        self.solid_count as usize
    }

    /// Iterate over `((x, y, z), block)` for every block, including air.
    pub fn iter(&self) -> impl Iterator<Item = ((usize, usize, usize), BlockId)> + '_ {
        self.blocks
            .iter()
            .enumerate()
            .map(|(i, &b)| (Self::coords(i), b))
    }
}

impl Default for Chunk {
    fn default() -> Self {
        Self::empty()
    }
}

impl std::fmt::Debug for Chunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Chunk")
            .field("solid_count", &self.solid_count)
            .finish_non_exhaustive()
    }
}

/// Position of a chunk in chunk units: chunk `(1, 0, 0)` covers world blocks
/// `x = 16..32`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, PartialOrd, Ord)]
pub struct ChunkPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl ChunkPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The chunk containing a world block position. Uses floor division so
    /// that block `-1` is in chunk `-1`, not chunk `0`.
    pub fn from_block(x: i32, y: i32, z: i32) -> Self {
        Self::new(
            x.div_euclid(CHUNK_SIZE_I32),
            y.div_euclid(CHUNK_SIZE_I32),
            z.div_euclid(CHUNK_SIZE_I32),
        )
    }

    /// World position of this chunk's minimum corner block.
    pub fn min_block(self) -> [i32; 3] {
        [
            self.x * CHUNK_SIZE_I32,
            self.y * CHUNK_SIZE_I32,
            self.z * CHUNK_SIZE_I32,
        ]
    }
}

/// Split a world block position into (chunk position, local position in chunk).
pub fn split_block_pos(x: i32, y: i32, z: i32) -> (ChunkPos, [usize; 3]) {
    let local = |v: i32| v.rem_euclid(CHUNK_SIZE_I32) as usize;
    (
        ChunkPos::from_block(x, y, z),
        [local(x), local(y), local(z)],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks;

    #[test]
    fn index_and_coords_round_trip() {
        for i in 0..CHUNK_VOLUME {
            let (x, y, z) = Chunk::coords(i);
            assert_eq!(Chunk::index(x, y, z), i);
        }
    }

    #[test]
    fn index_corners() {
        assert_eq!(Chunk::index(0, 0, 0), 0);
        assert_eq!(Chunk::index(15, 15, 15), CHUNK_VOLUME - 1);
        assert_eq!(Chunk::index(1, 0, 0), 1);
        assert_eq!(Chunk::index(0, 0, 1), 16);
        assert_eq!(Chunk::index(0, 1, 0), 256);
    }

    #[test]
    fn set_and_get() {
        let mut c = Chunk::empty();
        assert_eq!(c.get(3, 4, 5), blocks::AIR);
        let old = c.set(3, 4, 5, blocks::STONE);
        assert_eq!(old, blocks::AIR);
        assert_eq!(c.get(3, 4, 5), blocks::STONE);
        // Neighbours are untouched.
        assert_eq!(c.get(4, 4, 5), blocks::AIR);
        assert_eq!(c.get(3, 5, 5), blocks::AIR);
    }

    #[test]
    fn solid_count_tracks_changes() {
        let mut c = Chunk::empty();
        assert!(c.is_empty());
        c.set(0, 0, 0, blocks::DIRT);
        c.set(1, 0, 0, blocks::DIRT);
        assert_eq!(c.solid_count(), 2);
        // Replacing solid with solid doesn't change the count.
        c.set(0, 0, 0, blocks::STONE);
        assert_eq!(c.solid_count(), 2);
        c.set(0, 0, 0, blocks::AIR);
        c.set(1, 0, 0, blocks::AIR);
        assert!(c.is_empty());
        assert_eq!(Chunk::filled(blocks::STONE).solid_count(), CHUNK_VOLUME);
    }

    #[test]
    fn get_checked_rejects_outside() {
        let c = Chunk::filled(blocks::STONE);
        assert_eq!(c.get_checked(0, 0, 0), Some(blocks::STONE));
        assert_eq!(c.get_checked(15, 15, 15), Some(blocks::STONE));
        assert_eq!(c.get_checked(-1, 0, 0), None);
        assert_eq!(c.get_checked(0, 16, 0), None);
    }

    #[test]
    fn chunk_pos_handles_negative_coordinates() {
        assert_eq!(ChunkPos::from_block(0, 0, 0), ChunkPos::new(0, 0, 0));
        assert_eq!(ChunkPos::from_block(15, 15, 15), ChunkPos::new(0, 0, 0));
        assert_eq!(ChunkPos::from_block(16, 0, 0), ChunkPos::new(1, 0, 0));
        assert_eq!(
            ChunkPos::from_block(-1, -16, -17),
            ChunkPos::new(-1, -1, -2)
        );
        assert_eq!(
            split_block_pos(-1, 17, -16),
            (ChunkPos::new(-1, 1, -1), [15, 1, 0])
        );
        assert_eq!(ChunkPos::new(-1, 2, 0).min_block(), [-16, 32, 0]);
    }
}

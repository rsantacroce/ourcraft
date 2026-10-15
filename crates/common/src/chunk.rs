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
/// **Palette compression.** Most chunks contain only a handful of block types
/// (often just one: all air or all stone). So instead of storing a 2-byte id
/// per block, a chunk keeps a small *palette* of the block types it contains
/// and stores, per block, an index into that palette using as few bits as
/// needed: 0 bits for a uniform chunk, 1 bit for two types, 2 bits for up to
/// four, and so on. An all-air chunk takes a few bytes instead of 8 KiB.
#[derive(Clone)]
pub struct Chunk {
    /// Block types present (possibly plus some no longer used, until
    /// [`Chunk::compact`] is called).
    palette: Vec<BlockId>,
    /// Bits per block: 0, 1, 2, 4, 8 or 16. Powers of two never straddle
    /// two `u64` words, which keeps get/set simple.
    bits: u8,
    /// Packed palette indices, `64 / bits` blocks per word.
    data: Vec<u64>,
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
            palette: vec![block],
            bits: 0,
            data: Vec::new(),
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

    #[inline]
    fn palette_index(&self, i: usize) -> usize {
        if self.bits == 0 {
            return 0;
        }
        let bits = self.bits as usize;
        let per_word = 64 / bits;
        let word = self.data[i / per_word];
        let shift = (i % per_word) * bits;
        ((word >> shift) & ((1u64 << bits) - 1)) as usize
    }

    #[inline]
    fn set_palette_index(&mut self, i: usize, value: usize) {
        let bits = self.bits as usize;
        let per_word = 64 / bits;
        let shift = (i % per_word) * bits;
        let mask = ((1u64 << bits) - 1) << shift;
        let word = &mut self.data[i / per_word];
        *word = (*word & !mask) | ((value as u64) << shift);
    }

    /// Get the block at a flat index (see [`Chunk::index`]).
    #[inline]
    pub fn get_index(&self, i: usize) -> BlockId {
        self.palette[self.palette_index(i)]
    }

    /// Get the block at local coordinates (each in `0..16`).
    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
        self.get_index(Self::index(x, y, z))
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
        let i = Self::index(x, y, z);
        let old = self.get_index(i);
        if old == block {
            return old;
        }
        let pi = match self.palette.iter().position(|&b| b == block) {
            Some(pi) => pi,
            None => {
                self.palette.push(block);
                let needed = bits_for(self.palette.len());
                if needed > self.bits {
                    self.repack(needed);
                }
                self.palette.len() - 1
            }
        };
        self.set_palette_index(i, pi);
        match (old.is_air(), block.is_air()) {
            (true, false) => self.solid_count += 1,
            (false, true) => self.solid_count -= 1,
            _ => {}
        }
        old
    }

    /// Re-encode the indices with a different number of bits per block.
    fn repack(&mut self, bits: u8) {
        let old: Vec<usize> = (0..CHUNK_VOLUME).map(|i| self.palette_index(i)).collect();
        self.bits = bits;
        self.data = if bits == 0 {
            Vec::new()
        } else {
            vec![0; CHUNK_VOLUME * bits as usize / 64]
        };
        if bits > 0 {
            for (i, pi) in old.into_iter().enumerate() {
                self.set_palette_index(i, pi);
            }
        }
    }

    /// Drop palette entries that are no longer used and shrink the encoding.
    /// Worth doing before saving or sending a chunk.
    pub fn compact(&mut self) {
        let blocks: Vec<BlockId> = (0..CHUNK_VOLUME).map(|i| self.get_index(i)).collect();
        *self = Self::from_blocks(&blocks);
    }

    /// Build a chunk from one block per position, in [`Chunk::index`] order.
    pub fn from_blocks(blocks: &[BlockId]) -> Self {
        assert_eq!(blocks.len(), CHUNK_VOLUME);
        let mut palette: Vec<BlockId> = Vec::new();
        for &b in blocks {
            if !palette.contains(&b) {
                palette.push(b);
            }
        }
        let mut chunk = Self {
            bits: 0,
            data: Vec::new(),
            solid_count: blocks.iter().filter(|b| !b.is_air()).count() as u16,
            palette,
        };
        let bits = bits_for(chunk.palette.len());
        if bits > 0 {
            chunk.bits = bits;
            chunk.data = vec![0; CHUNK_VOLUME * bits as usize / 64];
            for (i, b) in blocks.iter().enumerate() {
                let pi = chunk.palette.iter().position(|p| p == b).unwrap();
                chunk.set_palette_index(i, pi);
            }
        }
        chunk
    }

    /// The block types this chunk may contain.
    pub fn palette(&self) -> &[BlockId] {
        &self.palette
    }

    /// Bits per block and the packed index words, for serialization.
    pub fn packed(&self) -> (u8, &[u64]) {
        (self.bits, &self.data)
    }

    /// Rebuild a chunk from [`Chunk::palette`] and [`Chunk::packed`]. Returns
    /// `None` if the parts are inconsistent (e.g. corrupt save data).
    pub fn from_packed(palette: Vec<BlockId>, bits: u8, data: Vec<u64>) -> Option<Self> {
        let valid_bits = matches!(bits, 0 | 1 | 2 | 4 | 8 | 16);
        let expected_words = CHUNK_VOLUME * bits as usize / 64;
        if !valid_bits
            || palette.is_empty()
            || data.len() != expected_words
            || palette.len() > (1usize << bits).max(1) && bits < 16
        {
            return None;
        }
        let mut chunk = Self {
            palette,
            bits,
            data,
            solid_count: 0,
        };
        let mut solid = 0u16;
        for i in 0..CHUNK_VOLUME {
            let pi = chunk.palette_index(i);
            let b = *chunk.palette.get(pi)?;
            solid += !b.is_air() as u16;
        }
        chunk.solid_count = solid;
        Some(chunk)
    }

    /// True if the chunk contains only air.
    pub fn is_empty(&self) -> bool {
        self.solid_count == 0
    }

    /// True if every block is the same (and so `get` is the same everywhere).
    pub fn uniform_block(&self) -> Option<BlockId> {
        if self.bits == 0 {
            Some(self.palette[0])
        } else {
            None
        }
    }

    /// Number of non-air blocks.
    pub fn solid_count(&self) -> usize {
        self.solid_count as usize
    }

    /// Iterate over `((x, y, z), block)` for every block, including air.
    pub fn iter(&self) -> impl Iterator<Item = ((usize, usize, usize), BlockId)> + '_ {
        (0..CHUNK_VOLUME).map(|i| (Self::coords(i), self.get_index(i)))
    }

    /// Rough heap memory used, in bytes.
    pub fn heap_size(&self) -> usize {
        self.palette.len() * 2 + self.data.len() * 8
    }
}

/// Smallest supported bit width that can index `palette_len` entries.
fn bits_for(palette_len: usize) -> u8 {
    match palette_len {
        0 | 1 => 0,
        2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        17..=256 => 8,
        _ => 16,
    }
}

impl PartialEq for Chunk {
    fn eq(&self, other: &Self) -> bool {
        (0..CHUNK_VOLUME).all(|i| self.get_index(i) == other.get_index(i))
    }
}

impl Eq for Chunk {}

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

    #[test]
    fn uniform_chunks_are_tiny() {
        let c = Chunk::filled(blocks::STONE);
        assert_eq!(c.heap_size(), 2);
        assert_eq!(c.uniform_block(), Some(blocks::STONE));
    }

    #[test]
    fn palette_grows_through_every_bit_width() {
        // Write more and more distinct block ids and check that every block
        // still reads back correctly after each repack.
        let mut c = Chunk::empty();
        let mut expected = vec![BlockId::AIR; CHUNK_VOLUME];
        for n in 1..300u16 {
            let i = (n as usize * 37) % CHUNK_VOLUME;
            let (x, y, z) = Chunk::coords(i);
            c.set(x, y, z, BlockId(n));
            expected[i] = BlockId(n);
        }
        assert_eq!(c.packed().0, 16);
        for (i, &b) in expected.iter().enumerate() {
            assert_eq!(c.get_index(i), b, "block {i}");
        }
        assert_eq!(c, Chunk::from_blocks(&expected));
    }

    #[test]
    fn compact_shrinks_palette() {
        let mut c = Chunk::empty();
        for n in 1..10u16 {
            c.set(n as usize, 0, 0, BlockId(n));
        }
        for n in 1..10u16 {
            c.set(n as usize, 0, 0, BlockId::AIR);
        }
        c.set(0, 0, 0, blocks::STONE);
        assert_eq!(c.packed().0, 4);
        let before = c.clone();
        c.compact();
        assert_eq!(c.packed().0, 1);
        assert_eq!(c, before);
        assert_eq!(c.solid_count(), 1);
    }

    #[test]
    fn packed_round_trip_and_rejects_garbage() {
        let mut c = Chunk::empty();
        c.set(1, 2, 3, blocks::DIRT);
        c.set(4, 5, 6, blocks::STONE);
        let (bits, data) = c.packed();
        let back = Chunk::from_packed(c.palette().to_vec(), bits, data.to_vec()).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.solid_count(), 2);
        assert!(Chunk::from_packed(vec![], 0, vec![]).is_none());
        assert!(Chunk::from_packed(vec![blocks::AIR], 3, vec![]).is_none());
        assert!(Chunk::from_packed(vec![blocks::AIR, blocks::DIRT], 1, vec![0; 3]).is_none());
        // Index pointing past the palette.
        assert!(Chunk::from_packed(vec![blocks::AIR], 1, vec![u64::MAX; 64]).is_none());
    }
}

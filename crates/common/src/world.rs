//! A collection of loaded chunks, addressed by world block coordinates.
//!
//! The world is infinite horizontally but has a fixed height of 256 blocks
//! (16 chunks). Chunks are loaded and unloaded in vertical *columns*, so a
//! column is the unit of generation, saving and network transfer.
//!
//! The server stacks other dimensions below the overworld in the same
//! world (see [`crate::dimension`]); the `_in` methods take the dimension
//! a column belongs to. Clients only ever hold their own dimension, at the
//! usual heights.

use std::collections::HashMap;

use crate::block::BlockId;
use crate::chunk::{CHUNK_SIZE_I32, Chunk, ChunkPos, split_block_pos};
use crate::dimension::Dimension;

/// Number of chunks stacked in a column.
pub const WORLD_HEIGHT_CHUNKS: i32 = 16;
/// World height in blocks. Valid block `y` values are `0..WORLD_HEIGHT`.
pub const WORLD_HEIGHT: i32 = WORLD_HEIGHT_CHUNKS * CHUNK_SIZE_I32;
/// Real-time seconds for one full day/night cycle.
pub const DAY_LENGTH_SECS: f32 = 20.0 * 60.0;

/// Horizontal position of a column of chunks, in chunk units.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, PartialOrd, Ord)]
pub struct ColumnPos {
    pub x: i32,
    pub z: i32,
}

impl ColumnPos {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// The column containing a world block position.
    pub fn from_block(x: i32, z: i32) -> Self {
        Self::new(x.div_euclid(CHUNK_SIZE_I32), z.div_euclid(CHUNK_SIZE_I32))
    }

    /// The column containing a world-space point (e.g. a player position).
    pub fn from_world(x: f32, z: f32) -> Self {
        Self::from_block(x.floor() as i32, z.floor() as i32)
    }

    pub fn chunk(self, cy: i32) -> ChunkPos {
        ChunkPos::new(self.x, cy, self.z)
    }

    /// Distance in columns, measured as the larger of the x and z distances
    /// (so the loaded area is a square).
    pub fn chebyshev_distance(self, other: ColumnPos) -> i32 {
        (self.x - other.x).abs().max((self.z - other.z).abs())
    }
}

impl From<ChunkPos> for ColumnPos {
    fn from(p: ChunkPos) -> Self {
        Self::new(p.x, p.z)
    }
}

/// All currently loaded chunks.
#[derive(Default)]
pub struct VoxelWorld {
    chunks: HashMap<ChunkPos, Chunk>,
}

impl VoxelWorld {
    pub fn new() -> Self {
        Self::default()
    }

    /// The block at a world position, or `None` if its chunk isn't loaded.
    /// Positions above or below the world (or between dimensions) are air.
    pub fn get_block(&self, x: i32, y: i32, z: i32) -> Option<BlockId> {
        if Dimension::of_block_y(y).is_none() {
            return Some(BlockId::AIR);
        }
        let (cpos, [lx, ly, lz]) = split_block_pos(x, y, z);
        self.chunks.get(&cpos).map(|c| c.get(lx, ly, lz))
    }

    /// The block at a world position, treating unloaded chunks as air.
    pub fn block(&self, x: i32, y: i32, z: i32) -> BlockId {
        self.get_block(x, y, z).unwrap_or(BlockId::AIR)
    }

    /// Change a block. Returns the previous block, or `None` if the position
    /// is outside the world or its chunk isn't loaded.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, block: BlockId) -> Option<BlockId> {
        Dimension::of_block_y(y)?;
        let (cpos, [lx, ly, lz]) = split_block_pos(x, y, z);
        self.chunks.get_mut(&cpos).map(|c| c.set(lx, ly, lz, block))
    }

    pub fn chunk(&self, pos: ChunkPos) -> Option<&Chunk> {
        self.chunks.get(&pos)
    }

    pub fn chunk_mut(&mut self, pos: ChunkPos) -> Option<&mut Chunk> {
        self.chunks.get_mut(&pos)
    }

    pub fn insert_chunk(&mut self, pos: ChunkPos, chunk: Chunk) {
        self.chunks.insert(pos, chunk);
    }

    /// Insert a whole overworld column (`WORLD_HEIGHT_CHUNKS` chunks,
    /// bottom first).
    pub fn insert_column(&mut self, pos: ColumnPos, chunks: Vec<Chunk>) {
        self.insert_column_in(Dimension::Overworld, pos, chunks);
    }

    /// Insert a whole column of a dimension (bottom first).
    pub fn insert_column_in(&mut self, dim: Dimension, pos: ColumnPos, chunks: Vec<Chunk>) {
        debug_assert_eq!(chunks.len(), WORLD_HEIGHT_CHUNKS as usize);
        let base = dim.base_chunk_y();
        for (cy, chunk) in chunks.into_iter().enumerate() {
            self.chunks.insert(pos.chunk(base + cy as i32), chunk);
        }
    }

    /// Remove an overworld column, returning its chunks (bottom first) if
    /// it was loaded.
    pub fn remove_column(&mut self, pos: ColumnPos) -> Option<Vec<Chunk>> {
        self.remove_column_in(Dimension::Overworld, pos)
    }

    pub fn remove_column_in(&mut self, dim: Dimension, pos: ColumnPos) -> Option<Vec<Chunk>> {
        let base = dim.base_chunk_y();
        let mut out = Vec::with_capacity(WORLD_HEIGHT_CHUNKS as usize);
        for cy in 0..WORLD_HEIGHT_CHUNKS {
            out.push(self.chunks.remove(&pos.chunk(base + cy))?);
        }
        Some(out)
    }

    pub fn has_column(&self, pos: ColumnPos) -> bool {
        self.has_column_in(Dimension::Overworld, pos)
    }

    pub fn has_column_in(&self, dim: Dimension, pos: ColumnPos) -> bool {
        self.chunks.contains_key(&pos.chunk(dim.base_chunk_y()))
    }

    /// Iterate over the positions of all loaded overworld columns.
    pub fn columns(&self) -> impl Iterator<Item = ColumnPos> + '_ {
        self.columns_in(Dimension::Overworld)
    }

    /// Iterate over the positions of a dimension's loaded columns.
    pub fn columns_in(&self, dim: Dimension) -> impl Iterator<Item = ColumnPos> + '_ {
        let base = dim.base_chunk_y();
        self.chunks
            .keys()
            .filter(move |p| p.y == base)
            .map(|&p| ColumnPos::from(p))
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Clone an overworld column's chunks (bottom first), e.g. for saving
    /// or sending.
    pub fn column(&self, pos: ColumnPos) -> Option<Vec<Chunk>> {
        self.column_in(Dimension::Overworld, pos)
    }

    pub fn column_in(&self, dim: Dimension, pos: ColumnPos) -> Option<Vec<Chunk>> {
        let base = dim.base_chunk_y();
        (0..WORLD_HEIGHT_CHUNKS)
            .map(|cy| self.chunks.get(&pos.chunk(base + cy)).cloned())
            .collect()
    }
}

/// Anything you can look up blocks in. Physics and raycasts use this so they
/// work both on the client's and the server's copy of the world.
pub trait BlockSource {
    /// The block at a world position, or `None` if it isn't loaded.
    fn get_block(&self, x: i32, y: i32, z: i32) -> Option<BlockId>;
}

impl BlockSource for VoxelWorld {
    fn get_block(&self, x: i32, y: i32, z: i32) -> Option<BlockId> {
        VoxelWorld::get_block(self, x, y, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks;

    fn column(fill: BlockId) -> Vec<Chunk> {
        (0..WORLD_HEIGHT_CHUNKS)
            .map(|_| Chunk::filled(fill))
            .collect()
    }

    #[test]
    fn get_and_set_across_chunks() {
        let mut w = VoxelWorld::new();
        w.insert_column(ColumnPos::new(-1, 0), column(blocks::AIR));
        assert_eq!(w.get_block(-1, 100, 5), Some(blocks::AIR));
        assert_eq!(w.set_block(-1, 100, 5, blocks::STONE), Some(blocks::AIR));
        assert_eq!(w.get_block(-1, 100, 5), Some(blocks::STONE));
        // Lands in chunk (-1, 6, 0) at local (15, 4, 5).
        assert_eq!(
            w.chunk(ChunkPos::new(-1, 6, 0)).unwrap().get(15, 4, 5),
            blocks::STONE
        );
        // Unloaded column.
        assert_eq!(w.get_block(0, 100, 5), None);
        assert_eq!(w.set_block(0, 100, 5, blocks::STONE), None);
        // Outside the world vertically.
        assert_eq!(w.get_block(-1, -1, 5), Some(blocks::AIR));
        assert_eq!(w.get_block(-1, WORLD_HEIGHT, 5), Some(blocks::AIR));
        assert_eq!(w.set_block(-1, WORLD_HEIGHT, 5, blocks::STONE), None);
    }

    #[test]
    fn columns_insert_and_remove() {
        let mut w = VoxelWorld::new();
        let p = ColumnPos::new(3, -7);
        w.insert_column(p, column(blocks::STONE));
        assert!(w.has_column(p));
        assert_eq!(w.chunk_count(), WORLD_HEIGHT_CHUNKS as usize);
        assert_eq!(w.columns().collect::<Vec<_>>(), vec![p]);
        let removed = w.remove_column(p).unwrap();
        assert_eq!(removed.len(), WORLD_HEIGHT_CHUNKS as usize);
        assert!(!w.has_column(p));
        assert_eq!(w.chunk_count(), 0);
    }

    #[test]
    fn dimensions_keep_their_own_columns() {
        let mut w = VoxelWorld::new();
        let p = ColumnPos::new(1, 1);
        let sky = Dimension::CloudIsles;
        w.insert_column_in(sky, p, column(blocks::STONE));
        assert!(w.has_column_in(sky, p) && !w.has_column(p));
        assert_eq!(w.columns_in(sky).collect::<Vec<_>>(), vec![p]);
        assert_eq!(w.columns().count(), 0);
        // Its blocks are down in the Cloud Isles' heights.
        assert_eq!(w.get_block(16, sky.base_y() + 5, 16), Some(blocks::STONE));
        assert_eq!(w.get_block(16, 5, 16), None);
        // Between the dimensions there's only air.
        assert_eq!(w.get_block(16, -100, 16), Some(blocks::AIR));
        assert_eq!(w.column_in(sky, p).unwrap().len(), 16);
        assert!(w.remove_column_in(sky, p).is_some());
        assert_eq!(w.chunk_count(), 0);
    }

    #[test]
    fn column_pos_from_world_floors() {
        assert_eq!(ColumnPos::from_world(-0.5, 15.9), ColumnPos::new(-1, 0));
        assert_eq!(ColumnPos::from_world(16.0, -16.0), ColumnPos::new(1, -1));
        assert_eq!(
            ColumnPos::new(0, 0).chebyshev_distance(ColumnPos::new(-3, 2)),
            3
        );
    }
}

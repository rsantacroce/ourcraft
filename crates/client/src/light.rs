//! Light: sunlight from above, and light from glowing blocks (torches,
//! lamps) spreading out from them.
//!
//! Every spot in the world has two light levels from 0 to 15:
//! - **sky light**: 15 under the open sky, going down by one for each step
//!   it takes around corners into caves and under roofs (two through
//!   leaves and water). At night the shader dims it.
//! - **block light**: the glowing block's level where it is, going down by
//!   one per step away from it. Night doesn't change it.
//!
//! Light never travels more than 15 steps. So to light a chunk exactly, we
//! only need the blocks within 16 of it: the 27 chunks around it, a
//! 48×48×48 "region". We fill in the light with a flood fill (breadth-first
//! search) from every source in the region. This runs on a background
//! thread as part of meshing, so editing a block re-lights the chunks
//! around it for free.

use std::collections::VecDeque;

use ourcraft_common::block::{BlockId, BlockRegistry, RenderKind, blocks};
use ourcraft_common::chunk::{CHUNK_SIZE, CHUNK_SIZE_I32, Chunk, ChunkPos};
use ourcraft_common::world::{ColumnPos, VoxelWorld, WORLD_HEIGHT_CHUNKS};

/// Edge of the light region: a chunk plus 16 blocks on each side.
pub const R: usize = 3 * CHUNK_SIZE;
const RI: i32 = R as i32;
pub const MAX_LIGHT: u8 = 15;

/// The highest non-air block of each `x, z` in a column, or -1 if the
/// whole column is air. Everything above it is open to the sky.
pub type Heights = [i16; CHUNK_SIZE * CHUNK_SIZE];

/// Work out a column's heights (on a background thread, when it arrives).
pub fn column_heights(chunks: &[Chunk]) -> Box<Heights> {
    let mut h = Box::new([-1i16; CHUNK_SIZE * CHUNK_SIZE]);
    for (cy, chunk) in chunks.iter().enumerate().rev() {
        if chunk.is_empty() {
            continue;
        }
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let slot = &mut h[x + z * CHUNK_SIZE];
                if *slot >= 0 {
                    continue;
                }
                if let Some(y) = (0..CHUNK_SIZE)
                    .rev()
                    .find(|&y| !chunk.get(x, y, z).is_air())
                {
                    *slot = (cy * CHUNK_SIZE + y) as i16;
                }
            }
        }
    }
    h
}

/// Find the new top of a column after the block at `y` changed.
pub fn height_after_edit(
    world: &VoxelWorld,
    x: i32,
    z: i32,
    y: i32,
    old: i16,
    block: BlockId,
) -> i16 {
    if !block.is_air() {
        return old.max(y as i16);
    }
    if y as i16 != old {
        return old; // removed something below the top
    }
    // The top block went away: look down for the next one.
    (0..y)
        .rev()
        .find(|&yy| world.block(x, yy, z) != BlockId::AIR)
        .map_or(-1, |yy| yy as i16)
}

/// Everything needed to light (and mesh) one chunk, copied from the world
/// so a background thread can work on it.
pub struct Neighbourhood {
    pub pos: ChunkPos,
    /// The 27 chunks around `pos`, index `dx+1 + (dz+1)*3 + (dy+1)*9`.
    chunks: Vec<Option<Chunk>>,
    /// Heights for the 48×48 columns of the region, `x + z * 48`.
    heights: Vec<i16>,
}

impl Neighbourhood {
    pub fn from_world(
        world: &VoxelWorld,
        heights: &impl Fn(ColumnPos) -> Option<std::sync::Arc<Heights>>,
        pos: ChunkPos,
    ) -> Self {
        let mut chunks = Vec::with_capacity(27);
        for dy in -1..=1 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    chunks.push(
                        world
                            .chunk(ChunkPos::new(pos.x + dx, pos.y + dy, pos.z + dz))
                            .cloned(),
                    );
                }
            }
        }
        let mut hs = vec![-1i16; R * R];
        for dz in -1..=1 {
            for dx in -1..=1 {
                let Some(h) = heights(ColumnPos::new(pos.x + dx, pos.z + dz)) else {
                    continue;
                };
                for z in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        let rx = (dx + 1) as usize * CHUNK_SIZE + x;
                        let rz = (dz + 1) as usize * CHUNK_SIZE + z;
                        hs[rx + rz * R] = h[x + z * CHUNK_SIZE];
                    }
                }
            }
        }
        Self {
            pos,
            chunks,
            heights: hs,
        }
    }

    /// One chunk in the middle of nothing (for tests): open sky above.
    pub fn isolated(chunk: &Chunk) -> Self {
        let mut chunks = vec![None; 27];
        chunks[13] = Some(chunk.clone());
        Self {
            pos: ChunkPos::new(0, 4, 0),
            chunks,
            heights: vec![-1; R * R],
        }
    }

    /// A block by chunk-local coordinates (`-16..32` on each axis).
    #[inline]
    pub fn block(&self, x: i32, y: i32, z: i32) -> BlockId {
        let n = CHUNK_SIZE_I32;
        let (ox, oy, oz) = (x.div_euclid(n), y.div_euclid(n), z.div_euclid(n));
        match &self.chunks[(ox + 1 + (oz + 1) * 3 + (oy + 1) * 9) as usize] {
            Some(c) => c.get(
                (x - ox * n) as usize,
                (y - oy * n) as usize,
                (z - oz * n) as usize,
            ),
            None if self.pos.y + oy < 0 => blocks::BEDROCK,
            None if self.pos.y + oy >= WORLD_HEIGHT_CHUNKS => BlockId::AIR,
            None => BlockId::AIR,
        }
    }
}

/// How much light a block lets through: 0 = none, otherwise how much it
/// costs to pass (air and glass 1; leaves and water 2).
fn light_cost(registry: &BlockRegistry, b: BlockId) -> u8 {
    if registry.is_opaque(b) {
        return 0;
    }
    let def = registry.get(b);
    if def.liquid || (def.render == RenderKind::Cutout && def.solid && def.name.contains("leaves"))
    {
        2
    } else {
        1
    }
}

/// Light levels for a region, packed `sky << 4 | block`, index
/// `x + z * 48 + y * 48 * 48` with each coordinate `local + 16`.
pub struct LightRegion(pub Vec<u8>);

impl LightRegion {
    /// (sky, block) at chunk-local coordinates (`-16..32`).
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        let v = self.0[idx(x + 16, y + 16, z + 16)];
        (v >> 4, v & 15)
    }
}

#[inline]
fn idx(x: i32, y: i32, z: i32) -> usize {
    (x + z * RI + y * RI * RI) as usize
}

/// Fill in sky and block light for the region around `n.pos`.
pub fn compute_light(n: &Neighbourhood, registry: &BlockRegistry) -> LightRegion {
    let total = R * R * R;
    let mut cost = vec![0u8; total];
    let mut sky = vec![0u8; total];
    let mut glow = vec![0u8; total];
    let mut queue: VecDeque<usize> = VecDeque::new();
    let base_y = n.pos.y * CHUNK_SIZE_I32 - 16;

    for y in 0..RI {
        for z in 0..RI {
            for x in 0..RI {
                let b = n.block(x - 16, y - 16, z - 16);
                let i = idx(x, y, z);
                cost[i] = light_cost(registry, b);
                let l = registry.light(b);
                if l > 0 {
                    glow[i] = l;
                }
            }
        }
    }

    // Sky light: full strength straight down until the first block.
    for z in 0..RI {
        for x in 0..RI {
            let top = n.heights[(x + z * RI) as usize] as i32;
            for y in (0..RI).rev() {
                if base_y + y <= top {
                    break;
                }
                let i = idx(x, y, z);
                sky[i] = MAX_LIGHT;
                queue.push_back(i);
            }
        }
    }
    flood(&mut sky, &cost, &mut queue);

    // Block light: from every glowing block.
    queue.extend((0..total).filter(|&i| glow[i] > 0));
    flood(&mut glow, &cost, &mut queue);

    LightRegion(sky.iter().zip(&glow).map(|(s, g)| s << 4 | g).collect())
}

/// Spread light from the queued cells to their neighbours, losing a little
/// at each step.
fn flood(level: &mut [u8], cost: &[u8], queue: &mut VecDeque<usize>) {
    let plane = (RI * RI) as isize;
    while let Some(i) = queue.pop_front() {
        let l = level[i];
        if l <= 1 {
            continue;
        }
        let ii = i as i32;
        let (x, z, y) = (ii % RI, (ii / RI) % RI, ii / (RI * RI));
        let steps: [(bool, isize); 6] = [
            (x > 0, -1),
            (x < RI - 1, 1),
            (z > 0, -(RI as isize)),
            (z < RI - 1, RI as isize),
            (y > 0, -plane),
            (y < RI - 1, plane),
        ];
        for (ok, d) in steps {
            if !ok {
                continue;
            }
            let j = (i as isize + d) as usize;
            let c = cost[j];
            if c == 0 {
                continue;
            }
            let nl = l.saturating_sub(c);
            if nl > level[j] {
                level[j] = nl;
                queue.push_back(j);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light_of(chunk: &Chunk, x: i32, y: i32, z: i32) -> (u8, u8) {
        let reg = BlockRegistry::with_builtin_blocks();
        let n = Neighbourhood::isolated(chunk);
        compute_light(&n, &reg).get(x, y, z)
    }

    #[test]
    fn open_air_is_fully_sky_lit() {
        assert_eq!(light_of(&Chunk::empty(), 5, 5, 5), (15, 0));
    }

    #[test]
    fn a_closed_box_is_dark_inside_and_a_lamp_lights_it() {
        // A hollow stone box (1×1×1 room) in the middle of the chunk.
        let mut c = Chunk::empty();
        for x in 4..=6 {
            for y in 4..=6 {
                for z in 4..=6 {
                    c.set(x, y, z, blocks::STONE);
                }
            }
        }
        c.set(5, 5, 5, BlockId::AIR);
        let mut n = Neighbourhood::isolated(&c);
        // The sky stops at the box's roof.
        for v in n.heights.iter_mut() {
            *v = -1;
        }
        for z in 4..=6 {
            for x in 4..=6 {
                n.heights[(x + 16 + (z + 16) * RI) as usize] = (4 * 16 + 6) as i16;
            }
        }
        let reg = BlockRegistry::with_builtin_blocks();
        assert_eq!(compute_light(&n, &reg).get(5, 5, 5), (0, 0), "dark room");

        // A torch inside lights it up.
        c.set(5, 5, 5, blocks::TORCH);
        let n2 = Neighbourhood {
            chunks: {
                let mut v = vec![None; 27];
                v[13] = Some(c.clone());
                v
            },
            ..n
        };
        assert_eq!(compute_light(&n2, &reg).get(5, 5, 5).1, 14);
    }

    #[test]
    fn light_fades_with_distance() {
        let mut c = Chunk::empty();
        c.set(8, 8, 8, blocks::LAMP);
        // No sky, to look at block light alone.
        let mut n = Neighbourhood::isolated(&c);
        n.heights.iter_mut().for_each(|h| *h = 200);
        let reg = BlockRegistry::with_builtin_blocks();
        let light = compute_light(&n, &reg);
        assert_eq!(light.get(8, 8, 8).1, 15);
        assert_eq!(light.get(9, 8, 8).1, 14);
        assert_eq!(light.get(12, 8, 8).1, 11);
        assert_eq!(light.get(8, 8, 8 + 14).1, 1);
        // Sky is dark everywhere (the "ground" is far above).
        assert_eq!(light.get(0, 0, 0).0, 0);
    }

    #[test]
    fn heights_find_the_top_block() {
        let mut col: Vec<Chunk> = (0..WORLD_HEIGHT_CHUNKS).map(|_| Chunk::empty()).collect();
        col[3].set(2, 7, 4, blocks::STONE);
        col[1].set(2, 1, 4, blocks::STONE);
        let h = column_heights(&col);
        assert_eq!(h[2 + 4 * CHUNK_SIZE], 3 * 16 + 7);
        assert_eq!(h[0], -1);
    }
}

//! A hand-built demo chunk for Milestone 1. Replaced by real world
//! generation in M2.

use crate::block::blocks;
use crate::chunk::{CHUNK_SIZE, Chunk};

/// A little island: stone and dirt layers, a grassy hill, a sandy corner,
/// a tree and a planks platform.
pub fn demo_chunk() -> Chunk {
    let mut c = Chunk::empty();
    let n = CHUNK_SIZE as i32;

    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            // A smooth hill centred near (5, 5), between 4 and 9 blocks tall.
            let dx = x as f32 - 5.0;
            let dz = z as f32 - 5.0;
            let hill = (4.0 - (dx * dx + dz * dz).sqrt() * 0.6).max(0.0);
            let height = 4 + hill.round() as usize;
            let sandy = x + z > 22;

            for y in 0..height {
                let block = if y + 1 == height {
                    if sandy { blocks::SAND } else { blocks::GRASS }
                } else if y + 3 >= height {
                    if sandy { blocks::SAND } else { blocks::DIRT }
                } else {
                    blocks::STONE
                };
                c.set(x, y, z, block);
            }
        }
    }

    // A tree on flat ground at (11, 4, 4).
    let (tx, ty, tz) = (11, 4, 4);
    for dy in 0..4 {
        c.set(tx, ty + dy, tz, blocks::LOG);
    }
    for y in (ty + 3)..=(ty + 5) {
        let r: i32 = if y == ty + 5 { 1 } else { 2 };
        for dx in -r..=r {
            for dz in -r..=r {
                let (x, z) = (tx as i32 + dx, tz as i32 + dz);
                let in_bounds = (0..n).contains(&x) && (0..n).contains(&z);
                let corner = dx.abs() == r && dz.abs() == r;
                if in_bounds && !corner && c.get(x as usize, y, z as usize).is_air() {
                    c.set(x as usize, y, z as usize, blocks::LEAVES);
                }
            }
        }
    }

    // A small planks platform floating over the sand.
    for x in 11..15 {
        for z in 11..15 {
            c.set(x, 8, z, blocks::PLANKS);
        }
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_chunk_has_ground_and_sky() {
        let c = demo_chunk();
        assert_eq!(c.get(0, 0, 0), blocks::STONE);
        assert!(c.get(0, 15, 0).is_air());
        assert_eq!(c.get(11, 5, 4), blocks::LOG);
    }
}

//! Finding the block the player is looking at.
//!
//! We walk along the ray from block to block (the "DDA" or Amanatides–Woo
//! algorithm): at each step, move into whichever neighbouring block the ray
//! reaches first. That visits exactly the blocks the ray passes through, in
//! order, without missing any or testing any twice.

use glam::{IVec3, Vec3};

use crate::block::{BlockId, BlockRegistry, RenderKind};
use crate::world::BlockSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RayHit {
    /// The block that was hit.
    pub block: IVec3,
    pub id: BlockId,
    /// Which face was hit, as a unit vector. `block + normal` is where a new
    /// block would be placed.
    pub normal: IVec3,
}

/// Cast a ray and return the first targetable block (not air or liquid)
/// within `max_distance`. `dir` doesn't need to be normalized.
pub fn raycast(
    world: &impl BlockSource,
    reg: &BlockRegistry,
    origin: Vec3,
    dir: Vec3,
    max_distance: f32,
) -> Option<RayHit> {
    let dir = dir.try_normalize()?;
    let mut block = origin.floor().as_ivec3();
    let step = IVec3::new(
        dir.x.signum() as i32,
        dir.y.signum() as i32,
        dir.z.signum() as i32,
    );
    // Distance along the ray to cross one whole block on each axis.
    let delta = Vec3::new(
        (1.0 / dir.x).abs(),
        (1.0 / dir.y).abs(),
        (1.0 / dir.z).abs(),
    );
    // Distance along the ray to the first block boundary on each axis.
    let first = |o: f32, d: f32, b: i32| {
        if d > 0.0 {
            (b as f32 + 1.0 - o) / d
        } else if d < 0.0 {
            (o - b as f32) / -d
        } else {
            f32::INFINITY
        }
    };
    let mut t_max = Vec3::new(
        first(origin.x, dir.x, block.x),
        first(origin.y, dir.y, block.y),
        first(origin.z, dir.z, block.z),
    );
    let mut normal = IVec3::ZERO;
    let mut t = 0.0;

    while t <= max_distance {
        if let Some(id) = world.get_block(block.x, block.y, block.z) {
            let targetable = reg.render_kind(id) != RenderKind::Invisible && !reg.is_liquid(id);
            if targetable {
                return Some(RayHit { block, id, normal });
            }
        }
        // Step into the next block along whichever axis is closest.
        let axis = if t_max.x < t_max.y && t_max.x < t_max.z {
            0
        } else if t_max.y < t_max.z {
            1
        } else {
            2
        };
        t = t_max[axis];
        t_max[axis] += delta[axis];
        block[axis] += step[axis];
        normal = IVec3::ZERO;
        normal[axis] = -step[axis];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks;
    use crate::chunk::Chunk;
    use crate::world::{ColumnPos, VoxelWorld, WORLD_HEIGHT_CHUNKS};

    fn world_with(blocks_at: &[(IVec3, BlockId)]) -> VoxelWorld {
        let mut w = VoxelWorld::new();
        for cx in -1..1 {
            for cz in -1..1 {
                let col = (0..WORLD_HEIGHT_CHUNKS).map(|_| Chunk::empty()).collect();
                w.insert_column(ColumnPos::new(cx, cz), col);
            }
        }
        for (p, b) in blocks_at {
            w.set_block(p.x, p.y, p.z, *b);
        }
        w
    }

    #[test]
    fn hits_block_in_front_with_correct_face() {
        let reg = BlockRegistry::with_builtin_blocks();
        let w = world_with(&[(IVec3::new(5, 10, 0), blocks::STONE)]);
        let hit = raycast(&w, &reg, Vec3::new(0.5, 10.5, 0.5), Vec3::X, 10.0).unwrap();
        assert_eq!(hit.block, IVec3::new(5, 10, 0));
        assert_eq!(hit.normal, IVec3::new(-1, 0, 0));
        assert_eq!(hit.id, blocks::STONE);
    }

    #[test]
    fn looking_down_hits_top_face() {
        let reg = BlockRegistry::with_builtin_blocks();
        let w = world_with(&[(IVec3::new(-3, 5, -3), blocks::DIRT)]);
        let hit = raycast(
            &w,
            &reg,
            Vec3::new(-1.5, 9.0, -2.5),
            Vec3::new(0.0, -1.0, 0.0),
            10.0,
        );
        assert_eq!(hit, None, "ray at x=-1.5 misses block at x=-3");
        let hit = raycast(
            &w,
            &reg,
            Vec3::new(-2.5, 9.0, -2.5),
            Vec3::new(-0.05, -1.0, -0.05),
            10.0,
        )
        .unwrap();
        assert_eq!(hit.block, IVec3::new(-3, 5, -3));
        assert_eq!(hit.normal, IVec3::Y);
    }

    #[test]
    fn respects_max_distance() {
        let reg = BlockRegistry::with_builtin_blocks();
        let w = world_with(&[(IVec3::new(8, 10, 0), blocks::STONE)]);
        assert!(raycast(&w, &reg, Vec3::new(0.5, 10.5, 0.5), Vec3::X, 5.0).is_none());
        assert!(raycast(&w, &reg, Vec3::new(0.5, 10.5, 0.5), Vec3::X, 9.0).is_some());
    }

    #[test]
    fn passes_through_water() {
        let reg = BlockRegistry::with_builtin_blocks();
        let w = world_with(&[
            (IVec3::new(2, 10, 0), blocks::WATER),
            (IVec3::new(3, 10, 0), blocks::SAND),
        ]);
        let hit = raycast(&w, &reg, Vec3::new(0.5, 10.5, 0.5), Vec3::X, 10.0).unwrap();
        assert_eq!(hit.id, blocks::SAND);
    }
}

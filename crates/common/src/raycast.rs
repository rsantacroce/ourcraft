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
    cast(world, reg, origin, dir, max_distance, false)
}

/// Like [`raycast`], but also stops at still water and lava (the sources
/// a bucket can scoop up).
pub fn raycast_sources(
    world: &impl BlockSource,
    reg: &BlockRegistry,
    origin: Vec3,
    dir: Vec3,
    max_distance: f32,
) -> Option<RayHit> {
    cast(world, reg, origin, dir, max_distance, true)
}

fn cast(
    world: &impl BlockSource,
    reg: &BlockRegistry,
    origin: Vec3,
    dir: Vec3,
    max_distance: f32,
    sources: bool,
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
            if sources && reg.is_fluid_source(id) {
                return Some(RayHit { block, id, normal });
            }
            let targetable = reg.render_kind(id) != RenderKind::Invisible && !reg.is_liquid(id);
            if targetable {
                if reg.shape(id).is_cube() {
                    return Some(RayHit { block, id, normal });
                }
                // Smaller shapes (torches, fences…): the ray must hit one of
                // the boxes.
                let near = |o: [i32; 3]| {
                    world
                        .get_block(block.x + o[0], block.y + o[1], block.z + o[2])
                        .unwrap_or(BlockId::AIR)
                };
                // Wire is easy to hit anywhere in its square.
                let boxes = if reg.is_wire(id) {
                    reg.shape(id).boxes()
                } else {
                    reg.boxes_at(id, near)
                };
                let hit = boxes
                    .into_iter()
                    .filter_map(|(min, max)| {
                        let b = block.as_vec3();
                        ray_box(origin, dir, b + Vec3::from(min), b + Vec3::from(max))
                    })
                    .filter(|(t, _)| *t <= max_distance)
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((_, normal)) = hit {
                    return Some(RayHit { block, id, normal });
                }
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

/// Where a ray enters a box, and the outward direction of the face it
/// enters through.
fn ray_box(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<(f32, IVec3)> {
    let mut t_near = f32::NEG_INFINITY;
    let mut t_far = f32::INFINITY;
    let mut normal = IVec3::ZERO;
    for axis in 0..3 {
        let (o, d) = (origin[axis], dir[axis]);
        if d.abs() < 1e-8 {
            if o < min[axis] || o > max[axis] {
                return None;
            }
            continue;
        }
        let (mut t1, mut t2) = ((min[axis] - o) / d, (max[axis] - o) / d);
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        // The face a ray enters through faces against its direction.
        let n = -(d.signum() as i32);
        if t1 > t_near {
            t_near = t1;
            normal = IVec3::ZERO;
            normal[axis] = n;
        }
        t_far = t_far.min(t2);
        if t_near > t_far {
            return None;
        }
    }
    (t_far >= 0.0).then_some((t_near.max(0.0), normal))
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
    fn small_shapes_are_hit_only_where_they_are() {
        let reg = BlockRegistry::with_builtin_blocks();
        let w = world_with(&[
            (IVec3::new(5, 10, 0), blocks::TORCH),
            (IVec3::new(8, 10, 0), blocks::STONE),
        ]);
        // Straight at the torch's stick (the middle of the block).
        let hit = raycast(&w, &reg, Vec3::new(0.5, 10.3, 0.5), Vec3::X, 20.0).unwrap();
        assert_eq!(hit.id, blocks::TORCH);
        assert_eq!(hit.normal, IVec3::new(-1, 0, 0));
        // Beside it: the ray passes and hits the stone behind.
        let hit = raycast(&w, &reg, Vec3::new(0.5, 10.3, 0.1), Vec3::X, 20.0).unwrap();
        assert_eq!(hit.id, blocks::STONE);
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

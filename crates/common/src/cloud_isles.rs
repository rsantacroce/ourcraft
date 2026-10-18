//! The Cloud Isles: a dreamy sky world of floating islands, reached
//! through a cloud portal (see [`crate::portal`]).
//!
//! **Blocks.** Sky grass on sky soil, over pale skystone; soft clouds that
//! catch you when you fall; sunstone (glass round a gold ingot) for portal
//! frames, and the shimmering cloud portal itself.
//!
//! **Terrain.** There's no ground at all, just sky. A low-frequency noise
//! map says where islands float; where it's high enough, there's an
//! island, thicker the further into the patch you are. Each island has a
//! gently domed top and a deep, pointed belly underneath, like an upside
//! down hill. Another noise lifts and lowers whole islands, so they float
//! at different heights, and a second, higher layer of little islets sits
//! above them. A layer of clouds drifts below everything. Cherry, birch
//! and oak trees, flowers and grass grow on top. Like the overworld it's
//! deterministic from the seed, so only changed columns are saved.

use fastnoise_lite::{FastNoiseLite, FractalType, NoiseType};

use crate::block::{
    BlockDef, BlockId, BlockRegistry, BlockTextures as T, Drops, RenderKind, ToolKind,
};
use crate::chunk::{CHUNK_SIZE, CHUNK_SIZE_I32, CHUNK_VOLUME, Chunk};
use crate::world::{ColumnPos, WORLD_HEIGHT, WORLD_HEIGHT_CHUNKS};
use crate::worldgen::splitmix64;

/// Typical height of the big islands' tops.
pub const ISLAND_LEVEL: i32 = 96;
/// Height of the cloud layer below the islands.
pub const CLOUD_LEVEL: i32 = 40;
/// Where the little islets float.
const ISLET_LEVEL: i32 = 150;
/// How much of the noise map is island (higher = fewer, smaller islands).
const ISLAND_EDGE: f32 = 0.18;
const ISLET_EDGE: f32 = 0.66;

/// The Cloud Isles' own blocks.
pub fn register_blocks(reg: &mut BlockRegistry) {
    use ToolKind::{Pickaxe, Shovel};
    let soil = reg.register(
        BlockDef::solid("core:sky_soil", "Sky Soil", T::all("sky_soil")).mined(
            0.5,
            Some(Shovel),
            0,
        ),
    );
    reg.register(
        BlockDef {
            drops: Drops::Block(soil),
            ..BlockDef::solid(
                "core:sky_grass",
                "Sky Grass",
                T::top_bottom_side("sky_grass_top", "sky_soil", "sky_grass_side"),
            )
        }
        .mined(0.6, Some(Shovel), 0),
    );
    reg.register(
        BlockDef::solid("core:skystone", "Skystone", T::all("skystone")).mined(
            1.5,
            Some(Pickaxe),
            1,
        ),
    );
    // Soft and a little see-through. Landing on one never hurts.
    reg.register(BlockDef {
        render: RenderKind::Translucent,
        ..BlockDef::solid("core:cloud", "Cloud", T::all("cloud")).mined(0.2, Some(Shovel), 0)
    });
    // Glass round a gold ingot: it glows like a little sun.
    reg.register(BlockDef {
        light: 12,
        ..BlockDef::solid("core:sunstone", "Sunstone", T::all("sunstone")).mined(
            1.0,
            Some(Pickaxe),
            0,
        )
    });
    // The inside of a lit portal: walk in, wait a moment, whoosh.
    reg.register(BlockDef {
        render: RenderKind::Translucent,
        solid: false,
        light: 11,
        drops: Drops::Nothing,
        hidden: true,
        ..BlockDef::solid("core:cloud_portal", "Cloud Portal", T::all("cloud_portal"))
            .mined(0.0, None, 0)
    });
}

fn noise(seed: u64, salt: u64, frequency: f32, octaves: i32) -> FastNoiseLite {
    let mut n = FastNoiseLite::with_seed(splitmix64(seed ^ salt) as i32);
    n.set_noise_type(Some(NoiseType::OpenSimplex2));
    n.set_frequency(Some(frequency));
    if octaves > 1 {
        n.set_fractal_type(Some(FractalType::FBm));
        n.set_fractal_octaves(Some(octaves));
    }
    n
}

/// Blocks the generator uses, looked up once by name.
struct SkyBlocks {
    grass: BlockId,
    soil: BlockId,
    stone: BlockId,
    cloud: BlockId,
    tall_grass: BlockId,
    flowers: Vec<BlockId>,
    /// Log and leaves of the woods that grow here.
    trees: Vec<(crate::woods::Wood, BlockId, BlockId)>,
}

impl SkyBlocks {
    fn look_up() -> Self {
        use crate::woods::Wood;
        let reg = BlockRegistry::with_builtin_blocks();
        let id = |n: &str| reg.by_name(n).unwrap_or(BlockId::AIR);
        Self {
            grass: id("core:sky_grass"),
            soil: id("core:sky_soil"),
            stone: id("core:skystone"),
            cloud: id("core:cloud"),
            tall_grass: id("core:tall_grass"),
            flowers: crate::colours::FLOWERS.iter().map(|f| id(f.0)).collect(),
            trees: [Wood::Cherry, Wood::Birch, Wood::Oak]
                .into_iter()
                .map(|w| {
                    let (log, leaves) = crate::woods::tree_blocks(&reg, w);
                    (w, log, leaves)
                })
                .collect(),
        }
    }
}

/// One island's slice through a column: its lowest and highest block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub bottom: i32,
    pub top: i32,
}

/// Generates the Cloud Isles. Cheap to share between threads.
pub struct SkyGenerator {
    seed: u64,
    islands: FastNoiseLite,
    lift: FastNoiseLite,
    belly: FastNoiseLite,
    islets: FastNoiseLite,
    clouds: FastNoiseLite,
    blocks: SkyBlocks,
}

impl SkyGenerator {
    pub fn new(seed: u64) -> Self {
        // Its own seed, so the islands don't line up with the hills below.
        let seed = splitmix64(seed ^ 0x636c_6f75_6473);
        Self {
            seed,
            islands: noise(seed, 1, 0.011, 3),
            lift: noise(seed, 2, 0.004, 2),
            belly: noise(seed, 3, 0.06, 1),
            islets: noise(seed, 4, 0.02, 2),
            clouds: noise(seed, 5, 0.025, 2),
            blocks: SkyBlocks::look_up(),
        }
    }

    /// The islands over a spot, lowest first (none, one or two).
    pub fn spans(&self, x: i32, z: i32) -> Vec<Span> {
        let (fx, fz) = (x as f32, z as f32);
        let mut out = Vec::with_capacity(2);
        let n = self.islands.get_noise_2d(fx, fz);
        if n > ISLAND_EDGE {
            // 0 at the shore, 1 deep inside.
            let inside = ((n - ISLAND_EDGE) / (1.0 - ISLAND_EDGE)).min(1.0);
            let centre = ISLAND_LEVEL as f32 + self.lift.get_noise_2d(fx, fz) * 24.0;
            let top = centre + inside.sqrt() * 9.0;
            let wobble = self.belly.get_noise_2d(fx, fz) * 4.0;
            let bottom = centre - inside * 48.0 - 1.0 + wobble;
            out.push(Span {
                bottom: bottom.round() as i32,
                top: top.round() as i32,
            });
        }
        let m = self.islets.get_noise_2d(fx, fz);
        if m > ISLET_EDGE {
            let inside = ((m - ISLET_EDGE) / (1.0 - ISLET_EDGE)).min(1.0);
            let centre = ISLET_LEVEL as f32 + self.lift.get_noise_2d(fz, fx) * 12.0;
            out.push(Span {
                bottom: (centre - inside * 14.0 - 1.0).round() as i32,
                top: (centre + inside.sqrt() * 3.0).round() as i32,
            });
        }
        // Keep everything inside the world, with the top chunk free.
        out.retain_mut(|s| {
            s.top = s.top.min(WORLD_HEIGHT - 20);
            s.bottom = s.bottom.max(1);
            s.bottom <= s.top
        });
        out
    }

    /// The highest grassy top at a spot, if anything floats there.
    pub fn surface(&self, x: i32, z: i32) -> Option<i32> {
        self.spans(x, z).last().map(|s| s.top)
    }

    /// Is there cloud at this height?
    fn cloud_at(&self, x: i32, y: i32, z: i32) -> bool {
        let thick = (CLOUD_LEVEL - 1..=CLOUD_LEVEL + 1).contains(&y);
        thick && {
            let c = self.clouds.get_noise_2d(x as f32, z as f32);
            // The middle layer is the widest; the others round it off.
            let need = if y == CLOUD_LEVEL { 0.45 } else { 0.6 };
            c > need
        }
    }

    /// Generate one column of chunks, bottom first (at the Cloud Isles' own
    /// heights, 0 to 255).
    pub fn generate_column(&self, pos: ColumnPos) -> Vec<Chunk> {
        let s = CHUNK_SIZE;
        let h = WORLD_HEIGHT as usize;
        let mut col = vec![BlockId::AIR; s * s * h];
        let idx = |x: usize, y: i32, z: usize| x + z * s + y as usize * s * s;
        let (x0, z0) = (pos.x * CHUNK_SIZE_I32, pos.z * CHUNK_SIZE_I32);
        let b = &self.blocks;
        for lz in 0..s {
            for lx in 0..s {
                let (wx, wz) = (x0 + lx as i32, z0 + lz as i32);
                for span in self.spans(wx, wz) {
                    for y in span.bottom..=span.top {
                        let depth = span.top - y;
                        col[idx(lx, y, lz)] = match depth {
                            0 => b.grass,
                            1..=3 => b.soil,
                            _ if self.gold_at(wx, y, wz) => crate::block::blocks::GOLD_ORE,
                            _ => b.stone,
                        };
                    }
                }
                for y in CLOUD_LEVEL - 1..=CLOUD_LEVEL + 1 {
                    if col[idx(lx, y, lz)].is_air() && self.cloud_at(wx, y, wz) {
                        col[idx(lx, y, lz)] = b.cloud;
                    }
                }
            }
        }
        self.place_trees(pos, &mut col);
        self.place_plants(pos, &mut col);
        (0..WORLD_HEIGHT_CHUNKS as usize)
            .map(|cy| Chunk::from_blocks(&col[cy * CHUNK_VOLUME..(cy + 1) * CHUNK_VOLUME]))
            .collect()
    }

    /// A little gold in the skystone (handy for more sunstone).
    fn gold_at(&self, x: i32, y: i32, z: i32) -> bool {
        let cell = splitmix64(
            self.seed
                ^ 0x676f_6c64
                ^ ((x >> 1) as u32 as u64)
                ^ (((y >> 1) as u32 as u64) << 21)
                ^ (((z >> 1) as u32 as u64) << 42),
        );
        cell % 1000 < 6
    }

    /// Trees, rooted on grass. Like the overworld's, a tree near a column
    /// edge is also drawn by the neighbouring column, from the same seed.
    fn place_trees(&self, pos: ColumnPos, col: &mut [BlockId]) {
        const MARGIN: i32 = 3;
        let s = CHUNK_SIZE_I32;
        let (x0, z0) = (pos.x * s, pos.z * s);
        let mut put = |x: i32, y: i32, z: i32, block: BlockId, replace: bool| {
            let (lx, lz) = (x - x0, z - z0);
            if (0..s).contains(&lx) && (0..s).contains(&lz) && (0..WORLD_HEIGHT).contains(&y) {
                let i = (lx + lz * s + y * s * s) as usize;
                if replace || col[i].is_air() {
                    col[i] = block;
                }
            }
        };
        for wz in (z0 - MARGIN)..(z0 + s + MARGIN) {
            for wx in (x0 - MARGIN)..(x0 + s + MARGIN) {
                let r = splitmix64(self.seed ^ splitmix64((wx as u64) << 32 ^ (wz as u32 as u64)));
                if r % 1000 >= 14 {
                    continue;
                }
                let Some(top) = self.surface(wx, wz) else {
                    continue;
                };
                // Not right on the edge, where roots would hang in the air.
                let solid_round = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .all(|(dx, dz)| self.surface(wx + dx, wz + dz).is_some());
                if !solid_round || top + 12 >= WORLD_HEIGHT {
                    continue;
                }
                let (wood, log, leaves) = self.blocks.trees[(r >> 20) as usize % 3];
                for (o, part) in crate::woods::tree_shape(wood, r) {
                    let (block, replace) = match part {
                        crate::woods::Part::Log => (log, true),
                        crate::woods::Part::Leaves => (leaves, false),
                    };
                    put(wx + o[0], top + 1 + o[1], wz + o[2], block, replace);
                }
                put(wx, top, wz, self.blocks.soil, true);
            }
        }
    }

    /// Grass and flowers, one block each.
    fn place_plants(&self, pos: ColumnPos, col: &mut [BlockId]) {
        let s = CHUNK_SIZE;
        let idx = |x: usize, y: i32, z: usize| x + z * s + y as usize * s * s;
        let b = &self.blocks;
        for lz in 0..s {
            for lx in 0..s {
                let (wx, wz) = (
                    pos.x * CHUNK_SIZE_I32 + lx as i32,
                    pos.z * CHUNK_SIZE_I32 + lz as i32,
                );
                for span in self.spans(wx, wz) {
                    let above = span.top + 1;
                    if above >= WORLD_HEIGHT
                        || col[idx(lx, span.top, lz)] != b.grass
                        || !col[idx(lx, above, lz)].is_air()
                    {
                        continue;
                    }
                    let r = splitmix64(
                        self.seed ^ 0x706c_616e ^ ((wx as u64) << 32 ^ (wz as u32 as u64)),
                    );
                    let roll = r % 100;
                    let plant = if roll < 8 {
                        b.tall_grass
                    } else if roll < 12 && !b.flowers.is_empty() {
                        // Flowers in patches of one kind.
                        let patch = splitmix64(
                            self.seed
                                ^ ((wx >> 3) as u32 as u64)
                                ^ (((wz >> 3) as u32 as u64) << 32),
                        );
                        b.flowers[patch as usize % b.flowers.len()]
                    } else {
                        continue;
                    };
                    col[idx(lx, above, lz)] = plant;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_at(col: &[Chunk], x: usize, y: i32, z: usize) -> BlockId {
        col[(y / 16) as usize].get(x, (y % 16) as usize, z)
    }

    #[test]
    fn same_seed_same_islands() {
        let a = SkyGenerator::new(42).generate_column(ColumnPos::new(3, -5));
        let b = SkyGenerator::new(42).generate_column(ColumnPos::new(3, -5));
        assert_eq!(a, b);
        assert_eq!(a.len(), WORLD_HEIGHT_CHUNKS as usize);
    }

    #[test]
    fn different_seeds_differ() {
        let differs = (0..8).any(|i| {
            let p = ColumnPos::new(i, i);
            SkyGenerator::new(1).generate_column(p) != SkyGenerator::new(2).generate_column(p)
        });
        assert!(differs);
    }

    #[test]
    fn islands_float_over_nothing() {
        let g = SkyGenerator::new(7);
        let mut land = 0;
        let mut void = 0;
        for i in 0..400 {
            let (x, z) = (i * 13 - 2600, i * -7 + 900);
            match g.spans(x, z).first() {
                Some(s) => {
                    land += 1;
                    assert!(s.bottom > 0 && s.top < WORLD_HEIGHT - 16, "{s:?}");
                }
                None => void += 1,
            }
        }
        // Plenty of both: islands, and gaps to fall through.
        assert!(land > 60 && void > 60, "land {land}, sky {void}");
        // The bottom of the world is always empty: no floor to land on.
        for c in 0..16 {
            let col = g.generate_column(ColumnPos::new(c, -c));
            assert!(col[0].is_empty(), "something at the very bottom");
            assert!(col.last().unwrap().is_empty());
        }
    }

    #[test]
    fn islands_have_grass_on_top() {
        let g = SkyGenerator::new(5);
        let (x, z) = (0..4000)
            .map(|i| (i * 3, i * 5))
            .find(|&(x, z)| {
                g.spans(x, z).len() == 1 && g.surface(x + 1, z).is_some() && {
                    let r = splitmix64(g.seed ^ splitmix64((x as u64) << 32 ^ (z as u32 as u64)));
                    r % 1000 >= 14 // no tree rooted here
                }
            })
            .expect("no islands at all");
        let col = g.generate_column(ColumnPos::from_block(x, z));
        let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
        let span = g.spans(x, z)[0];
        let top = block_at(&col, lx, span.top, lz);
        assert!(
            top == g.blocks.grass || top == g.blocks.soil,
            "top is {top:?}"
        );
        let deep = block_at(&col, lx, span.bottom, lz);
        assert!(deep == g.blocks.stone || deep == crate::block::blocks::GOLD_ORE);
    }

    #[test]
    fn trees_and_clouds_appear() {
        let g = SkyGenerator::new(9);
        let cherry = g.blocks.trees[0].1;
        let mut trees = false;
        let mut clouds = false;
        for i in 0..100 {
            let col = g.generate_column(ColumnPos::new(i % 10, i / 10));
            trees |= col
                .iter()
                .any(|c| g.blocks.trees.iter().any(|t| c.palette().contains(&t.1)));
            clouds |= col.iter().any(|c| c.palette().contains(&g.blocks.cloud));
        }
        assert_ne!(cherry, BlockId::AIR);
        assert!(trees, "no trees on any island");
        assert!(clouds, "no clouds");
    }
}

//! Procedural terrain, deterministic from a seed: the same seed always
//! produces the same world, on every machine. That's what lets the server
//! generate chunks on demand instead of storing the whole infinite world.
//!
//! **Noise.** "Coherent noise" (here OpenSimplex2) is a random-looking but
//! smooth function: nearby inputs give nearby outputs. Sampled at low
//! frequency it makes rolling hills; layering several frequencies ("fractal"
//! or "fBm" noise) adds detail. We combine a few noise maps:
//! - *continentalness*: broad lowlands vs highlands
//! - *hills*: smaller bumps
//! - *ridges*: sharp mountain ranges
//! - *temperature* and *moisture*: pick the biome
//! - two 3D noises for winding "spaghetti" caves, one for big "cheese" caves

use fastnoise_lite::{FastNoiseLite, FractalType, NoiseType};

use crate::block::{BlockId, blocks};
use crate::chunk::{CHUNK_SIZE, CHUNK_SIZE_I32, CHUNK_VOLUME, Chunk};
use crate::world::{ColumnPos, WORLD_HEIGHT, WORLD_HEIGHT_CHUNKS};

/// Typical ground level.
pub const BASE_HEIGHT: i32 = 64;
/// Lakes and seas fill up to this height.
pub const SEA_LEVEL: i32 = 58;
/// Caves below this height are flooded with lava.
pub const LAVA_LEVEL: i32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    Plains,
    Forest,
    Desert,
    Snowy,
    Mountains,
}

/// Generates terrain. Cheap to share between threads (`&self` only).
pub struct WorldGenerator {
    seed: u64,
    continental: FastNoiseLite,
    hills: FastNoiseLite,
    ridges: FastNoiseLite,
    temperature: FastNoiseLite,
    moisture: FastNoiseLite,
    cave_a: FastNoiseLite,
    cave_b: FastNoiseLite,
    cheese: FastNoiseLite,
    plants: Plants,
}

/// Plant blocks the generator puts in the world. Their numbers come after
/// the older built-in blocks, so they're looked up once by name (built-in
/// blocks always get the same numbers, before any mod's).
struct Plants {
    tall_grass: BlockId,
    fern: BlockId,
    sugar_cane: BlockId,
    mushrooms: [BlockId; 2],
    pumpkin: BlockId,
    melon: BlockId,
    /// Ripe wild crops.
    crops: [BlockId; 4],
    flowers: Vec<BlockId>,
    /// Log and leaves of each wood, in `Wood::ALL` order.
    trees: Vec<(BlockId, BlockId)>,
    granite: BlockId,
    diorite: BlockId,
    andesite: BlockId,
    deep_slate: BlockId,
    sandstone: BlockId,
    ice: BlockId,
    snow_layer: BlockId,
    spark_ore: BlockId,
    beehive: BlockId,
}

impl Plants {
    fn look_up() -> Self {
        let reg = crate::block::BlockRegistry::with_builtin_blocks();
        let id = |n: &str| reg.by_name(n).unwrap_or(BlockId::AIR);
        let ripe = |family: &str| {
            reg.by_name(family)
                .and_then(|b| reg.advanced(b, u8::MAX))
                .unwrap_or(BlockId::AIR)
        };
        Self {
            tall_grass: id("core:tall_grass"),
            fern: id("core:fern"),
            sugar_cane: id("core:sugar_cane"),
            mushrooms: [id("core:red_mushroom"), id("core:brown_mushroom")],
            pumpkin: id("core:pumpkin"),
            melon: id("core:melon"),
            flowers: crate::colours::FLOWERS.iter().map(|f| id(f.0)).collect(),
            trees: crate::woods::Wood::ALL
                .iter()
                .map(|w| crate::woods::tree_blocks(&reg, *w))
                .collect(),
            granite: id("core:granite"),
            diorite: id("core:diorite"),
            andesite: id("core:andesite"),
            deep_slate: id("core:deep_slate"),
            sandstone: id("core:sandstone"),
            ice: id("core:ice"),
            snow_layer: id("core:snow_layer"),
            spark_ore: id("core:spark_ore"),
            beehive: id("core:beehive"),
            crops: [
                ripe("core:wheat_crop"),
                ripe("core:carrot_crop"),
                ripe("core:potato_crop"),
                ripe("core:beetroot_crop"),
            ],
        }
    }
}

fn noise(
    seed: u64,
    salt: u64,
    frequency: f32,
    octaves: i32,
    fractal: FractalType,
) -> FastNoiseLite {
    let mut n = FastNoiseLite::with_seed(splitmix64(seed ^ salt) as i32);
    n.set_noise_type(Some(NoiseType::OpenSimplex2));
    n.set_frequency(Some(frequency));
    if octaves > 1 {
        n.set_fractal_type(Some(fractal));
        n.set_fractal_octaves(Some(octaves));
    }
    n
}

/// A fast, high-quality integer hash ("SplitMix64").
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl WorldGenerator {
    pub fn new(seed: u64) -> Self {
        use FractalType::{FBm, Ridged};
        Self {
            seed,
            continental: noise(seed, 1, 0.0015, 4, FBm),
            hills: noise(seed, 2, 0.012, 3, FBm),
            ridges: noise(seed, 3, 0.0035, 4, Ridged),
            temperature: noise(seed, 4, 0.0011, 2, FBm),
            moisture: noise(seed, 5, 0.0013, 2, FBm),
            cave_a: noise(seed, 6, 0.018, 1, FBm),
            cave_b: noise(seed, 7, 0.018, 1, FBm),
            cheese: noise(seed, 8, 0.011, 2, FBm),
            plants: Plants::look_up(),
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Height of the top solid block and the biome at a world column,
    /// before caves are carved.
    pub fn surface(&self, x: i32, z: i32) -> (i32, Biome) {
        let (fx, fz) = (x as f32, z as f32);
        let cont = self.continental.get_noise_2d(fx, fz);
        let hills = self.hills.get_noise_2d(fx, fz);
        // Ridged noise is high along thin lines: mountain ridges.
        let ridge = self.ridges.get_noise_2d(fx, fz);
        let mountain =
            ((ridge - 0.1) / 0.9).clamp(0.0, 1.0).powi(2) * 75.0 * smoothstep(-0.15, 0.35, cont);

        let height = BASE_HEIGHT as f32 + cont * 22.0 + hills * 6.0 + mountain;
        let height = (height.round() as i32).clamp(4, WORLD_HEIGHT - 16);

        let temp = self.temperature.get_noise_2d(fx, fz);
        let moist = self.moisture.get_noise_2d(fx, fz);
        let biome = if height > 105 {
            Biome::Mountains
        } else if temp < -0.3 {
            Biome::Snowy
        } else if temp > 0.25 && moist < 0.05 {
            Biome::Desert
        } else if moist > 0.1 {
            Biome::Forest
        } else {
            Biome::Plains
        };
        (height, biome)
    }

    /// A good place to spawn: the nearest flat, open, dry land to the origin,
    /// searching outwards in a spiral. Returns the block position to stand on
    /// top of.
    pub fn find_spawn(&self) -> (i32, i32, i32) {
        for r in 0..200i32 {
            for i in -r..=r {
                for (x, z) in [(i, -r), (i, r), (-r, i), (r, i)] {
                    let (wx, wz) = (x * 8, z * 8);
                    let (h, biome) = self.surface(wx, wz);
                    let open = matches!(biome, Biome::Plains | Biome::Desert | Biome::Snowy);
                    if h <= SEA_LEVEL + 2 || !open || self.is_cave(wx, h, wz, h) {
                        continue;
                    }
                    // Flat: all nearby columns within one block of this one.
                    let flat = (-3..=3).all(|dz| {
                        (-3..=3).all(|dx| (self.surface(wx + dx, wz + dz).0 - h).abs() <= 1)
                    });
                    if flat {
                        return (wx, h, wz);
                    }
                }
            }
        }
        (0, self.surface(0, 0).0, 0)
    }

    /// Is this underground position hollowed out by a cave?
    pub fn is_cave(&self, x: i32, y: i32, z: i32, surface: i32) -> bool {
        if y < 5 {
            return false;
        }
        let (fx, fy, fz) = (x as f32, y as f32 * 1.5, z as f32);
        // Spaghetti caves: where two noise fields are both near zero, which
        // happens along thin winding tubes.
        let a = self.cave_a.get_noise_3d(fx, fy, fz);
        let b = self.cave_b.get_noise_3d(fx, fy, fz);
        // Narrower near the surface so caves rarely leave huge holes.
        let width = if y > surface - 4 { 0.045 } else { 0.075 };
        if a.abs() < width && b.abs() < width {
            return true;
        }
        // Cheese caves: big open rooms, deep underground only.
        y < surface - 12 && y < 48 && self.cheese.get_noise_3d(fx, fy, fz) > 0.62
    }

    /// Generate one column of chunks, bottom first.
    pub fn generate_column(&self, pos: ColumnPos) -> Vec<Chunk> {
        let s = CHUNK_SIZE;
        let h = WORLD_HEIGHT as usize;
        // Build the column as one flat array, then cut it into chunks.
        // Index: x + z*16 + y*256 (same layout as a chunk, just taller).
        let mut col = vec![BlockId::AIR; s * s * h];
        let idx = |x: usize, y: usize, z: usize| x + z * s + y * s * s;
        let x0 = pos.x * CHUNK_SIZE_I32;
        let z0 = pos.z * CHUNK_SIZE_I32;

        for lz in 0..s {
            for lx in 0..s {
                let (wx, wz) = (x0 + lx as i32, z0 + lz as i32);
                let (height, biome) = self.surface(wx, wz);
                // Near and under water, keep caves away from the surface so
                // the sea floor doesn't get holes.
                let wet = height <= SEA_LEVEL + 1;
                for y in 0..=height {
                    let depth = height - y;
                    let block = if y == 0 {
                        blocks::BEDROCK
                    } else if !(wet && depth < 6) && self.is_cave(wx, y, wz, height) {
                        // The bottom of the deepest caves is a lake of lava.
                        if y <= LAVA_LEVEL {
                            col[idx(lx, y as usize, lz)] = blocks::LAVA;
                        }
                        continue;
                    } else {
                        let b = surface_block(biome, depth, height);
                        if b == blocks::STONE {
                            self.ore_at(wx, y, wz)
                                .unwrap_or_else(|| self.stone_at(wx, y, wz, biome, depth))
                        } else if wet && depth < 2 && self.clay_at(wx, wz) {
                            blocks::CLAY
                        } else {
                            b
                        }
                    };
                    col[idx(lx, y as usize, lz)] = block;
                }
                for y in (height + 1)..=SEA_LEVEL {
                    // Snowy lakes freeze over.
                    let frozen = y == SEA_LEVEL && biome == Biome::Snowy;
                    col[idx(lx, y as usize, lz)] = if frozen {
                        self.plants.ice
                    } else {
                        blocks::WATER
                    };
                }
            }
        }

        self.place_features(pos, &mut col);
        self.place_plants(pos, &mut col);

        (0..WORLD_HEIGHT_CHUNKS as usize)
            .map(|cy| Chunk::from_blocks(&col[cy * CHUNK_VOLUME..(cy + 1) * CHUNK_VOLUME]))
            .collect()
    }

    /// Ore veins in stone. The world is cut into 2×2×2 cells; a few cells
    /// hold an ore, and most blocks in such a cell are that ore, making small
    /// clumps. Rarer ores only appear deeper down.
    pub fn ore_at(&self, x: i32, y: i32, z: i32) -> Option<BlockId> {
        let h3 = |x: i32, y: i32, z: i32| {
            splitmix64((x as u32 as u64) ^ ((y as u32 as u64) << 21) ^ ((z as u32 as u64) << 42))
        };
        let cell = splitmix64(self.seed ^ 0x6f72_6573 ^ h3(x >> 1, y >> 1, z >> 1));
        let roll = cell % 10_000;
        let ore = if y < 16 && roll < 15 {
            blocks::DIAMOND_ORE
        } else if y < 32 && roll < 45 {
            blocks::GOLD_ORE
        } else if y < 64 && roll < 135 {
            blocks::IRON_ORE
        } else if y < 128 && roll < 300 {
            blocks::COAL_ORE
        } else if y < 20 && roll < 380 {
            self.plants.spark_ore
        } else {
            return None;
        };
        (splitmix64(cell ^ h3(x, y, z)) % 100 < 60).then_some(ore)
    }

    /// What stone is like at a spot: deep slate deep down, patches of
    /// granite, diorite and andesite, sandstone under deserts.
    fn stone_at(&self, x: i32, y: i32, z: i32, biome: Biome, depth: i32) -> BlockId {
        let p = &self.plants;
        if biome == Biome::Desert && depth < 8 {
            return p.sandstone;
        }
        // The change to deep slate wobbles a little.
        let wobble =
            (splitmix64(self.seed ^ 0x6465_6570 ^ (x as u32 as u64) ^ ((z as u32 as u64) << 32))
                % 4) as i32;
        if y < 14 + wobble {
            return p.deep_slate;
        }
        // Blobs: the world cut into 4×4×4 cells, a few cells of each kind.
        let cell = splitmix64(
            self.seed
                ^ 0x726f_636b
                ^ ((x >> 2) as u32 as u64)
                ^ (((y >> 2) as u32 as u64) << 21)
                ^ (((z >> 2) as u32 as u64) << 42),
        );
        match cell % 100 {
            0..3 => p.granite,
            3..6 => p.diorite,
            6..9 => p.andesite,
            _ => blocks::STONE,
        }
    }

    /// Which kind of tree grows here.
    fn wood_at(&self, x: i32, z: i32, biome: Biome) -> crate::woods::Wood {
        use crate::woods::Wood;
        let region = splitmix64(
            self.seed ^ 0x7472_6565 ^ ((x >> 5) as u32 as u64) ^ (((z >> 5) as u32 as u64) << 32),
        ) % 100;
        match biome {
            Biome::Snowy | Biome::Mountains => Wood::Pine,
            Biome::Forest => match region {
                0..40 => Wood::Oak,
                40..70 => Wood::Birch,
                70..88 => Wood::Dark,
                _ => Wood::Jungle,
            },
            _ => match region {
                0..8 => Wood::Cherry,
                8..20 => Wood::Birch,
                _ => Wood::Oak,
            },
        }
    }

    /// Patches of clay under lakes and the sea.
    fn clay_at(&self, x: i32, z: i32) -> bool {
        let patch = splitmix64(
            self.seed ^ 0x636c_6179 ^ ((x >> 2) as u32 as u64) ^ (((z >> 2) as u32 as u64) << 32),
        );
        patch % 100 < 18
    }

    /// Trees and cacti. A tree near a column edge spills into the neighbour,
    /// so each column also checks for trees rooted just outside it and draws
    /// the parts that overlap. Because tree placement only depends on the
    /// seed and position, both columns agree on every tree.
    fn place_features(&self, pos: ColumnPos, col: &mut [BlockId]) {
        const MARGIN: i32 = 3; // max leaf radius
        let s = CHUNK_SIZE_I32;
        let x0 = pos.x * s;
        let z0 = pos.z * s;
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
                let chance = r % 1000;
                // Cheap rejection before computing the surface.
                if chance >= 40 {
                    continue;
                }
                let (height, biome) = self.surface(wx, wz);
                let threshold = match biome {
                    Biome::Forest => 40,
                    Biome::Snowy => 12,
                    Biome::Plains => 4,
                    Biome::Desert => 6,
                    Biome::Mountains => 3,
                };
                if chance >= threshold
                    || height <= SEA_LEVEL + 1
                    || self.is_cave(wx, height, wz, height)
                {
                    continue;
                }
                let base = height + 1;
                let r2 = r >> 16;
                if biome == Biome::Desert {
                    let tall = 1 + (r2 % 3) as i32;
                    for dy in 0..tall {
                        put(wx, base + dy, wz, blocks::CACTUS, false);
                    }
                    continue;
                }
                // A tree of the kind that grows here.
                let wood = self.wood_at(wx, wz, biome);
                let (log, leaves) = self.plants.trees[wood as usize];
                for (o, part) in crate::woods::tree_shape(wood, r) {
                    let (block, replace) = match part {
                        crate::woods::Part::Log => (log, true),
                        crate::woods::Part::Leaves => (leaves, false),
                    };
                    put(wx + o[0], base + o[1], wz + o[2], block, replace);
                }
                // Now and then a bee hive hangs on an oak or birch trunk.
                let hive_tree = matches!(wood, crate::woods::Wood::Oak | crate::woods::Wood::Birch);
                if hive_tree && (r >> 40).is_multiple_of(10) {
                    put(wx + 1, base + 2, wz, self.plants.beehive, false);
                }
                // The ground under a tree is dirt, not grass.
                put(wx, height, wz, blocks::DIRT, true);
            }
        }
    }
}

impl WorldGenerator {
    /// Flowery patches (about one area in three).
    fn meadow(&self, x: i32, z: i32) -> bool {
        splitmix64(
            self.seed ^ 0x666c_6f77 ^ ((x >> 3) as u32 as u64) ^ (((z >> 3) as u32 as u64) << 32),
        )
        .is_multiple_of(3)
    }

    /// Which flower grows in a patch.
    fn flower_kind(&self, x: i32, z: i32) -> usize {
        (splitmix64(
            self.seed ^ 0x6b69_6e64 ^ ((x >> 4) as u32 as u64) ^ (((z >> 4) as u32 as u64) << 32),
        ) >> 8) as usize
    }

    /// Small plants, one block each (so they never cross into the next
    /// column): tall grass, wild crops, pumpkins and melons, sugar cane by
    /// the water, and mushrooms in forests and caves.
    fn place_plants(&self, pos: ColumnPos, col: &mut [BlockId]) {
        let s = CHUNK_SIZE;
        let idx = |x: usize, y: i32, z: usize| x + z * s + y as usize * s * s;
        let p = &self.plants;
        for lz in 0..s {
            for lx in 0..s {
                let (wx, wz) = (
                    pos.x * CHUNK_SIZE_I32 + lx as i32,
                    pos.z * CHUNK_SIZE_I32 + lz as i32,
                );
                let r = splitmix64(
                    self.seed ^ 0x706c_616e ^ splitmix64((wx as u64) << 32 ^ (wz as u32 as u64)),
                );
                let (height, biome) = self.surface(wx, wz);
                if height + 3 >= WORLD_HEIGHT || height < 1 {
                    continue;
                }
                let top = col[idx(lx, height, lz)];
                let above = idx(lx, height + 1, lz);
                let roll = r % 10_000;
                if col[above].is_air() && top == blocks::GRASS {
                    let plant = match biome {
                        Biome::Plains if roll < 1_200 => Some(p.tall_grass),
                        Biome::Plains if roll < 1_230 => {
                            Some(p.crops[(r >> 20) as usize % p.crops.len()])
                        }
                        // Flowers grow in patches of one kind.
                        Biome::Plains | Biome::Forest if roll < 1_600 && self.meadow(wx, wz) => {
                            Some(p.flowers[self.flower_kind(wx, wz) % p.flowers.len()])
                        }
                        Biome::Plains | Biome::Forest if roll >= 9_990 => Some(p.pumpkin),
                        Biome::Forest if roll < 400 => Some(p.tall_grass),
                        Biome::Forest if roll < 600 => Some(p.fern),
                        Biome::Forest if roll < 640 => Some(p.mushrooms[(r >> 20) as usize % 2]),
                        Biome::Forest if roll >= 9_984 => Some(p.melon),
                        _ => None,
                    };
                    if let Some(b) = plant {
                        col[above] = b;
                    }
                }
                // Sugar cane on the shore, right next to the water.
                let shore = height == SEA_LEVEL
                    && matches!(top, b if b == blocks::GRASS || b == blocks::SAND || b == blocks::DIRT);
                // Snow lies on snowy ground.
                if top == blocks::SNOW && col[above].is_air() && roll.is_multiple_of(3) {
                    col[above] = p.snow_layer;
                }
                if shore && col[above].is_air() && roll.is_multiple_of(9) {
                    let wet = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .any(|(dx, dz)| self.surface(wx + dx, wz + dz).0 < SEA_LEVEL);
                    if wet {
                        for dy in 1..=1 + (r >> 24) as i32 % 3 {
                            col[idx(lx, height + dy, lz)] = p.sugar_cane;
                        }
                    }
                }
                // Mushrooms on the floors of caves.
                for y in 2..height - 3 {
                    let here = idx(lx, y, lz);
                    if col[here].is_air()
                        && col[idx(lx, y - 1, lz)] == blocks::STONE
                        && splitmix64(r ^ y as u64).is_multiple_of(500)
                    {
                        col[here] = p.mushrooms[(y as usize) % 2];
                    }
                }
            }
        }
    }
}

/// Which block goes at `depth` blocks below the surface in a biome.
fn surface_block(biome: Biome, depth: i32, height: i32) -> BlockId {
    match biome {
        Biome::Desert => {
            if height < SEA_LEVEL - 3 && depth < 2 {
                blocks::GRAVEL
            } else if depth < 4 {
                blocks::SAND
            } else {
                blocks::STONE
            }
        }
        Biome::Mountains => {
            if depth == 0 && height > 125 {
                blocks::SNOW
            } else if depth < 2 && height < 112 {
                blocks::GRASS
            } else {
                blocks::STONE
            }
        }
        Biome::Plains | Biome::Forest | Biome::Snowy => {
            if height < SEA_LEVEL - 3 {
                // Sea floor.
                if depth < 2 {
                    blocks::GRAVEL
                } else {
                    blocks::STONE
                }
            } else if height <= SEA_LEVEL + 1 {
                // Beach.
                if depth < 3 {
                    blocks::SAND
                } else {
                    blocks::STONE
                }
            } else if depth == 0 {
                if biome == Biome::Snowy {
                    blocks::SNOW
                } else {
                    blocks::GRASS
                }
            } else if depth < 4 {
                blocks::DIRT
            } else {
                blocks::STONE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_world() {
        let a = WorldGenerator::new(42).generate_column(ColumnPos::new(3, -5));
        let b = WorldGenerator::new(42).generate_column(ColumnPos::new(3, -5));
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_differ() {
        let p = ColumnPos::new(0, 0);
        let a = WorldGenerator::new(1).generate_column(p);
        let b = WorldGenerator::new(2).generate_column(p);
        assert_ne!(a, b);
    }

    #[test]
    fn column_shape() {
        let g = WorldGenerator::new(7);
        let col = g.generate_column(ColumnPos::new(-2, 9));
        assert_eq!(col.len(), WORLD_HEIGHT_CHUNKS as usize);
        // Bedrock floor everywhere.
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                assert_eq!(col[0].get(x, 0, z), blocks::BEDROCK);
            }
        }
        // The top chunk is above any terrain.
        assert!(col.last().unwrap().is_empty());
    }

    #[test]
    fn spawn_is_on_dry_land() {
        for seed in 0..5 {
            let g = WorldGenerator::new(seed);
            let (x, y, z) = g.find_spawn();
            assert!(y > SEA_LEVEL, "seed {seed} spawns at {x},{y},{z}");
        }
    }

    #[test]
    fn low_ground_is_flooded() {
        let g = WorldGenerator::new(9);
        // Find a column whose surface is below sea level and check the water.
        let (x, z) = (0..4000)
            .map(|i| (i * 7, i * -3))
            .find(|&(x, z)| g.surface(x, z).0 < SEA_LEVEL - 2)
            .expect("no low ground found");
        let pos = ColumnPos::from_block(x, z);
        let col = g.generate_column(pos);
        let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
        let at = |y: i32| col[(y / 16) as usize].get(lx, (y % 16) as usize, lz);
        assert_eq!(at(SEA_LEVEL), blocks::WATER);
        assert!(at(SEA_LEVEL + 1).is_air());
    }

    #[test]
    fn heights_are_reasonable_and_vary() {
        let g = WorldGenerator::new(123);
        let mut min = i32::MAX;
        let mut max = i32::MIN;
        for i in -100..100 {
            let (h, _) = g.surface(i * 37, i * -53);
            min = min.min(h);
            max = max.max(h);
        }
        assert!(min >= 4 && max < WORLD_HEIGHT - 16);
        assert!(max - min > 10, "terrain is too flat: {min}..{max}");
    }

    #[test]
    fn ores_get_rarer_with_depth() {
        let g = WorldGenerator::new(5);
        let mut count = std::collections::HashMap::new();
        for x in 0..64 {
            for z in 0..64 {
                for y in 1..100 {
                    if let Some(o) = g.ore_at(x, y, z) {
                        *count.entry(o).or_insert(0) += 1;
                        if o == blocks::DIAMOND_ORE {
                            assert!(y < 16);
                        }
                    }
                }
            }
        }
        let n = |b| count.get(&b).copied().unwrap_or(0);
        assert!(n(blocks::COAL_ORE) > n(blocks::IRON_ORE));
        assert!(n(blocks::IRON_ORE) > n(blocks::GOLD_ORE));
        assert!(n(blocks::GOLD_ORE) > 0 && n(blocks::DIAMOND_ORE) > 0);
        // Ores are a small part of the stone.
        assert!(n(blocks::COAL_ORE) < 64 * 64 * 99 / 50);
    }

    #[test]
    fn plants_grow_wild() {
        let g = WorldGenerator::new(5);
        let grass = g.plants.tall_grass;
        assert_ne!(grass, BlockId::AIR);
        let found = (0..6).any(|x| {
            g.generate_column(ColumnPos::new(x, 0))
                .iter()
                .any(|c| c.palette().contains(&grass))
        });
        assert!(found, "no tall grass anywhere");
        let fern = g.plants.fern;
        let found = (0..64).any(|i| {
            g.generate_column(ColumnPos::new(i % 8, i / 8))
                .iter()
                .any(|c| c.palette().contains(&fern))
        });
        assert!(found, "no ferns in the forests");
    }

    #[test]
    fn trees_grow_somewhere() {
        let g = WorldGenerator::new(5);
        let found = (0..64).any(|i| {
            g.generate_column(ColumnPos::new(i % 8, i / 8))
                .iter()
                .any(|c| c.palette().contains(&blocks::LOG))
        });
        assert!(found);
    }
}

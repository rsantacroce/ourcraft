//! Villages: a few little houses round a well, joined by gravel paths,
//! on open plains, deserts and snowfields.
//!
//! **Where.** The world is cut into cells of [`CELL`]×[`CELL`] blocks. A
//! cell may hold one village, its middle chosen from the seed so that the
//! whole village (never more than [`RADIUS`] from the middle) stays inside
//! the cell. A column is always inside one cell, so it only needs to look
//! at that cell's village.
//!
//! **How.** Like trees, the village's plan depends only on the seed and
//! where it is, so every column works out the same plan and draws just
//! the part of it that falls inside itself. Trees don't grow inside a
//! village (see `WorldGenerator::feature_root`).

use crate::block::{BlockId, BlockRegistry, blocks};
use crate::chunk::CHUNK_SIZE_I32;
use crate::shapes::Dir4;
use crate::world::{ColumnPos, WORLD_HEIGHT};
use crate::worldgen::{Biome, SEA_LEVEL, WorldGenerator, splitmix64};

/// Size of the squares the world is cut into, one village at most each.
pub const CELL: i32 = 256;
/// No part of a village is further than this from its middle.
pub const RADIUS: i32 = 36;
/// Out of 100 cells, how many have a village (if the ground suits it).
const CHANCE: u64 = 40;

/// What the houses are built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Oak,
    Pine,
    Sandstone,
}

/// Where a village is: the middle of its well, and the ground height there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub style: Style,
}

/// A house: its footprint (`x0..x0+w`, `z0..z0+d`), the height of its
/// floor, and the wall its door is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct House {
    pub x0: i32,
    pub z0: i32,
    pub w: i32,
    pub d: i32,
    pub floor: i32,
    pub door: Dir4,
}

impl House {
    fn contains(&self, x: i32, z: i32, margin: i32) -> bool {
        x >= self.x0 - margin
            && x < self.x0 + self.w + margin
            && z >= self.z0 - margin
            && z < self.z0 + self.d + margin
    }

    /// The door's (lower) block.
    pub fn door_at(&self) -> (i32, i32) {
        let (mx, mz) = (self.x0 + self.w / 2, self.z0 + self.d / 2);
        match self.door {
            Dir4::North => (mx, self.z0),
            Dir4::South => (mx, self.z0 + self.d - 1),
            Dir4::West => (self.x0, mz),
            Dir4::East => (self.x0 + self.w - 1, mz),
        }
    }
}

/// A whole village, worked out from its site.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub site: Site,
    pub houses: Vec<House>,
    /// Gravel path squares (x, z), from each door to the square round
    /// the well.
    pub paths: Vec<(i32, i32)>,
}

/// The blocks villages are built from, looked up by name once.
pub(crate) struct VillageBlocks {
    /// Per [`Style`]: walls, corner posts, floor, roof, roof slab.
    styles: [[BlockId; 5]; 3],
    /// Per style, per facing (`Dir4::ALL` order): door bottom and top.
    doors: [[[BlockId; 2]; 4]; 3],
    /// Per facing: bed foot and head.
    beds: [[BlockId; 2]; 4],
    glass: BlockId,
    torch: BlockId,
    gravel: BlockId,
    fence: BlockId,
    well_roof: BlockId,
}

impl VillageBlocks {
    pub(crate) fn look_up(reg: &BlockRegistry) -> Self {
        let id = |n: &str| reg.by_name(n).unwrap_or(BlockId::AIR);
        let pine_log = crate::woods::Wood::Pine.log();
        let door = |family: &str| {
            Dir4::ALL.map(|f| {
                [false, true].map(|upper| {
                    reg.find_variant(family, f, false, upper)
                        .unwrap_or(BlockId::AIR)
                })
            })
        };
        Self {
            styles: [
                [
                    id("core:planks"),
                    id("core:log"),
                    blocks::COBBLESTONE,
                    id("core:planks"),
                    id("core:planks_slab"),
                ],
                [
                    id("core:pine_planks"),
                    id(&pine_log),
                    blocks::COBBLESTONE,
                    id("core:pine_planks"),
                    id("core:pine_slab"),
                ],
                [
                    id("core:sandstone"),
                    id("core:cut_sandstone"),
                    id("core:smooth_sandstone"),
                    id("core:smooth_sandstone"),
                    id("core:sandstone_slab"),
                ],
            ],
            doors: [
                door("core:door"),
                door("core:pine_door"),
                door("core:birch_door"),
            ],
            beds: Dir4::ALL.map(|f| {
                [false, true].map(|upper| {
                    reg.find_variant("core:bed", f, false, upper)
                        .unwrap_or(BlockId::AIR)
                })
            }),
            glass: blocks::GLASS,
            torch: id("core:torch"),
            gravel: blocks::GRAVEL,
            fence: id("core:fence"),
            well_roof: id("core:cobblestone_slab"),
        }
    }
}

fn dir_index(d: Dir4) -> usize {
    Dir4::ALL.iter().position(|x| *x == d).unwrap_or(0)
}

fn cell_of(v: i32) -> i32 {
    v.div_euclid(CELL)
}

impl WorldGenerator {
    /// The village in this cell, if there is one.
    pub fn village_site(&self, cell_x: i32, cell_z: i32) -> Option<Site> {
        let r = splitmix64(
            self.seed() ^ 0x7669_6c6c ^ splitmix64((cell_x as u64) << 32 ^ (cell_z as u32 as u64)),
        );
        if r % 100 >= CHANCE {
            return None;
        }
        // Keep the whole village inside the cell.
        let span = (CELL - 2 * RADIUS) as u64;
        let x = cell_x * CELL + RADIUS + ((r >> 8) % span) as i32;
        let z = cell_z * CELL + RADIUS + ((r >> 24) % span) as i32;
        let (y, biome) = self.surface(x, z);
        let style = match biome {
            Biome::Plains => Style::Oak,
            Biome::Snowy => Style::Pine,
            Biome::Desert => Style::Sandstone,
            Biome::Forest | Biome::Mountains => return None,
        };
        if y <= SEA_LEVEL + 1 || y + 12 >= WORLD_HEIGHT {
            return None;
        }
        Some(Site { x, y, z, style })
    }

    /// The village whose middle is within [`RADIUS`] of here, if any.
    pub fn village_near(&self, x: i32, z: i32) -> Option<Site> {
        let site = self.village_site(cell_of(x), cell_of(z))?;
        let (dx, dz) = (x - site.x, z - site.z);
        (dx * dx + dz * dz <= RADIUS * RADIUS).then_some(site)
    }

    /// Lay out the houses and paths of a village.
    pub fn village_plan(&self, site: Site) -> Plan {
        let mut r =
            splitmix64(self.seed() ^ 0x686f_7573 ^ (site.x as u64) << 32 ^ site.z as u32 as u64);
        let mut roll = |n: u64| {
            r = splitmix64(r);
            r % n
        };
        let count = 4 + roll(4) as usize;
        let turn = roll(1000) as f32 / 1000.0;
        let mut houses: Vec<House> = Vec::new();
        for i in 0..count {
            let angle =
                (i as f32 + turn + roll(30) as f32 / 100.0) * std::f32::consts::TAU / count as f32;
            let dist = 13.0 + roll(9) as f32;
            let w = if roll(2) == 0 { 5 } else { 7 };
            let d = if roll(2) == 0 { 5 } else { 7 };
            let hx = site.x + (angle.cos() * dist).round() as i32;
            let hz = site.z + (angle.sin() * dist).round() as i32;
            let floor = self.surface(hx, hz).0;
            let (dx, dz) = (site.x - hx, site.z - hz);
            let door = if dx.abs() >= dz.abs() {
                if dx > 0 { Dir4::East } else { Dir4::West }
            } else if dz > 0 {
                Dir4::South
            } else {
                Dir4::North
            };
            let house = House {
                x0: hx - w / 2,
                z0: hz - d / 2,
                w,
                d,
                floor,
                door,
            };
            let wet = floor <= SEA_LEVEL + 1;
            let steep = (floor - site.y).abs() > 7;
            let crowded = houses.iter().any(|o| {
                (house.x0..house.x0 + w)
                    .any(|x| (house.z0..house.z0 + d).any(|z| o.contains(x, z, 2)))
            });
            let on_square = house.contains(site.x, site.z, 5);
            if !(wet || steep || crowded || on_square || floor + 8 >= WORLD_HEIGHT) {
                houses.push(house);
            }
        }
        // Paths: straight out of each door, then one turn to the square.
        let mut paths = Vec::new();
        let on_square = |x: i32, z: i32| (x - site.x).abs() <= 3 && (z - site.z).abs() <= 3;
        for h in &houses {
            let (dx, dz) = h.door_at();
            let v = h.door.vector();
            let (mut x, mut z) = (dx + v[0], dz + v[2]);
            let x_first = matches!(h.door, Dir4::East | Dir4::West);
            while !on_square(x, z) {
                if !houses.iter().any(|o| o.contains(x, z, 0)) {
                    paths.push((x, z));
                }
                if (x_first && x != site.x) || (!x_first && z == site.z) {
                    x += (site.x - x).signum();
                } else {
                    z += (site.z - z).signum();
                }
            }
        }
        // The square round the well.
        for dz in -3..=3i32 {
            for dx in -3..=3i32 {
                if dx.abs() > 1 || dz.abs() > 1 {
                    paths.push((site.x + dx, site.z + dz));
                }
            }
        }
        Plan {
            site,
            houses,
            paths,
        }
    }

    /// Draw the part of any village that falls in this column.
    pub(crate) fn place_villages(&self, pos: ColumnPos, col: &mut [BlockId]) {
        let x0 = pos.x * CHUNK_SIZE_I32;
        let z0 = pos.z * CHUNK_SIZE_I32;
        let Some(site) = self.village_site(cell_of(x0), cell_of(z0)) else {
            return;
        };
        // Columns this far from the village have nothing to draw.
        let near = |a: i32, mid: i32| a + CHUNK_SIZE_I32 > mid - RADIUS && a < mid + RADIUS + 1;
        if !near(x0, site.x) || !near(z0, site.z) {
            return;
        }
        let plan = self.village_plan(site);
        let mut canvas = Canvas { col, x0, z0 };
        let b = &self.village;
        for &(x, z) in &plan.paths {
            let y = self.surface(x, z).0;
            if y > SEA_LEVEL && y + 2 < WORLD_HEIGHT {
                canvas.put(x, y, z, b.gravel);
                canvas.put(x, y + 1, z, BlockId::AIR);
            }
        }
        self.draw_well(&mut canvas, site);
        for h in &plan.houses {
            self.draw_house(&mut canvas, h, site.style);
        }
    }

    fn draw_well(&self, c: &mut Canvas, s: Site) {
        let b = &self.village;
        let y = s.y;
        for dz in -1..=1i32 {
            for dx in -1..=1i32 {
                let (x, z) = (s.x + dx, s.z + dz);
                c.put(x, y - 2, z, blocks::COBBLESTONE);
                let middle = dx == 0 && dz == 0;
                c.put(
                    x,
                    y - 1,
                    z,
                    if middle {
                        blocks::WATER
                    } else {
                        blocks::COBBLESTONE
                    },
                );
                c.put(
                    x,
                    y,
                    z,
                    if middle {
                        blocks::WATER
                    } else {
                        blocks::COBBLESTONE
                    },
                );
                let corner = dx != 0 && dz != 0;
                for dy in 1..=2 {
                    c.put(x, y + dy, z, if corner { b.fence } else { BlockId::AIR });
                }
                c.put(x, y + 3, z, b.well_roof);
            }
        }
        // A lamp post on one corner of the square, for the night.
        let (lx, lz) = (s.x + 3, s.z + 3);
        let ly = self.surface(lx, lz).0;
        c.put(lx, ly + 1, lz, b.fence);
        c.put(lx, ly + 2, lz, b.fence);
        c.put(lx, ly + 3, lz, b.torch);
    }

    fn draw_house(&self, c: &mut Canvas, h: &House, style: Style) {
        let b = &self.village;
        let [wall, corner, floor, roof, slab] = b.styles[style as usize];
        let f = h.floor;
        let (x1, z1) = (h.x0 + h.w - 1, h.z0 + h.d - 1);
        let (door_x, door_z) = h.door_at();
        let doors = b.doors[style as usize][dir_index(h.door)];
        // Clear the space (and a little round it) of hills and plants.
        for x in h.x0 - 1..=x1 + 1 {
            for z in h.z0 - 1..=z1 + 1 {
                for y in f + 1..(f + 10).min(WORLD_HEIGHT) {
                    c.put(x, y, z, BlockId::AIR);
                }
            }
        }
        for x in h.x0..=x1 {
            for z in h.z0..=z1 {
                // A foundation down to the ground.
                let ground = self.surface(x, z).0;
                for y in (ground + 1).max(f - 8)..f {
                    c.put(x, y, z, blocks::COBBLESTONE);
                }
                c.put(x, f, z, floor);
                let edge_x = x == h.x0 || x == x1;
                let edge_z = z == h.z0 || z == z1;
                if edge_x || edge_z {
                    let along = if edge_z && !edge_x {
                        x - h.x0
                    } else {
                        z - h.z0
                    };
                    let len = if edge_z && !edge_x { h.w } else { h.d };
                    for dy in 1..=3 {
                        let block = if edge_x && edge_z {
                            corner
                        } else if (x, z) == (door_x, door_z) && dy <= 2 {
                            doors[dy as usize - 1]
                        } else if dy == 2 && along % 2 == 0 && along > 0 && along < len - 1 {
                            b.glass
                        } else {
                            wall
                        };
                        c.put(x, f + dy, z, block);
                    }
                }
                c.put(x, f + 4, z, roof);
            }
        }
        // A slab rim round the roof, and a second layer in the middle.
        for x in h.x0 - 1..=x1 + 1 {
            for z in h.z0 - 1..=z1 + 1 {
                if !h.contains(x, z, 0) {
                    c.put(x, f + 4, z, slab);
                } else if h.contains(x, z, -1) {
                    c.put(x, f + 5, z, slab);
                }
            }
        }
        // Inside: a bed, a crafting table and a torch, kept clear of the door.
        let v = h.door.vector();
        let inside_door = (door_x - v[0], door_z - v[2]);
        let corners = [
            (h.x0 + 1, h.z0 + 1),
            (x1 - 1, h.z0 + 1),
            (h.x0 + 1, z1 - 1),
            (x1 - 1, z1 - 1),
        ];
        let free = |p: (i32, i32)| p != inside_door && h.contains(p.0, p.1, -1);
        let mut used = Vec::new();
        'bed: for &foot in &corners {
            for facing in Dir4::ALL {
                let fv = facing.vector();
                let head = (foot.0 + fv[0], foot.1 + fv[2]);
                if free(foot) && free(head) {
                    let [lower, upper] = b.beds[dir_index(facing)];
                    c.put(foot.0, f + 1, foot.1, lower);
                    c.put(head.0, f + 1, head.1, upper);
                    used.extend([foot, head]);
                    break 'bed;
                }
            }
        }
        let mut rest = corners.iter().filter(|p| free(**p) && !used.contains(p));
        if let Some(p) = rest.next() {
            c.put(p.0, f + 1, p.1, blocks::CRAFTING_TABLE);
        }
        if let Some(p) = rest.next() {
            c.put(p.0, f + 1, p.1, b.torch);
        }
    }
}

/// Drawing into one column: positions outside it are skipped.
struct Canvas<'a> {
    col: &'a mut [BlockId],
    x0: i32,
    z0: i32,
}

impl Canvas<'_> {
    fn put(&mut self, x: i32, y: i32, z: i32, block: BlockId) {
        let s = CHUNK_SIZE_I32;
        let (lx, lz) = (x - self.x0, z - self.z0);
        if (0..s).contains(&lx) && (0..s).contains(&lz) && (0..WORLD_HEIGHT).contains(&y) {
            self.col[(lx + lz * s + y * s * s) as usize] = block;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some_villages(generator: &WorldGenerator, n: usize) -> Vec<Site> {
        let mut found = Vec::new();
        for cz in -12..12 {
            for cx in -12..12 {
                if let Some(s) = generator.village_site(cx, cz) {
                    found.push(s);
                    if found.len() == n {
                        return found;
                    }
                }
            }
        }
        found
    }

    #[test]
    fn villages_exist_and_stay_in_their_cell() {
        let generator = WorldGenerator::new(1234);
        let sites = some_villages(&generator, 6);
        assert!(sites.len() >= 3, "only {} villages", sites.len());
        for s in sites {
            let plan = generator.village_plan(s);
            assert!(
                plan.houses.len() >= 2,
                "{s:?} has {} houses",
                plan.houses.len()
            );
            for h in &plan.houses {
                for (x, z) in [(h.x0 - 1, h.z0 - 1), (h.x0 + h.w, h.z0 + h.d)] {
                    assert!((x - s.x).abs() < RADIUS && (z - s.z).abs() < RADIUS);
                    assert_eq!((cell_of(x), cell_of(z)), (cell_of(s.x), cell_of(s.z)));
                }
            }
            for &(x, z) in &plan.paths {
                assert!((x - s.x).abs() <= RADIUS && (z - s.z).abs() <= RADIUS);
            }
            assert_eq!(generator.village_near(s.x + 5, s.z - 5), Some(s));
            // No trees inside.
            for dz in -RADIUS..=RADIUS {
                for dx in -RADIUS..=RADIUS {
                    assert!(
                        dx * dx + dz * dz > RADIUS * RADIUS
                            || generator.feature_root(s.x + dx, s.z + dz).is_none()
                    );
                }
            }
        }
    }

    #[test]
    fn houses_have_doors_beds_and_paths_lead_to_the_well() {
        let generator = WorldGenerator::new(1234);
        let reg = BlockRegistry::with_builtin_blocks();
        let s = some_villages(&generator, 1)[0];
        let plan = generator.village_plan(s);
        // Build the columns the village covers and look at the blocks.
        let block_at = |x: i32, y: i32, z: i32| {
            let pos = ColumnPos {
                x: x.div_euclid(16),
                z: z.div_euclid(16),
            };
            let chunks = generator.generate_column(pos);
            let (lx, lz) = (x.rem_euclid(16), z.rem_euclid(16));
            chunks[(y / 16) as usize].get(lx as usize, (y % 16) as usize, lz as usize)
        };
        let h = plan.houses[0];
        let (dx, dz) = h.door_at();
        let door = block_at(dx, h.floor + 1, dz);
        assert!(
            reg.get(door).name.contains("door"),
            "{}",
            reg.get(door).name
        );
        let beds = (h.x0..h.x0 + h.w)
            .flat_map(|x| (h.z0..h.z0 + h.d).map(move |z| (x, z)))
            .filter(|&(x, z)| reg.get(block_at(x, h.floor + 1, z)).name.contains("bed"))
            .count();
        assert_eq!(beds, 2, "a bed is two blocks");
        assert_eq!(block_at(s.x, s.y, s.z), blocks::WATER, "the well");
        let (px, pz) = plan.paths[0];
        assert_eq!(
            block_at(px, generator.surface(px, pz).0, pz),
            blocks::GRAVEL
        );
    }
}

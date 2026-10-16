//! Flowing water and lava, and blocks that fall (sand and gravel).
//!
//! Water you pour (or that's freed by digging) spreads: straight down as a
//! "falling" column, and sideways along the ground, one step lower each
//! block, up to 7 blocks (lava: 3, and slower). Cut off from its source it
//! dries up again. Lava that meets water turns to stone.
//!
//! [`next_state`] says what a spot should become given its neighbours; the
//! server calls it whenever something nearby changes.

use serde::{Deserialize, Serialize};

use crate::block::{
    BlockDef, BlockId, BlockRegistry, BlockShape, BlockTextures as T, Drops, RenderKind, blocks,
};

/// A block that's part of a fluid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fluid {
    /// `"core:water"` or `"core:lava"`.
    pub family: String,
    /// 0 for the source, then 1, 2, … as it flows further away.
    pub level: u8,
    /// Pouring straight down.
    pub falling: bool,
}

pub const WATER_RANGE: u8 = 7;
pub const LAVA_RANGE: u8 = 3;

fn range(family: &str) -> u8 {
    if family == "core:lava" {
        LAVA_RANGE
    } else {
        WATER_RANGE
    }
}

/// Seconds between steps of a flow (lava is slow and sticky).
pub fn delay_ticks(family: &str) -> u64 {
    if family == "core:lava" { 15 } else { 4 }
}

/// Add flowing water and lava (the water source is already registered).
pub fn register_fluids(reg: &mut BlockRegistry) {
    let water_src = blocks::WATER;
    reg.defs_mut(water_src).fluid = Some(Fluid {
        family: "core:water".into(),
        level: 0,
        falling: false,
    });
    let flow = |family: &str, display: &str, tex: &str, level: u8, falling: bool, step: u8| {
        let name = if falling {
            format!("{family}_falling")
        } else {
            format!("{family}_flow_{level}")
        };
        let lava = family == "core:lava";
        BlockDef {
            render: if lava {
                RenderKind::Cutout
            } else {
                RenderKind::Translucent
            },
            solid: false,
            breakable: false,
            liquid: true,
            light: if lava { 13 } else { 0 },
            drops: Drops::Nothing,
            hidden: true,
            shape: if falling {
                BlockShape::Cube
            } else {
                BlockShape::Boxes(vec![[0, 0, 0, 16, 16 - step * level, 16]])
            },
            fluid: Some(Fluid {
                family: family.into(),
                level,
                falling,
            }),
            ..BlockDef::solid(&name, display, T::all(tex))
        }
    };
    for level in 1..=WATER_RANGE {
        reg.register(flow("core:water", "Water", "water", level, false, 2));
    }
    reg.register(flow("core:water", "Water", "water", 1, true, 2));
    reg.register(BlockDef {
        solid: false,
        breakable: false,
        liquid: true,
        light: 15,
        drops: Drops::Nothing,
        fluid: Some(Fluid {
            family: "core:lava".into(),
            level: 0,
            falling: false,
        }),
        ..BlockDef::solid("core:lava", "Lava", T::all("lava"))
    });
    for level in 1..=LAVA_RANGE {
        reg.register(flow("core:lava", "Lava", "lava", level, false, 4));
    }
    reg.register(flow("core:lava", "Lava", "lava", 1, true, 4));
}

impl BlockRegistry {
    pub fn fluid(&self, b: BlockId) -> Option<&Fluid> {
        self.get(b).fluid.as_ref()
    }

    /// The flowing (or falling) block of a fluid family.
    fn flowing(&self, family: &str, level: u8, falling: bool) -> Option<BlockId> {
        self.iter()
            .find(|(_, d)| {
                d.fluid.as_ref().is_some_and(|f| {
                    f.family == family && f.falling == falling && (falling || f.level == level)
                })
            })
            .map(|(id, _)| id)
    }

    /// Can a fluid flow into (or a falling block fall into) this block?
    pub fn is_replaceable(&self, b: BlockId) -> bool {
        b.is_air() || self.fluid(b).is_some_and(|f| f.level > 0 || f.falling)
    }

    /// Still water or lava: what a bucket picks up.
    pub fn is_fluid_source(&self, b: BlockId) -> bool {
        self.fluid(b).is_some_and(|f| f.level == 0 && !f.falling)
    }

    /// The source block of a fluid family (`"core:water"`).
    pub fn fluid_source(&self, family: &str) -> Option<BlockId> {
        self.iter()
            .find(|(_, d)| {
                d.fluid
                    .as_ref()
                    .is_some_and(|f| f.family == family && f.level == 0 && !f.falling)
            })
            .map(|(id, _)| id)
    }

    pub fn falls(&self, b: BlockId) -> bool {
        self.get(b).falls
    }

    /// How high the surface of a liquid block is (0–1), or 0 if it isn't
    /// liquid.
    pub fn liquid_height(&self, b: BlockId) -> f32 {
        if !self.is_liquid(b) {
            return 0.0;
        }
        self.shape(b)
            .boxes()
            .iter()
            .map(|(_, max)| max[1])
            .fold(0.0, f32::max)
    }
}

/// What the block at a spot should become, looking at its neighbours
/// (`get` takes an offset). `None` means "stay as it is".
pub fn next_state(reg: &BlockRegistry, get: impl Fn([i32; 3]) -> BlockId) -> Option<BlockId> {
    let here = get([0, 0, 0]);
    let fluid_here = reg.fluid(here).cloned();
    let sides = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
    let touches = |family: &str| {
        sides
            .iter()
            .chain([[0, 1, 0], [0, -1, 0]].iter())
            .any(|o| reg.fluid(get(*o)).is_some_and(|f| f.family == family))
    };

    // Lava next to water hardens.
    if let Some(f) = &fluid_here
        && f.family == "core:lava"
        && touches("core:water")
    {
        return Some(if f.level == 0 && !f.falling {
            blocks::STONE
        } else {
            blocks::COBBLESTONE
        });
    }
    // Sources stay; so do solid blocks.
    if fluid_here
        .as_ref()
        .is_some_and(|f| f.level == 0 && !f.falling)
    {
        return None;
    }
    if !reg.is_replaceable(here) {
        return None;
    }

    // What would flow in here? Pouring from above beats spreading.
    let mut best: Option<(String, u8, bool)> = None;
    if let Some(f) = reg.fluid(get([0, 1, 0])) {
        best = Some((f.family.clone(), 1, true));
    } else {
        for o in sides {
            let Some(f) = reg.fluid(get(o)) else { continue };
            // A flow only spreads sideways along something it rests on.
            let under = get([o[0], -1, o[2]]);
            let resting = (f.level == 0 && !f.falling) || !reg.is_replaceable(under);
            if !resting {
                continue;
            }
            let level = if f.falling { 1 } else { f.level + 1 };
            if level > range(&f.family) {
                continue;
            }
            if best.as_ref().is_none_or(|b| level < b.1) {
                best = Some((f.family.clone(), level, false));
            }
        }
    }
    let new = match best {
        Some((family, level, falling)) => {
            // Lava flowing into water's way (or water into lava's) hardens.
            let other = if family == "core:lava" {
                "core:water"
            } else {
                "core:lava"
            };
            if touches(other) {
                Some(blocks::COBBLESTONE)
            } else {
                reg.flowing(&family, level, falling)
            }
        }
        None => fluid_here.is_some().then_some(BlockId::AIR),
    };
    new.filter(|n| *n != here)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A tiny world: a stone floor at y = 0, air above, plus some blocks.
    struct Mini(HashMap<[i32; 3], BlockId>);

    impl Mini {
        fn get(&self, p: [i32; 3]) -> BlockId {
            self.0.get(&p).copied().unwrap_or(if p[1] <= 0 {
                blocks::STONE
            } else {
                BlockId::AIR
            })
        }

        /// Update every spot near the fluid until nothing changes.
        fn settle(&mut self, reg: &BlockRegistry) {
            for _ in 0..40 {
                let mut changes = Vec::new();
                for x in -10..=10 {
                    for y in 1..=6 {
                        for z in -2..=2 {
                            let get = |o: [i32; 3]| self.get([x + o[0], y + o[1], z + o[2]]);
                            if let Some(b) = next_state(reg, get) {
                                changes.push(([x, y, z], b));
                            }
                        }
                    }
                }
                if changes.is_empty() {
                    return;
                }
                for (p, b) in changes {
                    self.0.insert(p, b);
                }
            }
        }
    }

    #[test]
    fn water_spreads_seven_blocks_and_stops() {
        let reg = BlockRegistry::with_builtin_blocks();
        let mut w = Mini(HashMap::from([([0, 1, 0], blocks::WATER)]));
        w.settle(&reg);
        let level = |x| reg.fluid(w.get([x, 1, 0])).map(|f| f.level);
        assert_eq!(level(0), Some(0));
        assert_eq!(level(3), Some(3));
        assert_eq!(level(7), Some(7));
        assert_eq!(level(8), None, "no further than 7");
    }

    #[test]
    fn water_pours_down_and_dries_up_without_a_source() {
        let reg = BlockRegistry::with_builtin_blocks();
        let mut w = Mini(HashMap::from([([0, 5, 0], blocks::WATER)]));
        // A ledge under the source, so it spreads off the edge and falls.
        w.0.insert([0, 4, 0], blocks::STONE);
        w.settle(&reg);
        assert!(
            reg.fluid(w.get([1, 3, 0])).is_some_and(|f| f.falling),
            "falls past the ledge"
        );
        assert!(reg.fluid(w.get([1, 1, 0])).is_some(), "reaches the ground");
        // Take the source away: everything drains.
        w.0.insert([0, 5, 0], BlockId::AIR);
        w.settle(&reg);
        assert!((-10..=10).all(|x| reg.fluid(w.get([x, 1, 0])).is_none()));
    }

    #[test]
    fn lava_is_shorter_and_hardens_in_water() {
        let reg = BlockRegistry::with_builtin_blocks();
        let lava = reg.by_name("core:lava").unwrap();
        let mut w = Mini(HashMap::from([([0, 1, 0], lava)]));
        w.settle(&reg);
        assert!(reg.fluid(w.get([3, 1, 0])).is_some());
        assert!(reg.fluid(w.get([4, 1, 0])).is_none());
        // Pour water next to it.
        w.0.insert([-1, 1, 0], blocks::WATER);
        w.settle(&reg);
        assert_eq!(w.get([0, 1, 0]), blocks::STONE);
    }

    #[test]
    fn sand_and_gravel_fall() {
        let reg = BlockRegistry::with_builtin_blocks();
        assert!(reg.falls(blocks::SAND) && reg.falls(blocks::GRAVEL));
        assert!(!reg.falls(blocks::DIRT));
    }
}

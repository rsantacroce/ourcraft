//! Trees and wood (milestone 19): oak (the original `core:log`,
//! `core:planks`, `core:leaves`) plus birch, pine, jungle, cherry and dark
//! wood. Each wood has logs, planks, leaves, a sapling, stairs, a slab, a
//! fence, a fence gate, a door, a trapdoor and a boat.
//!
//! Tree shapes live here too, so the world generator and a growing sapling
//! make the same trees.

use crate::block::{
    BlockDef, BlockId, BlockRegistry, BlockShape, BlockTextures as T, Drops, RenderKind, ToolKind,
};
use crate::farming::{Stage, StageKind};
use crate::worldgen::splitmix64;

/// A kind of tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wood {
    Oak,
    Birch,
    Pine,
    Jungle,
    Cherry,
    Dark,
}

impl Wood {
    pub const ALL: [Wood; 6] = [
        Wood::Oak,
        Wood::Birch,
        Wood::Pine,
        Wood::Jungle,
        Wood::Cherry,
        Wood::Dark,
    ];

    /// Used in names: `core:birch_planks`.
    pub fn key(self) -> &'static str {
        match self {
            Wood::Oak => "oak",
            Wood::Birch => "birch",
            Wood::Pine => "pine",
            Wood::Jungle => "jungle",
            Wood::Cherry => "cherry",
            Wood::Dark => "dark",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Wood::Oak => "Oak",
            Wood::Birch => "Birch",
            Wood::Pine => "Pine",
            Wood::Jungle => "Jungle",
            Wood::Cherry => "Cherry",
            Wood::Dark => "Dark",
        }
    }

    pub fn from_key(key: &str) -> Option<Wood> {
        Wood::ALL.into_iter().find(|w| w.key() == key)
    }

    /// Block names. Oak keeps the original names.
    pub fn log(self) -> String {
        self.named("log", "core:log")
    }
    pub fn planks(self) -> String {
        self.named("planks", "core:planks")
    }
    pub fn leaves(self) -> String {
        self.named("leaves", "core:leaves")
    }
    pub fn sapling(self) -> String {
        format!("core:{}_sapling", self.key())
    }
    pub fn stairs(self) -> String {
        self.named("stairs", "core:planks_stairs")
    }
    pub fn slab(self) -> String {
        self.named("slab", "core:planks_slab")
    }
    pub fn fence(self) -> String {
        self.named("fence", "core:fence")
    }
    pub fn fence_gate(self) -> String {
        self.named("fence_gate", "core:fence_gate")
    }
    pub fn door(self) -> String {
        self.named("door", "core:door")
    }
    pub fn trapdoor(self) -> String {
        self.named("trapdoor", "core:trapdoor")
    }
    pub fn boat(self) -> String {
        format!("core:{}_boat", self.key())
    }

    fn named(self, part: &str, oak: &str) -> String {
        if self == Wood::Oak {
            oak.to_owned()
        } else {
            format!("core:{}_{part}", self.key())
        }
    }

    /// Texture names (oak's are the originals).
    pub fn texture(self, part: &str) -> String {
        if self == Wood::Oak {
            part.to_owned()
        } else {
            format!("{part}_{}", self.key())
        }
    }
}

/// Which wood a block or item belongs to, from its name.
pub fn wood_of(name: &str) -> Option<Wood> {
    let rest = name.strip_prefix("core:")?;
    Wood::ALL
        .into_iter()
        .find(|w| rest.starts_with(&format!("{}_", w.key())))
}

/// Add the new woods (milestone 19). Oak's blocks are already there, apart
/// from its sapling.
pub fn register_woods(reg: &mut BlockRegistry) {
    use crate::shapes::{
        register_door, register_fence, register_fence_gate, register_stairs_and_slab,
        register_trapdoor,
    };
    for wood in Wood::ALL {
        let d = wood.display();
        if wood != Wood::Oak {
            reg.register(
                BlockDef::solid(
                    &wood.log(),
                    &format!("{d} Log"),
                    T::top_bottom_side(
                        &wood.texture("log_top"),
                        &wood.texture("log_top"),
                        &wood.texture("log_side"),
                    ),
                )
                .mined(2.0, Some(ToolKind::Axe), 0),
            );
            reg.register(
                BlockDef::solid(
                    &wood.planks(),
                    &format!("{d} Planks"),
                    T::all(&wood.texture("planks")),
                )
                .mined(2.0, Some(ToolKind::Axe), 0),
            );
            reg.register(BlockDef {
                render: RenderKind::Cutout,
                drops: Drops::Nothing,
                bonus_drop: vec![(wood.sapling(), 12)],
                ..BlockDef::solid(
                    &wood.leaves(),
                    &format!("{d} Leaves"),
                    T::all(&wood.texture("leaves")),
                )
                .mined(0.3, Some(ToolKind::Hoe), 0)
            });
        }
        reg.register(BlockDef {
            render: RenderKind::Cutout,
            solid: false,
            shape: BlockShape::Boxes(vec![[0, 0, 8, 16, 14, 8], [8, 0, 0, 8, 14, 16]]),
            stage: Some(Stage {
                family: wood.sapling(),
                stage: 0,
                max: 0,
                kind: StageKind::Sapling {
                    wood: wood.key().into(),
                },
            }),
            ..BlockDef::solid(
                &wood.sapling(),
                &format!("{d} Sapling"),
                T::all(&format!("sapling_{}", wood.key())),
            )
            .mined(0.0, None, 0)
        });
        if wood == Wood::Oak {
            continue;
        }
        let planks = wood.texture("planks");
        register_stairs_and_slab(reg, wood.key(), d, &planks, ToolKind::Axe, 0);
        register_fence(reg, &wood.fence(), &format!("{d} Fence"), &planks);
        register_fence_gate(reg, &wood.fence_gate(), &format!("{d} Fence Gate"), &planks);
        register_door(
            reg,
            &wood.door(),
            &format!("{d} Door"),
            &planks,
            &wood.texture("door_lower"),
            &wood.texture("door_upper"),
        );
        register_trapdoor(
            reg,
            &wood.trapdoor(),
            &format!("{d} Trapdoor"),
            &wood.texture("trapdoor"),
        );
    }
}

/// One block of a tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Log,
    Leaves,
}

/// The blocks of a tree of `wood` rooted at `[0, 0, 0]` (the first block of
/// the trunk), picked with `r`. Logs replace what's there; leaves only fill
/// empty space (whoever builds it decides).
pub fn tree_shape(wood: Wood, r: u64) -> Vec<([i32; 3], Part)> {
    let mut out = Vec::new();
    let bit = |i: i32| (splitmix64(r ^ i as u64) & 1) == 1;
    let blob = |out: &mut Vec<([i32; 3], Part)>, top: i32, layers: &[(i32, i32)], trim: bool| {
        for &(dy, radius) in layers {
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    let corner = dx.abs() == radius && dz.abs() == radius;
                    if corner && (radius <= 1 || (trim && bit(dx * 31 + dz * 7 + dy * 3))) {
                        continue;
                    }
                    out.push(([dx, top + dy, dz], Part::Leaves));
                }
            }
        }
    };
    let trunk = |out: &mut Vec<([i32; 3], Part)>, height: i32| {
        for y in 0..height {
            out.push(([0, y, 0], Part::Log));
        }
    };
    let r2 = r >> 16;
    match wood {
        Wood::Oak => {
            // Just as before milestone 19: 4–6 tall, a round blob on top
            // with some corners trimmed.
            let height = 4 + (r2 % 3) as i32;
            let top = height;
            for dy in -3..=1i32 {
                let radius: i32 = if dy <= -2 { 2 } else { 1 };
                for dz in -radius..=radius {
                    for dx in -radius..=radius {
                        let corner = dx.abs() == radius && dz.abs() == radius;
                        let trim =
                            (r2 >> ((dx + 2 + (dz + 2) * 5 + (dy + 3) * 25) as u64 % 40)) & 1 == 1;
                        if corner && (radius == 1 || trim) {
                            continue;
                        }
                        out.push(([dx, top + dy, dz], Part::Leaves));
                    }
                }
            }
            trunk(&mut out, height);
        }
        Wood::Birch => {
            let height = 5 + (r2 % 3) as i32;
            blob(&mut out, height, &[(-2, 2), (-1, 2), (0, 1), (1, 1)], true);
            trunk(&mut out, height);
        }
        Wood::Pine => {
            // A cone: wider rings lower down, a point on top.
            let height = 6 + (r2 % 4) as i32;
            let mut dy = 0;
            let mut layers = vec![(1, 0), (0, 1)];
            for i in 0..height - 3 {
                dy -= 1;
                layers.push((dy, if i % 2 == 0 { 2 } else { 1 }));
            }
            blob(&mut out, height, &layers, false);
            trunk(&mut out, height);
        }
        Wood::Jungle => {
            let height = 8 + (r2 % 5) as i32;
            blob(&mut out, height, &[(-2, 3), (-1, 3), (0, 2), (1, 1)], true);
            trunk(&mut out, height);
        }
        Wood::Cherry => {
            // Short, with a wide, flat cloud of blossom.
            let height = 4 + (r2 % 2) as i32;
            blob(&mut out, height, &[(-1, 3), (0, 3), (1, 2)], true);
            trunk(&mut out, height);
        }
        Wood::Dark => {
            let height = 4 + (r2 % 2) as i32;
            blob(&mut out, height, &[(-2, 3), (-1, 3), (0, 2)], true);
            trunk(&mut out, height);
        }
    }
    out
}

/// The log and leaves blocks of a wood.
pub fn tree_blocks(reg: &BlockRegistry, wood: Wood) -> (BlockId, BlockId) {
    (
        reg.by_name(&wood.log())
            .unwrap_or(crate::block::blocks::LOG),
        reg.by_name(&wood.leaves())
            .unwrap_or(crate::block::blocks::LEAVES),
    )
}

/// The widest any tree's leaves reach from its trunk.
pub const TREE_RADIUS: i32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wood_has_its_blocks() {
        let r = BlockRegistry::with_builtin_blocks();
        for w in Wood::ALL {
            for name in [
                w.log(),
                w.planks(),
                w.leaves(),
                w.sapling(),
                w.stairs(),
                w.slab(),
                w.fence(),
                w.fence_gate(),
                w.door(),
                w.trapdoor(),
            ] {
                assert!(r.by_name(&name).is_some(), "no {name}");
            }
        }
        assert_eq!(wood_of("core:birch_planks"), Some(Wood::Birch));
        assert_eq!(wood_of("core:planks"), None);
    }

    #[test]
    fn trees_have_trunks_and_stay_narrow() {
        for w in Wood::ALL {
            let t = tree_shape(w, 12345);
            assert!(
                t.iter().any(|(p, k)| *p == [0, 0, 0] && *k == Part::Log),
                "{w:?}"
            );
            assert!(t.iter().any(|(_, k)| *k == Part::Leaves));
            assert!(
                t.iter()
                    .all(|(p, _)| p[0].abs() <= TREE_RADIUS && p[2].abs() <= TREE_RADIUS)
            );
        }
    }
}

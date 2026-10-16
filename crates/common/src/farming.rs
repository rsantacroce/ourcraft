//! Farming (milestone 17): farmland, crops that grow in stages, pumpkins
//! and melons on stems, sugar cane, tall grass and mushrooms, plus two
//! blocks that change step by step when used: cake (eaten a slice at a
//! time) and the composter (filled with plant scraps, it gives bone meal).
//!
//! A block id says everything about a block, so a wheat plant at stage 3
//! is its own block. Each such block remembers its family and stage in
//! [`Stage`]. The server makes plants grow (see the server's `farming`
//! module); this file says what grows into what, and where plants can
//! stand.

use serde::{Deserialize, Serialize};

use crate::block::{
    BlockDef, BlockId, BlockRegistry, BlockShape, BlockTextures as T, Drops, RenderKind, ShapeBox,
    ToolKind, blocks,
};

/// What kind of stepped block this is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StageKind {
    /// Grows on farmland; ripe at the last stage.
    Crop,
    /// Grows on farmland; once ripe, puts this fruit block next to itself.
    Stem { fruit: String },
    /// Sugar cane: grows taller (up to three) next to water.
    Cane,
    /// Tall grass: sits on grass or dirt, and you can build into it.
    Grass,
    /// A flower: sits on grass or dirt.
    Flower,
    /// Grows in the dark on any solid block.
    Mushroom,
    /// Each stage is one more slice eaten.
    Cake,
    /// Each stage is a bit fuller; full at the last.
    Composter,
}

/// A block's family (the first block of it) and stage in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stage {
    pub family: String,
    pub stage: u8,
    /// The last stage (ripe, full, or the last slice).
    pub max: u8,
    pub kind: StageKind,
}

/// Highest a sugar cane grows on its own.
pub const CANE_HEIGHT: i32 = 3;

/// A plant drawn as two crossed panels, `height` sixteenths tall.
fn cross(height: u8) -> BlockShape {
    let h = height.clamp(1, 16);
    BlockShape::Boxes(vec![[0, 0, 8, 16, h, 8], [8, 0, 0, 8, h, 16]])
}

fn plant_def(name: &str, display: &str, texture: &str, shape: BlockShape) -> BlockDef {
    BlockDef {
        render: RenderKind::Cutout,
        solid: false,
        shape,
        ..BlockDef::solid(name, display, T::all(texture)).mined(0.0, Some(ToolKind::Hoe), 0)
    }
}

fn harvest(items: &[(&str, u16, u16)]) -> Drops {
    Drops::Harvest(
        items
            .iter()
            .map(|(n, a, b)| ((*n).to_owned(), *a, *b))
            .collect(),
    )
}

/// Add the farming blocks (milestone 17) to the built-in list.
pub fn register_farming(reg: &mut BlockRegistry) {
    // ---- Farmland: dry, and wet (near water). Drops dirt. ----
    for (name, top) in [
        ("core:farmland", "farmland"),
        ("core:farmland_wet", "farmland_wet"),
    ] {
        reg.register(BlockDef {
            shape: BlockShape::Boxes(vec![[0, 0, 0, 16, 15, 16]]),
            drops: Drops::Block(blocks::DIRT),
            hidden: name.ends_with("_wet"),
            ..BlockDef::solid(name, "Farmland", T::top_bottom_side(top, "dirt", "dirt")).mined(
                0.6,
                Some(ToolKind::Shovel),
                0,
            )
        });
    }

    // ---- Crops: (family, display, texture, stages, young drops, ripe drops) ----
    type DropList = &'static [(&'static str, u16, u16)];
    let crops: [(&str, &str, &str, u8, DropList, DropList); 4] = [
        (
            "core:wheat_crop",
            "Wheat",
            "wheat",
            7,
            &[("core:wheat_seeds", 1, 1)],
            &[("core:wheat", 1, 1), ("core:wheat_seeds", 1, 3)],
        ),
        (
            "core:carrot_crop",
            "Carrots",
            "carrots",
            3,
            &[("core:carrot", 1, 1)],
            &[("core:carrot", 2, 4)],
        ),
        (
            "core:potato_crop",
            "Potatoes",
            "potatoes",
            3,
            &[("core:potato", 1, 1)],
            &[("core:potato", 2, 4)],
        ),
        (
            "core:beetroot_crop",
            "Beetroots",
            "beetroots",
            3,
            &[("core:beetroot_seeds", 1, 1)],
            &[("core:beetroot", 1, 1), ("core:beetroot_seeds", 1, 3)],
        ),
    ];
    for (family, display, tex, max, young, ripe) in crops {
        for stage in 0..=max {
            let name = if stage == 0 {
                family.to_owned()
            } else {
                format!("{family}_{stage}")
            };
            let height = 3 + (13 * stage as u32 / max as u32) as u8;
            reg.register(BlockDef {
                drops: harvest(if stage == max { ripe } else { young }),
                hidden: true,
                stage: Some(Stage {
                    family: family.to_owned(),
                    stage,
                    max,
                    kind: StageKind::Crop,
                }),
                ..plant_def(&name, display, &format!("{tex}_{stage}"), cross(height))
            });
        }
    }

    // ---- Pumpkins and melons, and the stems they grow on. ----
    reg.register(
        BlockDef::solid(
            "core:pumpkin",
            "Pumpkin",
            T::top_bottom_side("pumpkin_top", "pumpkin_top", "pumpkin_side"),
        )
        .mined(1.0, Some(ToolKind::Axe), 0),
    );
    reg.register(BlockDef {
        drops: harvest(&[("core:melon_slice", 3, 7)]),
        ..BlockDef::solid(
            "core:melon",
            "Melon",
            T::top_bottom_side("melon_top", "melon_top", "melon_side"),
        )
        .mined(1.0, Some(ToolKind::Axe), 0)
    });
    reg.register(BlockDef {
        light: 15,
        ..BlockDef::solid(
            "core:pumpkin_lantern",
            "Pumpkin Lantern",
            T::top_bottom_side("pumpkin_top", "pumpkin_top", "pumpkin_face"),
        )
        .mined(1.0, Some(ToolKind::Axe), 0)
    });
    for (family, seeds, fruit) in [
        ("core:pumpkin_stem", "core:pumpkin_seeds", "core:pumpkin"),
        ("core:melon_stem", "core:melon_seeds", "core:melon"),
    ] {
        let max = 7;
        for stage in 0..=max {
            let name = if stage == 0 {
                family.to_owned()
            } else {
                format!("{family}_{stage}")
            };
            let tex = if stage == max {
                "stem_ripe".to_owned()
            } else {
                format!("stem_{stage}")
            };
            reg.register(BlockDef {
                drops: harvest(&[(seeds, 1, 1)]),
                hidden: true,
                stage: Some(Stage {
                    family: family.to_owned(),
                    stage,
                    max,
                    kind: StageKind::Stem {
                        fruit: fruit.to_owned(),
                    },
                }),
                ..plant_def(&name, "Stem", &tex, cross(2 + stage * 2))
            });
        }
    }

    // ---- Wild plants. ----
    let one = |family: &str, kind: StageKind| {
        Some(Stage {
            family: family.to_owned(),
            stage: 0,
            max: 0,
            kind,
        })
    };
    reg.register(BlockDef {
        stage: one("core:sugar_cane", StageKind::Cane),
        ..plant_def("core:sugar_cane", "Sugar Cane", "sugar_cane", cross(16))
    });
    reg.register(BlockDef {
        drops: Drops::Nothing,
        bonus_drop: Some(("core:wheat_seeds".into(), 6)),
        stage: one("core:tall_grass", StageKind::Grass),
        ..plant_def("core:tall_grass", "Tall Grass", "tall_grass", cross(13))
    });
    for (name, display) in [
        ("core:red_mushroom", "Red Mushroom"),
        ("core:brown_mushroom", "Brown Mushroom"),
    ] {
        reg.register(BlockDef {
            light: if name == "core:red_mushroom" { 0 } else { 1 },
            stage: one(name, StageKind::Mushroom),
            ..plant_def(name, display, name.trim_start_matches("core:"), cross(8))
        });
    }

    // ---- Cake: seven slices, eaten one at a time. ----
    let slices = 6;
    for bite in 0..=slices {
        let name = if bite == 0 {
            "core:cake".to_owned()
        } else {
            format!("core:cake_{bite}")
        };
        let x0 = 1 + bite * 2;
        let b: ShapeBox = [x0, 0, 1, 15, 8, 15];
        reg.register(BlockDef {
            render: RenderKind::Cutout,
            drops: Drops::Nothing,
            hidden: bite > 0,
            shape: BlockShape::Boxes(vec![b]),
            stage: Some(Stage {
                family: "core:cake".into(),
                stage: bite,
                max: slices,
                kind: StageKind::Cake,
            }),
            ..BlockDef::solid(
                &name,
                "Cake",
                T::top_bottom_side("cake_top", "cake_bottom", "cake_side"),
            )
            .mined(0.5, None, 0)
        });
    }

    // ---- Composter: wooden walls, filling up with compost. ----
    let full = 7;
    let mut composter = None;
    for level in 0..=full {
        let name = if level == 0 {
            "core:composter".to_owned()
        } else {
            format!("core:composter_{level}")
        };
        let mut boxes: Vec<ShapeBox> = vec![
            [0, 0, 0, 16, 2, 16],
            [0, 2, 0, 2, 16, 16],
            [14, 2, 0, 16, 16, 16],
            [2, 2, 0, 14, 16, 2],
            [2, 2, 14, 14, 16, 16],
        ];
        if level > 0 {
            boxes.push([2, 2, 2, 14, 2 + level * 2 - 1, 14]);
        }
        let id = reg.register(BlockDef {
            render: RenderKind::Cutout,
            drops: composter.map_or(Drops::Itself, Drops::Block),
            hidden: level > 0,
            shape: BlockShape::Boxes(boxes),
            stage: Some(Stage {
                family: "core:composter".into(),
                stage: level,
                max: full,
                kind: StageKind::Composter,
            }),
            ..BlockDef::solid(
                &name,
                "Composter",
                T::top_bottom_side("compost", "planks", "composter_side"),
            )
            .mined(0.6, Some(ToolKind::Axe), 0)
        });
        composter.get_or_insert(id);
    }
}

impl BlockRegistry {
    pub fn stage(&self, b: BlockId) -> Option<&Stage> {
        self.get(b).stage.as_ref()
    }

    /// The block of `family` at `stage`.
    pub fn at_stage(&self, family: &str, stage: u8) -> Option<BlockId> {
        self.iter()
            .find(|(_, d)| {
                d.stage
                    .as_ref()
                    .is_some_and(|s| s.family == family && s.stage == stage)
            })
            .map(|(id, _)| id)
    }

    /// The same block a few stages further on (capped at the last), or
    /// `None` if it's already there.
    pub fn advanced(&self, b: BlockId, steps: u8) -> Option<BlockId> {
        let s = self.stage(b)?;
        if s.stage >= s.max {
            return None;
        }
        self.at_stage(&s.family, s.stage.saturating_add(steps).min(s.max))
    }

    /// Does this grow over time (crops, stems, sugar cane)?
    pub fn grows(&self, b: BlockId) -> bool {
        self.stage(b).is_some_and(|s| {
            matches!(
                s.kind,
                StageKind::Crop | StageKind::Stem { .. } | StageKind::Cane
            )
        })
    }

    /// Ripe (a crop or stem at its last stage)?
    pub fn is_ripe(&self, b: BlockId) -> bool {
        self.stage(b).is_some_and(|s| {
            matches!(s.kind, StageKind::Crop | StageKind::Stem { .. }) && s.stage == s.max
        })
    }

    pub fn is_farmland(&self, b: BlockId) -> bool {
        matches!(
            self.get(b).name.as_str(),
            "core:farmland" | "core:farmland_wet"
        )
    }

    /// Plants and cakes stand on the block below and go when it does.
    pub fn sits_on_ground(&self, b: BlockId) -> bool {
        self.stage(b)
            .is_some_and(|s| !matches!(s.kind, StageKind::Composter))
    }

    /// Something you can place a block straight into (tall grass), like
    /// air.
    pub fn is_soft(&self, b: BlockId) -> bool {
        self.stage(b)
            .is_some_and(|s| matches!(s.kind, StageKind::Grass))
    }

    /// Can `plant` be here? `get` gives the block at an offset from the
    /// spot.
    pub fn fits(&self, plant: BlockId, get: impl Fn([i32; 3]) -> BlockId) -> bool {
        let Some(s) = self.stage(plant) else {
            return true;
        };
        let below = get([0, -1, 0]);
        let named = |b: BlockId, names: &[&str]| names.contains(&self.get(b).name.as_str());
        match s.kind {
            StageKind::Crop | StageKind::Stem { .. } => self.is_farmland(below),
            StageKind::Grass | StageKind::Flower => {
                named(below, &["core:grass", "core:dirt", "core:snow"]) || self.is_farmland(below)
            }
            StageKind::Cane => {
                if self.stage(below).is_some_and(|b| b.kind == StageKind::Cane) {
                    return true;
                }
                let ground = named(
                    below,
                    &["core:grass", "core:dirt", "core:sand", "core:snow"],
                );
                let wet = [[1, -1, 0], [-1, -1, 0], [0, -1, 1], [0, -1, -1]]
                    .iter()
                    .any(|o| {
                        self.fluid(get(*o))
                            .is_some_and(|f| f.family == "core:water")
                    });
                ground && wet
            }
            StageKind::Mushroom => self.is_opaque(below),
            StageKind::Cake => self.is_solid(below),
            StageKind::Composter => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> BlockRegistry {
        BlockRegistry::with_builtin_blocks()
    }

    #[test]
    fn wheat_grows_up_to_ripe() {
        let r = reg();
        let w = r.by_name("core:wheat_crop").unwrap();
        assert!(r.grows(w) && !r.is_ripe(w));
        let mut b = w;
        for _ in 0..7 {
            b = r.advanced(b, 1).unwrap();
        }
        assert!(r.is_ripe(b));
        assert_eq!(r.advanced(b, 1), None);
        // Bone meal can skip ahead, but not past ripe.
        assert_eq!(r.advanced(w, 20), Some(b));
        assert!(matches!(r.get(b).drops, Drops::Harvest(ref d) if d.len() == 2));
    }

    #[test]
    fn crops_need_farmland_and_cane_needs_water() {
        let r = reg();
        let wheat = r.by_name("core:wheat_crop").unwrap();
        let farmland = r.by_name("core:farmland").unwrap();
        assert!(r.fits(wheat, |o| if o == [0, -1, 0] {
            farmland
        } else {
            BlockId::AIR
        }));
        assert!(!r.fits(wheat, |o| if o == [0, -1, 0] {
            blocks::DIRT
        } else {
            BlockId::AIR
        }));
        let cane = r.by_name("core:sugar_cane").unwrap();
        let dry = |o: [i32; 3]| {
            if o == [0, -1, 0] {
                blocks::SAND
            } else {
                BlockId::AIR
            }
        };
        assert!(!r.fits(cane, dry));
        let wet = |o: [i32; 3]| match o {
            [0, -1, 0] => blocks::SAND,
            [1, -1, 0] => blocks::WATER,
            _ => BlockId::AIR,
        };
        assert!(r.fits(cane, wet));
        let on_cane = |o: [i32; 3]| if o == [0, -1, 0] { cane } else { BlockId::AIR };
        assert!(r.fits(cane, on_cane));
    }

    #[test]
    fn cakes_get_eaten_and_composters_fill() {
        let r = reg();
        let cake = r.by_name("core:cake").unwrap();
        let last = r.at_stage("core:cake", 6).unwrap();
        assert_eq!(r.advanced(cake, 6), Some(last));
        let c = r.by_name("core:composter").unwrap();
        let full = r.at_stage("core:composter", 7).unwrap();
        assert_eq!(r.get(full).drops, Drops::Block(c));
        assert!(!r.sits_on_ground(c) && r.sits_on_ground(cake));
    }
}

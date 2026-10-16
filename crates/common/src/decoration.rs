//! Decoration and storage (milestone 21): flower pots, paintings, item
//! frames, banners, lanterns, candles, chains, glass panes and iron bars,
//! bookshelves and books, a lectern, barrels and scaffolding.

use crate::block::{
    BlockDef, BlockRegistry, BlockShape, BlockTextures as T, Drops, RenderKind, ShapeBox, ToolKind,
};
use crate::colours::{COLOURS, FLOWERS};
use crate::shapes::{VariantKind, all_dirs, register_family};

/// The pictures to hang: (key, display name, the wool colour in the
/// middle of its recipe).
pub const PAINTINGS: [(&str, &str, &str); 8] = [
    ("sunset", "Sunset", "orange"),
    ("mountain", "Mountain", "white"),
    ("sea", "Sea", "blue"),
    ("flower", "Flower", "pink"),
    ("cat", "Cat", "gray"),
    ("house", "House", "red"),
    ("rainbow", "Rainbow", "lime"),
    ("night", "Starry Night", "black"),
];

/// What can go in a flower pot: the flowers, the saplings and the
/// mushrooms. (Potted: `core:potted_<name without core:>`.)
pub fn pottable() -> Vec<String> {
    let mut list: Vec<String> = FLOWERS.iter().map(|f| f.0.to_owned()).collect();
    list.extend(crate::woods::Wood::ALL.iter().map(|w| w.sapling()));
    list.extend([
        "core:red_mushroom".to_owned(),
        "core:brown_mushroom".to_owned(),
    ]);
    list
}

/// The pot with this plant in it.
pub fn potted(plant: &str) -> String {
    format!("core:potted_{}", plant.trim_start_matches("core:"))
}

/// Add the decoration blocks (milestone 21).
pub fn register_decoration(reg: &mut BlockRegistry) {
    let cutout = |name: &str, display: &str, tex: T| BlockDef {
        render: RenderKind::Cutout,
        ..BlockDef::solid(name, display, tex)
    };
    // ---- Flower pots, empty and with each plant. ----
    let pot_box: ShapeBox = [5, 0, 5, 11, 6, 11];
    let pot = reg.register(BlockDef {
        shape: BlockShape::Boxes(vec![pot_box]),
        ..cutout(
            "core:flower_pot",
            "Flower Pot",
            T::top_bottom_side("pot_top", "pot_side", "pot_side"),
        )
        .mined(0.0, None, 0)
    });
    for plant in pottable() {
        let short = plant.trim_start_matches("core:");
        reg.register(BlockDef {
            shape: BlockShape::Boxes(vec![pot_box, [3, 6, 8, 13, 16, 8], [8, 6, 3, 8, 16, 13]]),
            drops: Drops::Block(pot),
            bonus_drop: vec![(plant.clone(), 1)],
            hidden: true,
            ..cutout(
                &potted(&plant),
                "Flower Pot",
                T::top_bottom_side("pot_top", "pot_side", &format!("potted_{short}")),
            )
            .mined(0.0, None, 0)
        });
    }

    // ---- Paintings and item frames, hung on walls. ----
    for (key, display, _) in PAINTINGS {
        let tex = format!("painting_{key}");
        let def = |name: &str| cutout(name, "", T::all(&tex)).mined(0.0, None, 0);
        register_family(
            reg,
            VariantKind::WallHanging,
            &format!("core:painting_{key}"),
            &format!("Painting: {display}"),
            &def,
            all_dirs(&[[0, 0, 15, 16, 16, 16]], false, false),
            false,
        );
    }
    let frame = |name: &str| cutout(name, "", T::all("item_frame")).mined(0.0, None, 0);
    register_family(
        reg,
        VariantKind::WallHanging,
        "core:item_frame",
        "Item Frame",
        &frame,
        all_dirs(&[[2, 2, 15, 14, 14, 16]], false, false),
        false,
    );

    // ---- Banners in every colour: standing, or on a wall. ----
    for (c, display, _) in COLOURS {
        let tex = format!("banner_{c}");
        let def = |name: &str| BlockDef {
            solid: false,
            ..cutout(name, "", T::top_bottom_side("planks", "planks", &tex)).mined(
                0.5,
                Some(ToolKind::Axe),
                0,
            )
        };
        let mut states = all_dirs(&[[7, 0, 7, 9, 16, 9], [1, 3, 8, 15, 16, 9]], false, false);
        states.extend(all_dirs(&[[1, 0, 15, 15, 14, 16]], false, true));
        register_family(
            reg,
            VariantKind::Banner,
            &format!("core:{c}_banner"),
            &format!("{display} Banner"),
            &def,
            states,
            false,
        );
        // The wall ones are the "upper" half of the family.
        let family = format!("core:{c}_banner");
        for id in 0..reg.len() as u16 {
            let d = reg.defs_mut(crate::block::BlockId(id));
            if let Some(v) = d.variant.as_mut()
                && v.family == family
                && v.upper
            {
                v.kind = VariantKind::WallBanner;
            }
        }
    }

    // ---- Lights and chains. ----
    reg.register(BlockDef {
        light: 15,
        shape: BlockShape::Boxes(vec![[5, 0, 5, 11, 8, 11], [6, 8, 6, 10, 10, 10]]),
        ..cutout(
            "core:lantern",
            "Lantern",
            T::top_bottom_side("lantern_top", "lantern_top", "lantern"),
        )
        .mined(1.0, Some(ToolKind::Pickaxe), 0)
    });
    reg.register(BlockDef {
        light: 10,
        solid: false,
        shape: BlockShape::Boxes(vec![[7, 0, 7, 9, 7, 9]]),
        ..cutout(
            "core:candle",
            "Candle",
            T::top_bottom_side("candle_top", "candle", "candle"),
        )
        .mined(0.0, None, 0)
    });
    reg.register(BlockDef {
        shape: BlockShape::Boxes(vec![[7, 0, 7, 9, 16, 9]]),
        ..cutout("core:chain", "Chain", T::all("chain")).mined(1.0, Some(ToolKind::Pickaxe), 0)
    });

    // ---- Panes and bars. ----
    let pane = |name: &str, display: &str, tex: &str, render: RenderKind| BlockDef {
        render,
        shape: BlockShape::Pane,
        ..BlockDef::solid(name, display, T::all(tex)).mined(0.3, None, 0)
    };
    reg.register(pane(
        "core:glass_pane",
        "Glass Pane",
        "glass",
        RenderKind::Cutout,
    ));
    for (c, display, _) in COLOURS {
        reg.register(pane(
            &format!("core:{c}_stained_glass_pane"),
            &format!("{display} Stained Glass Pane"),
            &format!("stained_glass_{c}"),
            RenderKind::Translucent,
        ));
    }
    reg.register(BlockDef {
        ..pane(
            "core:iron_bars",
            "Iron Bars",
            "iron_bars",
            RenderKind::Cutout,
        )
        .mined(2.0, Some(ToolKind::Pickaxe), 0)
    });

    // ---- Books, a lectern, barrels and scaffolding. ----
    reg.register(
        BlockDef::solid(
            "core:bookshelf",
            "Bookshelf",
            T::top_bottom_side("planks", "planks", "bookshelf"),
        )
        .mined(1.5, Some(ToolKind::Axe), 0),
    );
    reg.register(BlockDef {
        shape: BlockShape::Boxes(vec![
            [0, 0, 0, 16, 2, 16],
            [5, 2, 5, 11, 12, 11],
            [1, 12, 1, 15, 16, 15],
        ]),
        ..cutout(
            "core:lectern",
            "Lectern",
            T::top_bottom_side("lectern_top", "planks", "planks"),
        )
        .mined(1.5, Some(ToolKind::Axe), 0)
    });
    reg.register(
        BlockDef::solid(
            "core:barrel",
            "Barrel",
            T::top_bottom_side("barrel_top", "barrel_top", "barrel_side"),
        )
        .mined(2.0, Some(ToolKind::Axe), 0),
    );
    reg.register(BlockDef {
        solid: false,
        ..cutout("core:scaffolding", "Scaffolding", T::all("scaffolding")).mined(0.0, None, 0)
    });
    // ---- Bee hives (milestone 23): empty, and full of honey. ----
    for (stage, tex) in [(0, "beehive_front"), (1, "beehive_front_full")] {
        let name = if stage == 0 {
            "core:beehive".to_owned()
        } else {
            "core:beehive_full".to_owned()
        };
        reg.register(BlockDef {
            hidden: stage > 0,
            drops: Drops::Item("core:beehive".into(), 1),
            stage: Some(crate::farming::Stage {
                family: "core:beehive".into(),
                stage,
                max: 1,
                kind: crate::farming::StageKind::Hive,
            }),
            ..BlockDef::solid(
                &name,
                "Bee Hive",
                T::top_bottom_side("beehive_top", "beehive_top", tex),
            )
            .mined(0.6, Some(ToolKind::Axe), 0)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pots_paintings_banners_and_panes() {
        let r = BlockRegistry::with_builtin_blocks();
        for plant in pottable() {
            assert!(r.by_name(&potted(&plant)).is_some(), "{plant}");
        }
        let painting = r.by_name("core:painting_cat").unwrap();
        let placed = r
            .placement(painting, [1, 0, 0], [0.0, 0.0, 1.0])
            .unwrap()
            .block;
        assert_eq!(r.variant_kind(placed), Some(VariantKind::WallHanging));
        assert!(
            r.placement(painting, [0, 1, 0], [0.0, 0.0, 1.0]).is_none(),
            "walls only"
        );
        let banner = r.by_name("core:blue_banner").unwrap();
        let wall = r
            .placement(banner, [0, 0, -1], [0.0, 0.0, 1.0])
            .unwrap()
            .block;
        assert_eq!(r.variant_kind(wall), Some(VariantKind::WallBanner));
        let pane = r.by_name("core:glass_pane").unwrap();
        let alone = r.boxes_at(pane, |_| crate::block::BlockId::AIR).len();
        let joined = r.boxes_at(pane, |o| {
            if o == [1, 0, 0] {
                pane
            } else {
                crate::block::BlockId::AIR
            }
        });
        assert!(joined.len() > alone);
        assert!(r.is_climbable(r.by_name("core:scaffolding").unwrap()));
    }
}

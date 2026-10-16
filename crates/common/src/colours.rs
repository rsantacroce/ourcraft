//! Colours (milestone 18): flowers, sixteen dyes, and things dyed in
//! every colour: wool, beds, carpets, stained glass and clay tiles.
//!
//! Every coloured block is named after its colour, like
//! `core:light_blue_wool`. White wool is the original `core:wool`, and the
//! red bed is the original `core:bed`, so old worlds keep theirs.

use crate::block::{BlockDef, BlockRegistry, BlockShape, BlockTextures as T, RenderKind, ToolKind};
use crate::farming::{Stage, StageKind};

/// The sixteen colours: (name, display name, RGB).
pub const COLOURS: [(&str, &str, [u8; 3]); 16] = [
    ("white", "White", [240, 240, 234]),
    ("light_gray", "Light Gray", [168, 168, 160]),
    ("gray", "Gray", [92, 96, 100]),
    ("black", "Black", [30, 30, 36]),
    ("brown", "Brown", [120, 80, 50]),
    ("red", "Red", [180, 44, 44]),
    ("orange", "Orange", [236, 128, 30]),
    ("yellow", "Yellow", [246, 206, 50]),
    ("lime", "Lime", [120, 200, 40]),
    ("green", "Green", [80, 110, 30]),
    ("cyan", "Cyan", [30, 140, 150]),
    ("light_blue", "Light Blue", [90, 170, 220]),
    ("blue", "Blue", [50, 60, 170]),
    ("purple", "Purple", [120, 50, 170]),
    ("magenta", "Magenta", [190, 70, 180]),
    ("pink", "Pink", [236, 140, 170]),
];

/// The flowers: (name, display name, the dye it makes).
pub const FLOWERS: [(&str, &str, &str); 8] = [
    ("core:dandelion", "Dandelion", "yellow"),
    ("core:poppy", "Poppy", "red"),
    ("core:cornflower", "Cornflower", "blue"),
    ("core:daisy", "Daisy", "light_gray"),
    ("core:pink_tulip", "Pink Tulip", "pink"),
    ("core:orange_tulip", "Orange Tulip", "orange"),
    ("core:allium", "Allium", "magenta"),
    ("core:forget_me_not", "Forget-me-not", "light_blue"),
];

/// The colour's number (0 = white), by name.
pub fn colour_index(name: &str) -> Option<u8> {
    COLOURS.iter().position(|c| c.0 == name).map(|i| i as u8)
}

/// The wool block of a colour (white is plain `core:wool`).
pub fn wool_name(colour: &str) -> String {
    if colour == "white" {
        "core:wool".into()
    } else {
        format!("core:{colour}_wool")
    }
}

/// The bed of a colour (red is plain `core:bed`).
pub fn bed_name(colour: &str) -> String {
    if colour == "red" {
        "core:bed".into()
    } else {
        format!("core:{colour}_bed")
    }
}

/// Which colour a dye item is (`"core:blue_dye"` → 12).
pub fn dye_colour(item: &str) -> Option<u8> {
    colour_index(item.strip_prefix("core:")?.strip_suffix("_dye")?)
}

/// Add the coloured blocks (milestone 18) to the built-in list.
pub fn register_colours(reg: &mut BlockRegistry) {
    for (name, display, _) in FLOWERS {
        reg.register(BlockDef {
            render: RenderKind::Cutout,
            solid: false,
            shape: BlockShape::Boxes(vec![[0, 0, 8, 16, 12, 8], [8, 0, 0, 8, 12, 16]]),
            stage: Some(Stage {
                family: name.into(),
                stage: 0,
                max: 0,
                kind: StageKind::Flower,
            }),
            ..BlockDef::solid(name, display, T::all(name.trim_start_matches("core:")))
                .mined(0.0, None, 0)
        });
    }
    for (c, display, _) in COLOURS {
        if c != "white" {
            reg.register(
                BlockDef::solid(
                    &wool_name(c),
                    &format!("{display} Wool"),
                    T::all(&format!("wool_{c}")),
                )
                .mined(0.8, None, 0),
            );
        }
    }
    for (c, display, _) in COLOURS {
        reg.register(BlockDef {
            render: RenderKind::Cutout,
            shape: BlockShape::Boxes(vec![[0, 0, 0, 16, 1, 16]]),
            ..BlockDef::solid(
                &format!("core:{c}_carpet"),
                &format!("{display} Carpet"),
                T::all(&if c == "white" {
                    "wool".to_owned()
                } else {
                    format!("wool_{c}")
                }),
            )
            .mined(0.1, None, 0)
        });
    }
    for (c, display, _) in COLOURS {
        reg.register(BlockDef {
            render: RenderKind::Translucent,
            ..BlockDef::solid(
                &format!("core:{c}_stained_glass"),
                &format!("{display} Stained Glass"),
                T::all(&format!("stained_glass_{c}")),
            )
            .mined(0.3, None, 0)
        });
    }
    for (c, display, _) in COLOURS {
        reg.register(
            BlockDef::solid(
                &format!("core:{c}_clay_tiles"),
                &format!("{display} Clay Tiles"),
                T::all(&format!("clay_tiles_{c}")),
            )
            .mined(1.2, Some(ToolKind::Pickaxe), 1),
        );
    }
    for (c, display, _) in COLOURS {
        if c != "red" {
            crate::shapes::register_bed(
                reg,
                &bed_name(c),
                &format!("{display} Bed"),
                &format!("_{c}"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_colour_has_wool_beds_carpet_glass_and_tiles() {
        let r = BlockRegistry::with_builtin_blocks();
        for (c, _, _) in COLOURS {
            for name in [
                wool_name(c),
                bed_name(c),
                format!("core:{c}_carpet"),
                format!("core:{c}_stained_glass"),
                format!("core:{c}_clay_tiles"),
            ] {
                assert!(r.by_name(&name).is_some(), "no {name}");
            }
        }
        assert_eq!(dye_colour("core:light_blue_dye"), Some(11));
        assert_eq!(dye_colour("core:wool"), None);
    }
}

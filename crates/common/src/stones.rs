//! Stone kinds (milestone 19): smooth stone, stone bricks (mossy and
//! cracked), mossy cobblestone, sandstone (smooth and cut), granite,
//! diorite and andesite (and polished), deep slate further down, obsidian,
//! ice, snow, and walls, stairs and slabs for the new kinds.

use crate::block::{
    BlockDef, BlockRegistry, BlockShape, BlockTextures as T, Drops, RenderKind, ToolKind,
};
use crate::farming::{Stage, StageKind};

/// Plain stone kinds: (name, display name, texture). Each drops itself and
/// needs a pickaxe.
pub const STONES: [(&str, &str, &str); 17] = [
    ("core:smooth_stone", "Smooth Stone", "smooth_stone"),
    ("core:stone_bricks", "Stone Bricks", "stone_bricks"),
    (
        "core:mossy_stone_bricks",
        "Mossy Stone Bricks",
        "mossy_stone_bricks",
    ),
    (
        "core:cracked_stone_bricks",
        "Cracked Stone Bricks",
        "cracked_stone_bricks",
    ),
    (
        "core:mossy_cobblestone",
        "Mossy Cobblestone",
        "mossy_cobblestone",
    ),
    ("core:sandstone", "Sandstone", "sandstone"),
    (
        "core:smooth_sandstone",
        "Smooth Sandstone",
        "smooth_sandstone",
    ),
    ("core:cut_sandstone", "Cut Sandstone", "cut_sandstone"),
    ("core:granite", "Granite", "granite"),
    (
        "core:polished_granite",
        "Polished Granite",
        "polished_granite",
    ),
    ("core:diorite", "Diorite", "diorite"),
    (
        "core:polished_diorite",
        "Polished Diorite",
        "polished_diorite",
    ),
    ("core:andesite", "Andesite", "andesite"),
    (
        "core:polished_andesite",
        "Polished Andesite",
        "polished_andesite",
    ),
    (
        "core:cobbled_deep_slate",
        "Cobbled Deep Slate",
        "cobbled_deep_slate",
    ),
    (
        "core:polished_deep_slate",
        "Polished Deep Slate",
        "polished_deep_slate",
    ),
    (
        "core:deep_slate_bricks",
        "Deep Slate Bricks",
        "deep_slate_bricks",
    ),
];

/// Kinds with walls: (block, wall key).
pub const WALLS: [(&str, &str); 8] = [
    ("core:cobblestone", "cobblestone"),
    ("core:mossy_cobblestone", "mossy_cobblestone"),
    ("core:stone_bricks", "stone_brick"),
    ("core:sandstone", "sandstone"),
    ("core:granite", "granite"),
    ("core:diorite", "diorite"),
    ("core:andesite", "andesite"),
    ("core:cobbled_deep_slate", "deep_slate"),
];

/// Kinds with stairs and slabs: (key, display, texture). Named
/// `core:{key}_stairs` and `core:{key}_slab`.
pub const STAIRS: [(&str, &str, &str); 8] = [
    ("stone_bricks", "Stone Brick", "stone_bricks"),
    (
        "mossy_cobblestone",
        "Mossy Cobblestone",
        "mossy_cobblestone",
    ),
    ("sandstone", "Sandstone", "sandstone"),
    ("smooth_stone", "Smooth Stone", "smooth_stone"),
    ("polished_granite", "Polished Granite", "polished_granite"),
    ("polished_diorite", "Polished Diorite", "polished_diorite"),
    (
        "polished_andesite",
        "Polished Andesite",
        "polished_andesite",
    ),
    ("cobbled_deep_slate", "Deep Slate", "cobbled_deep_slate"),
];

/// Add the stone kinds (milestone 19) to the built-in list.
pub fn register_stones(reg: &mut BlockRegistry) {
    let pick =
        |d: BlockDef, hardness: f32, tier: u8| d.mined(hardness, Some(ToolKind::Pickaxe), tier);
    for (name, display, tex) in STONES {
        let soft = name.contains("sandstone");
        reg.register(pick(
            BlockDef::solid(name, display, T::all(tex)),
            if soft { 0.8 } else { 1.5 },
            1,
        ));
    }
    let cobbled = reg.by_name("core:cobbled_deep_slate").expect("just added");
    reg.register(BlockDef {
        drops: Drops::Block(cobbled),
        ..pick(
            BlockDef::solid("core:deep_slate", "Deep Slate", T::all("deep_slate")),
            2.5,
            1,
        )
    });
    reg.register(pick(
        BlockDef::solid("core:obsidian", "Obsidian", T::all("obsidian")),
        12.0,
        4,
    ));
    // ---- Ice and snow ----
    reg.register(BlockDef {
        render: RenderKind::Translucent,
        drops: Drops::Nothing,
        ..BlockDef::solid("core:ice", "Ice", T::all("ice")).mined(0.5, Some(ToolKind::Pickaxe), 0)
    });
    reg.register(
        BlockDef::solid("core:packed_ice", "Packed Ice", T::all("packed_ice")).mined(
            0.6,
            Some(ToolKind::Pickaxe),
            0,
        ),
    );
    reg.register(BlockDef {
        drops: Drops::Item("core:snowball".into(), 4),
        ..BlockDef::solid("core:snow_block", "Snow Block", T::all("snow")).mined(
            0.3,
            Some(ToolKind::Shovel),
            0,
        )
    });
    reg.register(BlockDef {
        render: RenderKind::Cutout,
        solid: false,
        shape: BlockShape::Boxes(vec![[0, 0, 0, 16, 2, 16]]),
        drops: Drops::Item("core:snowball".into(), 1),
        stage: Some(Stage {
            family: "core:snow_layer".into(),
            stage: 0,
            max: 0,
            kind: StageKind::Layer,
        }),
        ..BlockDef::solid("core:snow_layer", "Snow", T::all("snow")).mined(
            0.1,
            Some(ToolKind::Shovel),
            0,
        )
    });
    // ---- Walls, stairs and slabs ----
    for (block, key) in WALLS {
        let tex = reg
            .by_name(block)
            .and_then(|b| reg.get(b).textures.clone())
            .map_or_else(|| "stone".to_owned(), |t| t.side);
        let display = reg
            .get(reg.by_name(block).expect("registered"))
            .display_name
            .clone();
        reg.register(BlockDef {
            render: RenderKind::Cutout,
            shape: BlockShape::Wall,
            ..pick(
                BlockDef::solid(
                    &format!("core:{key}_wall"),
                    &format!("{display} Wall"),
                    T::all(&tex),
                ),
                1.5,
                1,
            )
        });
    }
    for (key, display, tex) in STAIRS {
        crate::shapes::register_stairs_and_slab(reg, key, display, tex, ToolKind::Pickaxe, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stones_walls_and_stairs_exist() {
        let r = BlockRegistry::with_builtin_blocks();
        for (name, _, _) in STONES {
            assert!(r.by_name(name).is_some(), "{name}");
        }
        for (_, key) in WALLS {
            let w = r.by_name(&format!("core:{key}_wall")).unwrap();
            assert_eq!(*r.shape(w), BlockShape::Wall);
        }
        for (key, _, _) in STAIRS {
            assert!(r.by_name(&format!("core:{key}_stairs")).is_some());
        }
        let deep = r.by_name("core:deep_slate").unwrap();
        assert_eq!(
            r.get(deep).drops,
            Drops::Block(r.by_name("core:cobbled_deep_slate").unwrap())
        );
    }
}

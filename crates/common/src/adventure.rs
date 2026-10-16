//! Tools and adventure (milestone 20): fishing, a bow with arrows for
//! targets, armour, a compass, a clock, a map, a spyglass, rope, a saddle
//! and a campfire.

use crate::block::{BlockDef, BlockRegistry, BlockShape, BlockTextures as T, RenderKind, ToolKind};
use crate::item::{ItemDef, ItemKind};

/// The four armour slots, top to bottom.
pub const ARMOUR_PIECES: [(&str, &str); 4] = [
    ("helmet", "Helmet"),
    ("chestplate", "Chestplate"),
    ("leggings", "Leggings"),
    ("boots", "Boots"),
];

/// Armour materials: (key, display, crafting material, protection of each
/// piece, uses before it wears out).
pub const ARMOUR_MATERIALS: [(&str, &str, &str, [u8; 4], u16); 4] = [
    ("wool", "Woolly", "#wool", [1, 3, 2, 1], 60),
    ("iron", "Iron", "core:iron_ingot", [2, 6, 5, 2], 200),
    ("gold", "Gold", "core:gold_ingot", [2, 5, 3, 1], 100),
    ("diamond", "Diamond", "core:diamond", [3, 8, 6, 3], 450),
];

/// Each point of armour takes this much off a bonk.
pub const PROTECTION_PER_POINT: f32 = 0.04;

/// The items of milestone 20.
pub fn items() -> Vec<ItemDef> {
    let one = |name: &str, display: &str, durability: u16| ItemDef {
        max_stack: 1,
        durability,
        ..ItemDef::material(name, display)
    };
    let mut items = vec![
        one("core:fishing_rod", "Fishing Rod", 64),
        ItemDef {
            food: 2,
            smelts_into: Some("core:cooked_fish".into()),
            ..ItemDef::material("core:fish", "Fish")
        },
        ItemDef {
            food: 5,
            ..ItemDef::material("core:cooked_fish", "Cooked Fish")
        },
        one("core:bow", "Bow", 300),
        ItemDef::material("core:arrow", "Arrow"),
        ItemDef::material("core:flint", "Flint"),
        ItemDef::material("core:feather", "Feather"),
        ItemDef::material("core:string", "String"),
        one("core:compass", "Compass", 0),
        one("core:clock", "Clock", 0),
        one("core:map", "Map", 0),
        one("core:spyglass", "Spyglass", 0),
        ItemDef::material("core:rope", "Rope"),
        one("core:saddle", "Saddle", 0),
    ];
    for (key, display, _, points, durability) in ARMOUR_MATERIALS {
        for (slot, (piece, piece_display)) in ARMOUR_PIECES.iter().enumerate() {
            items.push(ItemDef {
                kind: ItemKind::Armour {
                    slot: slot as u8,
                    points: points[slot],
                },
                max_stack: 1,
                durability,
                ..ItemDef::material(
                    &format!("core:{key}_{piece}"),
                    &format!("{display} {piece_display}"),
                )
            });
        }
    }
    items
}

/// The blocks of milestone 20: a target and a campfire.
pub fn register_blocks(reg: &mut BlockRegistry) {
    reg.register(
        BlockDef::solid(
            "core:target",
            "Target",
            T::top_bottom_side("target", "target", "target"),
        )
        .mined(0.5, Some(ToolKind::Hoe), 0),
    );
    reg.register(BlockDef {
        render: RenderKind::Cutout,
        light: 14,
        shape: BlockShape::Boxes(vec![
            [0, 0, 6, 16, 4, 10],
            [6, 0, 0, 10, 4, 16],
            [4, 4, 4, 12, 10, 12],
        ]),
        ..BlockDef::solid(
            "core:campfire",
            "Campfire",
            T::top_bottom_side("campfire_top", "log_top", "campfire_side"),
        )
        .mined(1.0, Some(ToolKind::Axe), 0)
    });
}

/// What a campfire cooks a food into (the same as a furnace would).
pub fn campfire_result<'a>(items: &'a crate::item::ItemRegistry, item: &str) -> Option<&'a str> {
    let def = items.get(items.by_name(item)?)?;
    let result = def.smelts_into.as_deref()?;
    let cooked = items.get(items.by_name(result)?)?;
    (cooked.food > 0).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemRegistry;

    #[test]
    fn armour_fits_its_slot() {
        let items = ItemRegistry::build(&BlockRegistry::with_builtin_blocks(), &[]);
        let boots = items
            .get(items.by_name("core:diamond_boots").unwrap())
            .unwrap();
        assert_eq!(boots.kind, ItemKind::Armour { slot: 3, points: 3 });
        assert_eq!(
            campfire_result(&items, "core:fish"),
            Some("core:cooked_fish")
        );
        assert_eq!(
            campfire_result(&items, "core:sand"),
            None,
            "glass isn't food"
        );
    }
}

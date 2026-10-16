//! Our own magic (milestone 24): a wish table that gives tools and armour
//! a sparkle (faster and longer-lasting tools, a little more protection
//! from armour), and potions brewed in bottles (hop high, glow in the
//! dark, swim like a fish).
//!
//! A sparkly item is its own item, `core:sparkly_<name>`, so it's saved
//! and sent like any other.

use crate::block::{BlockDef, BlockRegistry, BlockShape, BlockTextures as T, RenderKind, ToolKind};
use crate::item::{ItemDef, ItemKind};

/// Sparkly tools break blocks this much faster.
pub const SPARKLE_SPEED: f32 = 1.5;

/// The potions: (name, display name, the effect, seconds it lasts).
pub const POTIONS: [(&str, &str, Effect, f32); 3] = [
    ("core:hop_potion", "Hop Potion", Effect::Hop, 90.0),
    ("core:glow_potion", "Glow Potion", Effect::Glow, 120.0),
    ("core:swim_potion", "Swim Potion", Effect::Swim, 120.0),
];

/// What a potion does while it lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    /// Jump much higher.
    Hop,
    /// See in the dark.
    Glow,
    /// Swim faster, and never run out of breath.
    Swim,
}

impl Effect {
    pub const ALL: [Effect; 3] = [Effect::Hop, Effect::Glow, Effect::Swim];

    /// This effect's bit in [`crate::physics::PlayerInput::effects`].
    pub fn bit(self) -> u8 {
        1 << self as u8
    }

    /// The bits of the effects with time left (`seconds` in [`Self::ALL`]
    /// order).
    pub fn bits(seconds: &[f32; 3]) -> u8 {
        Self::ALL
            .iter()
            .zip(seconds)
            .filter(|(_, s)| **s > 0.0)
            .fold(0, |b, (e, _)| b | e.bit())
    }
}

/// The effect of drinking an item, if it's a potion.
pub fn potion_effect(item: &str) -> Option<(Effect, f32)> {
    POTIONS.iter().find(|p| p.0 == item).map(|p| (p.2, p.3))
}

/// The sparkly version of an item's name.
pub fn sparkly(name: &str) -> String {
    format!("core:sparkly_{}", name.trim_start_matches("core:"))
}

/// Sparkly versions of every tool and piece of armour among `items`.
pub fn sparkly_items(items: &[ItemDef]) -> Vec<ItemDef> {
    items
        .iter()
        .filter_map(|d| {
            let kind = match d.kind {
                ItemKind::Tool { .. } => d.kind,
                ItemKind::Armour { slot, points } => ItemKind::Armour {
                    slot,
                    points: points + 1,
                },
                _ => return None,
            };
            Some(ItemDef {
                name: sparkly(&d.name),
                display_name: format!("Sparkly {}", d.display_name),
                kind,
                durability: d.durability * 2,
                sparkly: true,
                ..d.clone()
            })
        })
        .collect()
}

/// Potions, bottles and wish stars.
pub fn items() -> Vec<ItemDef> {
    let mut items = vec![
        ItemDef::material("core:bottle", "Bottle"),
        ItemDef::material("core:wish_star", "Wish Star"),
    ];
    for (name, display, _, _) in POTIONS {
        items.push(ItemDef {
            max_stack: 1,
            leaves: Some("core:bottle".into()),
            ..ItemDef::material(name, display)
        });
    }
    items
}

/// The wish table.
pub fn register_blocks(reg: &mut BlockRegistry) {
    reg.register(BlockDef {
        render: RenderKind::Cutout,
        light: 7,
        shape: BlockShape::Boxes(vec![[0, 0, 0, 16, 12, 16]]),
        ..BlockDef::solid(
            "core:wish_table",
            "Wish Table",
            T::top_bottom_side("wish_table_top", "obsidian", "wish_table_side"),
        )
        .mined(5.0, Some(ToolKind::Pickaxe), 1)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ItemRegistry;

    #[test]
    fn sparkly_tools_last_longer_and_armour_protects_more() {
        let items = ItemRegistry::build(&BlockRegistry::with_builtin_blocks(), &[]);
        let plain = items
            .get(items.by_name("core:iron_pickaxe").unwrap())
            .unwrap();
        let shiny = items
            .get(items.by_name(&sparkly("core:iron_pickaxe")).unwrap())
            .unwrap();
        assert_eq!(shiny.durability, plain.durability * 2);
        assert!(shiny.sparkly && shiny.tool() == plain.tool());
        let helmet = items
            .get(items.by_name(&sparkly("core:diamond_helmet")).unwrap())
            .unwrap();
        assert_eq!(helmet.kind, ItemKind::Armour { slot: 0, points: 4 });
        assert_eq!(
            potion_effect("core:hop_potion").map(|p| p.0),
            Some(Effect::Hop)
        );
    }
}

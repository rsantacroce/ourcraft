//! Items: everything that can sit in an inventory slot.
//!
//! Every block is also an item, with the **same number**: item 5 is block 5
//! (logs). Things that aren't blocks (sticks, coal, ingots, tools) come
//! after all the blocks. So the item list is built from the block list once
//! all mods have added their blocks.
//!
//! Like blocks, items are saved by *name*, so their numbers may change
//! between runs without breaking saved inventories.

use serde::{Deserialize, Serialize};

use crate::block::{BlockId, BlockRegistry, Drops, ToolKind};

/// Identifies a kind of item. Index into [`ItemRegistry`].
#[derive(
    Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize, PartialOrd, Ord,
)]
pub struct ItemId(pub u16);

impl ItemId {
    /// "No item" (it's the item for air).
    pub const NONE: ItemId = ItemId(0);

    /// The item for a block.
    pub fn from_block(b: BlockId) -> Self {
        ItemId(b.0)
    }
}

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    /// Places this block.
    Block(BlockId),
    /// Something for crafting or smelting (sticks, coal, ingots…).
    Material,
    /// Breaks blocks faster. Tiers: 1 wood, 2 stone, 3 iron, 4 diamond.
    Tool { kind: ToolKind, tier: u8 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemDef {
    /// Namespaced name, e.g. `"core:stick"`. Blocks use their block name.
    pub name: String,
    pub display_name: String,
    pub kind: ItemKind,
    pub max_stack: u16,
    /// Uses before a tool breaks (0 = never wears out).
    pub durability: u16,
    /// Seconds a furnace burns on one of these (0 = not a fuel).
    pub fuel: f32,
    /// What a furnace turns this into, by name.
    pub smelts_into: Option<String>,
    /// Hunger points it fills when eaten (0 = not food). Out of 20.
    pub food: u8,
    /// Health it gives back when eaten. Out of 20.
    pub heals: u8,
}

impl ItemDef {
    fn material(name: &str, display: &str) -> Self {
        Self {
            name: name.into(),
            display_name: display.into(),
            kind: ItemKind::Material,
            max_stack: 64,
            durability: 0,
            fuel: 0.0,
            smelts_into: None,
            food: 0,
            heals: 0,
        }
    }

    fn new_tool(name: &str, display: &str, kind: ToolKind, tier: u8) -> Self {
        Self {
            kind: ItemKind::Tool { kind, tier },
            max_stack: 1,
            durability: TOOL_DURABILITY[tier as usize - 1],
            fuel: if tier == 1 { 10.0 } else { 0.0 },
            ..Self::material(name, display)
        }
    }

    pub fn block(&self) -> Option<BlockId> {
        match self.kind {
            ItemKind::Block(b) if !b.is_air() => Some(b),
            _ => None,
        }
    }

    pub fn tool(&self) -> Option<(ToolKind, u8)> {
        match self.kind {
            ItemKind::Tool { kind, tier } => Some((kind, tier)),
            _ => None,
        }
    }
}

/// Uses per tier: wood, stone, iron, diamond.
pub const TOOL_DURABILITY: [u16; 4] = [60, 132, 251, 1562];
/// How much faster the right tool is, per tier.
pub const TOOL_SPEED: [f32; 4] = [2.0, 4.0, 6.0, 8.0];
/// Seconds a furnace takes to smelt one item.
pub const SMELT_TIME: f32 = 5.0;
/// Breaking takes a bit less time than the hardness suggests: kids are
/// impatient, and so are grown-ups.
const BREAK_SPEEDUP: f32 = 0.7;

/// Non-block items that come with the game. Icons are drawn by the client
/// (by name, without the `core:` part).
fn builtin_items() -> Vec<ItemDef> {
    use ToolKind::{Axe, Pickaxe, Shovel};
    let mut items = vec![
        ItemDef {
            fuel: 2.5,
            ..ItemDef::material("core:stick", "Stick")
        },
        ItemDef {
            fuel: 40.0,
            ..ItemDef::material("core:coal", "Coal")
        },
        ItemDef::material("core:iron_ingot", "Iron Ingot"),
        ItemDef::material("core:gold_ingot", "Gold Ingot"),
        ItemDef::material("core:diamond", "Diamond"),
        ItemDef {
            smelts_into: Some("core:brick".into()),
            ..ItemDef::material("core:clay_ball", "Clay Ball")
        },
        ItemDef::material("core:brick", "Brick"),
        ItemDef {
            max_stack: 1,
            durability: 120,
            ..ItemDef::material("core:shears", "Shears")
        },
        ItemDef {
            max_stack: 16,
            smelts_into: Some("core:fried_egg".into()),
            ..ItemDef::material("core:egg", "Egg")
        },
        ItemDef {
            food: 4,
            ..ItemDef::material("core:apple", "Apple")
        },
        // ---- Milestone 15: food ----
        ItemDef {
            food: 6,
            ..ItemDef::material("core:fried_egg", "Fried Egg")
        },
        ItemDef {
            food: 4,
            heals: 8,
            ..ItemDef::material("core:golden_apple", "Golden Apple")
        },
    ];
    for (tier, material) in [(1, "Wooden"), (2, "Stone"), (3, "Iron"), (4, "Diamond")] {
        let key = ["wood", "stone", "iron", "diamond"][tier as usize - 1];
        for (kind, tool) in [(Pickaxe, "Pickaxe"), (Axe, "Axe"), (Shovel, "Shovel")] {
            items.push(ItemDef::new_tool(
                &format!("core:{key}_{}", tool.to_lowercase()),
                &format!("{material} {tool}"),
                kind,
                tier,
            ));
        }
    }
    items
}

/// Furnace and fuel properties of blocks, by block name.
fn block_extras(name: &str) -> (f32, Option<&'static str>) {
    match name {
        "core:log" => (15.0, Some("core:coal")),
        "core:planks" => (7.5, None),
        "core:crafting_table" => (15.0, None),
        "core:wool" => (5.0, None),
        "core:sand" => (0.0, Some("core:glass")),
        "core:cobblestone" => (0.0, Some("core:stone")),
        "core:iron_ore" => (0.0, Some("core:iron_ingot")),
        "core:gold_ore" => (0.0, Some("core:gold_ingot")),
        "core:clay" => (0.0, Some("core:bricks")),
        _ => (0.0, None),
    }
}

/// All known items, indexed by [`ItemId`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemRegistry {
    defs: Vec<ItemDef>,
}

impl ItemRegistry {
    /// One item per block (same numbers), then the built-in items, then
    /// `extra` (from mods).
    pub fn build(blocks: &BlockRegistry, extra: &[ItemDef]) -> Self {
        let mut defs: Vec<ItemDef> = blocks
            .iter()
            .map(|(id, b)| {
                let (fuel, smelts) = block_extras(&b.name);
                ItemDef {
                    name: b.name.clone(),
                    display_name: b.display_name.clone(),
                    kind: ItemKind::Block(id),
                    max_stack: 64,
                    durability: 0,
                    fuel,
                    smelts_into: smelts.map(Into::into),
                    food: 0,
                    heals: 0,
                }
            })
            .collect();
        for item in builtin_items().into_iter().chain(extra.iter().cloned()) {
            if !defs.iter().any(|d| d.name == item.name) {
                defs.push(item);
            }
        }
        Self { defs }
    }

    /// A registry from a full list (as sent by the server).
    pub fn from_defs(defs: Vec<ItemDef>) -> Self {
        Self { defs }
    }

    pub fn defs(&self) -> &[ItemDef] {
        &self.defs
    }

    pub fn get(&self, id: ItemId) -> Option<&ItemDef> {
        self.defs.get(id.0 as usize)
    }

    pub fn by_name(&self, name: &str) -> Option<ItemId> {
        self.defs
            .iter()
            .position(|d| d.name == name)
            .map(|i| ItemId(i as u16))
    }

    pub fn iter(&self) -> impl Iterator<Item = (ItemId, &ItemDef)> {
        self.defs
            .iter()
            .enumerate()
            .map(|(i, d)| (ItemId(i as u16), d))
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn max_stack(&self, id: ItemId) -> u16 {
        self.get(id).map_or(64, |d| d.max_stack.max(1))
    }

    pub fn name(&self, id: ItemId) -> &str {
        self.get(id).map_or("core:air", |d| &d.name)
    }

    pub fn display_name(&self, id: ItemId) -> &str {
        self.get(id).map_or("?", |d| &d.display_name)
    }

    /// The tool an item is, if it's one.
    fn tool_of(&self, held: Option<ItemId>) -> Option<(ToolKind, u8)> {
        held.and_then(|h| self.get(h)).and_then(|d| d.tool())
    }

    /// Whether breaking `block` while holding `held` gives anything.
    pub fn can_harvest(
        &self,
        blocks: &BlockRegistry,
        block: BlockId,
        held: Option<ItemId>,
    ) -> bool {
        let def = blocks.get(block);
        if def.min_tier == 0 {
            return true;
        }
        match (self.tool_of(held), def.tool) {
            (Some((kind, tier)), Some(needed)) => kind == needed && tier >= def.min_tier,
            _ => false,
        }
    }

    /// Seconds to break `block` while holding `held` (in survival).
    pub fn break_time(&self, blocks: &BlockRegistry, block: BlockId, held: Option<ItemId>) -> f32 {
        let def = blocks.get(block);
        let speed = match (self.tool_of(held), def.tool) {
            (Some((kind, tier)), Some(best)) if kind == best => TOOL_SPEED[tier as usize - 1],
            _ => 1.0,
        };
        let penalty = if self.can_harvest(blocks, block, held) {
            1.5
        } else {
            5.0
        };
        def.hardness * penalty / speed * BREAK_SPEEDUP
    }

    /// What you get for breaking `block` while holding `held`.
    pub fn drop_for(
        &self,
        blocks: &BlockRegistry,
        block: BlockId,
        held: Option<ItemId>,
    ) -> Option<(ItemId, u16)> {
        if !self.can_harvest(blocks, block, held) {
            return None;
        }
        match &blocks.get(block).drops {
            Drops::Itself => Some((ItemId::from_block(block), 1)),
            Drops::Block(b) => Some((ItemId::from_block(*b), 1)),
            Drops::Item(name, n) => self.by_name(name).map(|i| (i, *n)),
            Drops::Nothing => None,
        }
    }

    /// What a furnace makes from this item.
    pub fn smelt_result(&self, id: ItemId) -> Option<ItemId> {
        self.get(id)?
            .smelts_into
            .as_deref()
            .and_then(|n| self.by_name(n))
    }

    pub fn fuel(&self, id: ItemId) -> f32 {
        self.get(id).map_or(0.0, |d| d.fuel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks;

    fn setup() -> (BlockRegistry, ItemRegistry) {
        let b = BlockRegistry::with_builtin_blocks();
        let i = ItemRegistry::build(&b, &[]);
        (b, i)
    }

    #[test]
    fn blocks_and_items_share_numbers() {
        let (b, i) = setup();
        assert_eq!(i.get(ItemId(5)).unwrap().name, "core:log");
        assert_eq!(i.get(ItemId(5)).unwrap().block(), Some(blocks::LOG));
        let stick = i.by_name("core:stick").unwrap();
        assert!(stick.0 as usize >= b.len(), "items come after blocks");
        assert_eq!(i.get(stick).unwrap().block(), None);
        assert_eq!(i.max_stack(i.by_name("core:iron_pickaxe").unwrap()), 1);
    }

    #[test]
    fn stone_needs_a_pickaxe() {
        let (b, i) = setup();
        let hand = None;
        let wood_pick = i.by_name("core:wood_pickaxe");
        assert_eq!(i.drop_for(&b, blocks::STONE, hand), None);
        assert_eq!(
            i.drop_for(&b, blocks::STONE, wood_pick),
            Some((ItemId::from_block(blocks::COBBLESTONE), 1))
        );
        assert!(i.break_time(&b, blocks::STONE, wood_pick) < i.break_time(&b, blocks::STONE, hand));
        // Dirt is fine by hand, and a shovel is quicker.
        assert!(i.drop_for(&b, blocks::DIRT, hand).is_some());
        let shovel = i.by_name("core:stone_shovel");
        assert!(i.break_time(&b, blocks::DIRT, shovel) < i.break_time(&b, blocks::DIRT, hand));
    }

    #[test]
    fn ores_need_the_right_tier() {
        let (b, i) = setup();
        let stone_pick = i.by_name("core:stone_pickaxe");
        let iron_pick = i.by_name("core:iron_pickaxe");
        assert!(i.drop_for(&b, blocks::IRON_ORE, stone_pick).is_some());
        assert!(i.drop_for(&b, blocks::DIAMOND_ORE, stone_pick).is_none());
        assert_eq!(
            i.drop_for(&b, blocks::DIAMOND_ORE, iron_pick),
            Some((i.by_name("core:diamond").unwrap(), 1))
        );
        assert_eq!(
            i.drop_for(&b, blocks::COAL_ORE, i.by_name("core:wood_pickaxe")),
            Some((i.by_name("core:coal").unwrap(), 1))
        );
    }

    #[test]
    fn smelting_and_fuel() {
        let (_, i) = setup();
        let ore = i.by_name("core:iron_ore").unwrap();
        assert_eq!(i.smelt_result(ore), i.by_name("core:iron_ingot"));
        assert_eq!(
            i.smelt_result(ItemId::from_block(blocks::SAND)),
            Some(ItemId::from_block(blocks::GLASS))
        );
        assert!(i.fuel(i.by_name("core:coal").unwrap()) > 0.0);
        assert!(i.fuel(ItemId::from_block(blocks::PLANKS)) > 0.0);
        assert_eq!(i.fuel(ItemId::from_block(blocks::STONE)), 0.0);
    }
}

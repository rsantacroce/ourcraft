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
    /// Worn in an armour slot (0 helmet … 3 boots); `points` of
    /// protection.
    Armour { slot: u8, points: u8 },
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
    /// What's left after eating it or crafting with it (a bowl, an empty
    /// bucket).
    pub leaves: Option<String>,
    /// Given a sparkle at a wish table (faster tools, see `magic`).
    pub sparkly: bool,
}

impl ItemDef {
    pub(crate) fn material(name: &str, display: &str) -> Self {
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
            leaves: None,
            sparkly: false,
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

    /// An item from a mod.
    pub fn from_spec(spec: &ourcraft_mod_api::ItemSpec) -> Self {
        Self {
            max_stack: spec.max_stack.clamp(1, 64),
            food: spec.food.min(20),
            heals: spec.heals.min(20),
            fuel: spec.fuel.clamp(0.0, 600.0),
            smelts_into: spec.smelts_into.clone(),
            ..Self::material(&spec.name, &spec.display_name)
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
fn builtin_items(blocks: &BlockRegistry) -> Vec<ItemDef> {
    use ToolKind::{Axe, Hoe, Pickaxe, Shovel};
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
        ItemDef {
            max_stack: 16,
            ..ItemDef::material("core:bucket", "Bucket")
        },
        ItemDef {
            max_stack: 1,
            ..ItemDef::material("core:water_bucket", "Water Bucket")
        },
        ItemDef {
            max_stack: 1,
            ..ItemDef::material("core:lava_bucket", "Lava Bucket")
        },
        // ---- Milestone 17: farming ----
        ItemDef::material("core:wheat", "Wheat"),
        plants(blocks, "core:wheat_seeds", "Wheat Seeds", "core:wheat_crop"),
        ItemDef {
            food: 3,
            ..plants(blocks, "core:carrot", "Carrot", "core:carrot_crop")
        },
        ItemDef {
            food: 1,
            smelts_into: Some("core:baked_potato".into()),
            ..plants(blocks, "core:potato", "Potato", "core:potato_crop")
        },
        ItemDef {
            food: 5,
            ..ItemDef::material("core:baked_potato", "Baked Potato")
        },
        ItemDef {
            food: 1,
            ..ItemDef::material("core:beetroot", "Beetroot")
        },
        plants(
            blocks,
            "core:beetroot_seeds",
            "Beetroot Seeds",
            "core:beetroot_crop",
        ),
        plants(
            blocks,
            "core:pumpkin_seeds",
            "Pumpkin Seeds",
            "core:pumpkin_stem",
        ),
        plants(blocks, "core:melon_seeds", "Melon Seeds", "core:melon_stem"),
        ItemDef {
            food: 2,
            ..ItemDef::material("core:melon_slice", "Melon Slice")
        },
        ItemDef::material("core:sugar", "Sugar"),
        ItemDef::material("core:paper", "Paper"),
        ItemDef {
            food: 5,
            ..ItemDef::material("core:bread", "Bread")
        },
        ItemDef {
            food: 8,
            ..ItemDef::material("core:pumpkin_pie", "Pumpkin Pie")
        },
        ItemDef {
            food: 2,
            ..ItemDef::material("core:cookie", "Cookie")
        },
        ItemDef {
            fuel: 2.5,
            ..ItemDef::material("core:bowl", "Bowl")
        },
        ItemDef {
            food: 6,
            max_stack: 1,
            leaves: Some("core:bowl".into()),
            ..ItemDef::material("core:beetroot_soup", "Beetroot Soup")
        },
        ItemDef {
            food: 6,
            max_stack: 1,
            leaves: Some("core:bowl".into()),
            ..ItemDef::material("core:mushroom_stew", "Mushroom Stew")
        },
        ItemDef {
            food: 2,
            max_stack: 1,
            leaves: Some("core:bucket".into()),
            ..ItemDef::material("core:milk_bucket", "Milk Bucket")
        },
        ItemDef::material("core:bone_meal", "Bone Meal"),
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
        for (kind, tool) in [
            (Pickaxe, "Pickaxe"),
            (Axe, "Axe"),
            (Shovel, "Shovel"),
            (Hoe, "Hoe"),
        ] {
            items.push(ItemDef::new_tool(
                &format!("core:{key}_{}", tool.to_lowercase()),
                &format!("{material} {tool}"),
                kind,
                tier,
            ));
        }
    }
    // ---- Milestone 23: honey ----
    items.push(ItemDef {
        food: 6,
        heals: 2,
        max_stack: 1,
        leaves: Some("core:bowl".into()),
        ..ItemDef::material("core:honey_bowl", "Bowl of Honey")
    });
    // ---- Milestone 22: spark dust (it lays wire) and mine carts ----
    items.push(plants(
        blocks,
        "core:spark_dust",
        "Spark Dust",
        "core:spark_wire",
    ));
    items.push(ItemDef {
        max_stack: 1,
        ..ItemDef::material("core:minecart", "Mine Cart")
    });
    // ---- Milestone 21: books ----
    items.push(ItemDef {
        max_stack: 16,
        ..ItemDef::material("core:book", "Book")
    });
    // ---- Milestone 20: tools and adventure ----
    items.extend(crate::adventure::items());
    // ---- Milestone 19: snowballs and a boat of every wood ----
    items.push(ItemDef {
        max_stack: 16,
        ..ItemDef::material("core:snowball", "Snowball")
    });
    for wood in crate::woods::Wood::ALL {
        items.push(ItemDef {
            max_stack: 1,
            ..ItemDef::material(&wood.boat(), &format!("{} Boat", wood.display()))
        });
    }
    // ---- Milestone 18: a dye in every colour ----
    for (c, display, _) in crate::colours::COLOURS {
        items.push(ItemDef::material(
            &format!("core:{c}_dye"),
            &format!("{display} Dye"),
        ));
    }
    // ---- Milestone 24: magic (sparkly tools and armour come last, made
    // from everything above) ----
    items.extend(crate::magic::items());
    let shiny = crate::magic::sparkly_items(&items);
    items.extend(shiny);
    items
}

/// Something you plant: placing it puts `crop` in the world.
fn plants(blocks: &BlockRegistry, name: &str, display: &str, crop: &str) -> ItemDef {
    ItemDef {
        kind: blocks
            .by_name(crop)
            .map_or(ItemKind::Material, ItemKind::Block),
        ..ItemDef::material(name, display)
    }
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
        "core:composter" => (15.0, None),
        "core:cactus" => (0.0, Some("core:green_dye")),
        "core:stone" => (0.0, Some("core:smooth_stone")),
        "core:stone_bricks" => (0.0, Some("core:cracked_stone_bricks")),
        "core:sandstone" => (0.0, Some("core:smooth_sandstone")),
        "core:cobbled_deep_slate" => (0.0, Some("core:deep_slate")),
        // Every wood burns, and logs turn to coal.
        n if n.ends_with("_log") => (15.0, Some("core:coal")),
        n if n.ends_with("_planks") || n.ends_with("_sapling") => (7.5, None),
        _ => (0.0, None),
    }
}

/// All known items, indexed by [`ItemId`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemRegistry {
    defs: Vec<ItemDef>,
    /// Name → id, for quick lookups.
    names: std::collections::HashMap<String, ItemId>,
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
                    leaves: None,
                    sparkly: false,
                }
            })
            .collect();
        for item in builtin_items(blocks)
            .into_iter()
            .chain(extra.iter().cloned())
        {
            if !defs.iter().any(|d| d.name == item.name) {
                defs.push(item);
            }
        }
        Self::indexed(defs)
    }

    /// A registry from a full list (as sent by the server).
    pub fn from_defs(defs: Vec<ItemDef>) -> Self {
        Self::indexed(defs)
    }

    fn indexed(defs: Vec<ItemDef>) -> Self {
        let mut names = std::collections::HashMap::new();
        for (i, d) in defs.iter().enumerate() {
            names.entry(d.name.clone()).or_insert(ItemId(i as u16));
        }
        Self { defs, names }
    }

    pub fn defs(&self) -> &[ItemDef] {
        &self.defs
    }

    pub fn get(&self, id: ItemId) -> Option<&ItemDef> {
        self.defs.get(id.0 as usize)
    }

    pub fn by_name(&self, name: &str) -> Option<ItemId> {
        self.names.get(name).copied()
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
            (Some((kind, tier)), Some(best)) if kind == best => {
                let sparkle = held.and_then(|h| self.get(h)).is_some_and(|d| d.sparkly);
                TOOL_SPEED[tier as usize - 1]
                    * if sparkle {
                        crate::magic::SPARKLE_SPEED
                    } else {
                        1.0
                    }
            }
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
            Drops::Harvest(_) | Drops::Nothing => None,
        }
    }

    /// Everything a broken block gives: its normal drop, or a harvest of
    /// several items (each a random amount, picked with `luck`).
    pub fn drops(
        &self,
        blocks: &BlockRegistry,
        block: BlockId,
        held: Option<ItemId>,
        luck: u32,
    ) -> Vec<(ItemId, u16)> {
        if let Drops::Harvest(list) = &blocks.get(block).drops {
            if !self.can_harvest(blocks, block, held) {
                return Vec::new();
            }
            return list
                .iter()
                .enumerate()
                .filter_map(|(i, (name, lo, hi))| {
                    let spread = (hi.saturating_sub(*lo) + 1) as u32;
                    let roll = luck.rotate_left(i as u32 * 7) % spread;
                    let n = lo + roll as u16;
                    Some((self.by_name(name)?, n)).filter(|(_, n)| *n > 0)
                })
                .collect();
        }
        self.drop_for(blocks, block, held).into_iter().collect()
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

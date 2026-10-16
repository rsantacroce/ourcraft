//! Block types and the registry that describes them.
//!
//! A block in the world is just a [`BlockId`] (a `u16`). Everything else about
//! it (its name, whether it's solid, which textures it uses) lives once in the
//! [`BlockRegistry`]. Mods will add entries to this registry in M8.

use serde::{Deserialize, Serialize};

/// Identifies a kind of block. Index into [`BlockRegistry`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub struct BlockId(pub u16);

impl BlockId {
    pub const AIR: BlockId = BlockId(0);

    pub fn is_air(self) -> bool {
        self == Self::AIR
    }
}

/// The built-in blocks. Their ids are fixed by the order they are registered
/// in [`BlockRegistry::with_builtin_blocks`].
pub mod blocks {
    use super::BlockId;

    pub const AIR: BlockId = BlockId(0);
    pub const STONE: BlockId = BlockId(1);
    pub const DIRT: BlockId = BlockId(2);
    pub const GRASS: BlockId = BlockId(3);
    pub const SAND: BlockId = BlockId(4);
    pub const LOG: BlockId = BlockId(5);
    pub const LEAVES: BlockId = BlockId(6);
    pub const PLANKS: BlockId = BlockId(7);
    pub const SNOW: BlockId = BlockId(8);
    pub const CACTUS: BlockId = BlockId(9);
    pub const BEDROCK: BlockId = BlockId(10);
    pub const GRAVEL: BlockId = BlockId(11);
    pub const COBBLESTONE: BlockId = BlockId(12);
    pub const GLASS: BlockId = BlockId(13);
    pub const BRICKS: BlockId = BlockId(14);
    pub const LAMP: BlockId = BlockId(15);
    pub const WATER: BlockId = BlockId(16);
    pub const COAL_ORE: BlockId = BlockId(17);
    pub const IRON_ORE: BlockId = BlockId(18);
    pub const GOLD_ORE: BlockId = BlockId(19);
    pub const DIAMOND_ORE: BlockId = BlockId(20);
    pub const CLAY: BlockId = BlockId(21);
    pub const CRAFTING_TABLE: BlockId = BlockId(22);
    pub const FURNACE: BlockId = BlockId(23);
    pub const IRON_BLOCK: BlockId = BlockId(24);
    pub const GOLD_BLOCK: BlockId = BlockId(25);
    pub const DIAMOND_BLOCK: BlockId = BlockId(26);
    pub const WOOL: BlockId = BlockId(27);
    pub const TORCH: BlockId = BlockId(28);
    pub const WALL_TORCH_EAST: BlockId = BlockId(29);
    pub const WALL_TORCH_WEST: BlockId = BlockId(30);
    pub const WALL_TORCH_SOUTH: BlockId = BlockId(31);
    pub const WALL_TORCH_NORTH: BlockId = BlockId(32);
    pub const LAVA: BlockId = BlockId(111);
}

/// The six faces of a block, in the order used for per-face lookups.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Face {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl Face {
    pub const ALL: [Face; 6] = [
        Face::PosX,
        Face::NegX,
        Face::PosY,
        Face::NegY,
        Face::PosZ,
        Face::NegZ,
    ];

    /// The unit offset to the neighbouring block on this side.
    pub fn normal(self) -> [i32; 3] {
        match self {
            Face::PosX => [1, 0, 0],
            Face::NegX => [-1, 0, 0],
            Face::PosY => [0, 1, 0],
            Face::NegY => [0, -1, 0],
            Face::PosZ => [0, 0, 1],
            Face::NegZ => [0, 0, -1],
        }
    }
}

/// Texture names for each face of a block. The client turns names into
/// texture-array layers; names (rather than numbers) let mods ship their own
/// textures without clashing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockTextures {
    pub top: String,
    pub bottom: String,
    pub side: String,
}

impl BlockTextures {
    /// The same texture on all six faces.
    pub fn all(name: &str) -> Self {
        Self {
            top: name.to_owned(),
            bottom: name.to_owned(),
            side: name.to_owned(),
        }
    }

    pub fn top_bottom_side(top: &str, bottom: &str, side: &str) -> Self {
        Self {
            top: top.to_owned(),
            bottom: bottom.to_owned(),
            side: side.to_owned(),
        }
    }

    pub fn for_face(&self, face: Face) -> &str {
        match face {
            Face::PosY => &self.top,
            Face::NegY => &self.bottom,
            _ => &self.side,
        }
    }
}

/// How a block is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderKind {
    /// Not drawn at all (air).
    Invisible,
    /// Fully solid: hides the faces of neighbouring blocks.
    Opaque,
    /// Drawn with see-through holes (leaves, glass). Pixels with low alpha
    /// are skipped, and neighbours stay visible behind it.
    Cutout,
    /// See-through and tinted (water). Drawn in a separate, blended pass
    /// after everything solid.
    Translucent,
}

/// What you get when you break a block (in survival).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Drops {
    /// The block itself.
    Itself,
    /// A different block (stone drops cobblestone).
    Block(BlockId),
    /// Some of an item, by name (coal ore drops `core:coal`).
    Item(String, u16),
    /// Several items, each a random amount from `min` to `max` (a ripe
    /// crop gives wheat and seeds): `(name, min, max)`.
    Harvest(Vec<(String, u16, u16)>),
    Nothing,
}

/// One box of a block's shape, in sixteenths of a block:
/// `[x0, y0, z0, x1, y1, z1]` (0–16).
pub type ShapeBox = [u8; 6];

/// What a block looks like: a full cube, or some smaller boxes (torches,
/// and later slabs, stairs, fences, doors…).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlockShape {
    #[default]
    Cube,
    Boxes(Vec<ShapeBox>),
    /// A post that grows rails towards its neighbours (see
    /// `BlockRegistry::boxes_at`).
    Fence,
    /// A thick post that joins walls, fences and solid blocks with low
    /// walls.
    Wall,
}

impl BlockShape {
    /// The boxes in block units (0–1), as (min, max) corners.
    pub fn boxes(&self) -> Vec<([f32; 3], [f32; 3])> {
        match self {
            BlockShape::Cube => vec![([0.0; 3], [1.0; 3])],
            BlockShape::Fence => BlockShape::Boxes(vec![[6, 0, 6, 10, 16, 10]]).boxes(),
            BlockShape::Wall => BlockShape::Boxes(vec![[4, 0, 4, 12, 16, 12]]).boxes(),
            BlockShape::Boxes(b) => b
                .iter()
                .map(|b| {
                    let f = |v: u8| v as f32 / 16.0;
                    ([f(b[0]), f(b[1]), f(b[2])], [f(b[3]), f(b[4]), f(b[5])])
                })
                .collect(),
        }
    }

    pub fn is_cube(&self) -> bool {
        matches!(self, BlockShape::Cube)
    }
}

/// The kinds of tool. Each block is quickest to break with one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolKind {
    Pickaxe,
    Axe,
    Shovel,
    /// Turns dirt into farmland (and cuts plants quickly).
    Hoe,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockDef {
    /// Namespaced name, e.g. `"core:stone"`. Mods use their own namespace.
    pub name: String,
    /// Name shown to players.
    pub display_name: String,
    pub render: RenderKind,
    /// Players and entities collide with solid blocks.
    pub solid: bool,
    /// Can players break it?
    pub breakable: bool,
    /// Players swim in liquids instead of falling.
    pub liquid: bool,
    /// Light emitted, 0–15 (0 = none). Used for glowing blocks.
    pub light: u8,
    pub drops: Drops,
    /// `None` for air (nothing is drawn).
    pub textures: Option<BlockTextures>,
    /// How long it takes to break, roughly in seconds by hand.
    pub hardness: f32,
    /// The tool that breaks it fastest.
    pub tool: Option<ToolKind>,
    /// The tool tier needed to get anything when it breaks (0 = bare
    /// hands are fine; 1 = wood, 2 = stone, 3 = iron, 4 = diamond).
    pub min_tier: u8,
    /// Sometimes drops these items as well: (item name, one time in N).
    pub bonus_drop: Vec<(String, u16)>,
    pub shape: BlockShape,
    /// Left out of the creative palette (variants like wall torches, which
    /// you get by placing the normal kind).
    pub hidden: bool,
    /// Its family and state, for blocks placed in different ways (stairs
    /// facing east, an open door…). See `shapes`.
    pub variant: Option<crate::shapes::Variant>,
    /// Part of a fluid (water, lava). See `fluids`.
    pub fluid: Option<crate::fluids::Fluid>,
    /// Falls when there's nothing under it (sand, gravel).
    pub falls: bool,
    /// Plants that grow, cakes being eaten, composters filling up. See
    /// `farming`.
    pub stage: Option<crate::farming::Stage>,
}

impl BlockDef {
    /// A normal solid, opaque, breakable block.
    pub fn solid(name: &str, display_name: &str, textures: BlockTextures) -> Self {
        Self {
            name: name.to_owned(),
            display_name: display_name.to_owned(),
            render: RenderKind::Opaque,
            solid: true,
            breakable: true,
            liquid: false,
            light: 0,
            drops: Drops::Itself,
            textures: Some(textures),
            hardness: 1.0,
            tool: None,
            min_tier: 0,
            bonus_drop: Vec::new(),
            shape: BlockShape::Cube,
            hidden: false,
            variant: None,
            fluid: None,
            falls: false,
            stage: None,
        }
    }

    /// Set how it's broken: hardness, best tool and the tier it needs.
    pub fn mined(self, hardness: f32, tool: Option<ToolKind>, min_tier: u8) -> Self {
        Self {
            hardness,
            tool,
            min_tier,
            ..self
        }
    }

    /// A solid block you can see through the holes of.
    pub fn cutout(name: &str, display_name: &str, textures: BlockTextures) -> Self {
        Self {
            render: RenderKind::Cutout,
            ..Self::solid(name, display_name, textures)
        }
    }
}

/// All known block types, indexed by [`BlockId`].
#[derive(Clone, Debug)]
pub struct BlockRegistry {
    defs: Vec<BlockDef>,
    /// Name → id, for quick lookups.
    names: std::collections::HashMap<String, BlockId>,
}

impl BlockRegistry {
    /// A registry containing only air.
    pub fn new() -> Self {
        Self {
            names: [("core:air".to_owned(), BlockId::AIR)].into(),
            defs: vec![BlockDef {
                name: "core:air".to_owned(),
                display_name: "Air".to_owned(),
                render: RenderKind::Invisible,
                solid: false,
                breakable: false,
                liquid: false,
                light: 0,
                drops: Drops::Nothing,
                textures: None,
                hardness: 0.0,
                tool: None,
                min_tier: 0,
                bonus_drop: Vec::new(),
                shape: BlockShape::Cube,
                hidden: false,
                variant: None,
                fluid: None,
                falls: false,
                stage: None,
            }],
        }
    }

    /// A registry with all built-in blocks, matching the ids in [`blocks`].
    pub fn with_builtin_blocks() -> Self {
        use BlockTextures as T;
        use ToolKind::{Axe, Pickaxe, Shovel};
        let mut reg = Self::new();
        reg.register(
            BlockDef {
                drops: Drops::Block(blocks::COBBLESTONE),
                ..BlockDef::solid("core:stone", "Stone", T::all("stone"))
            }
            .mined(1.5, Some(Pickaxe), 1),
        );
        reg.register(BlockDef::solid("core:dirt", "Dirt", T::all("dirt")).mined(
            0.5,
            Some(Shovel),
            0,
        ));
        reg.register(
            BlockDef {
                drops: Drops::Block(blocks::DIRT),
                ..BlockDef::solid(
                    "core:grass",
                    "Grass",
                    T::top_bottom_side("grass_top", "dirt", "grass_side"),
                )
            }
            .mined(0.6, Some(Shovel), 0),
        );
        reg.register(BlockDef::solid("core:sand", "Sand", T::all("sand")).mined(
            0.5,
            Some(Shovel),
            0,
        ));
        reg.register(
            BlockDef::solid(
                "core:log",
                "Log",
                T::top_bottom_side("log_top", "log_top", "log_side"),
            )
            .mined(2.0, Some(Axe), 0),
        );
        reg.register(BlockDef {
            // Now and then an apple falls out.
            bonus_drop: vec![("core:apple".into(), 8), ("core:oak_sapling".into(), 12)],
            ..BlockDef::cutout("core:leaves", "Leaves", T::all("leaves")).mined(0.2, None, 0)
        });
        reg.register(
            BlockDef::solid("core:planks", "Planks", T::all("planks")).mined(2.0, Some(Axe), 0),
        );
        reg.register(
            BlockDef {
                drops: Drops::Block(blocks::DIRT),
                bonus_drop: vec![("core:snowball".into(), 1)],
                ..BlockDef::solid(
                    "core:snow",
                    "Snowy Grass",
                    T::top_bottom_side("snow", "dirt", "snow_side"),
                )
            }
            .mined(0.6, Some(Shovel), 0),
        );
        reg.register(
            BlockDef::solid(
                "core:cactus",
                "Cactus",
                T::top_bottom_side("cactus_top", "cactus_top", "cactus_side"),
            )
            .mined(0.4, None, 0),
        );
        reg.register(BlockDef {
            breakable: false,
            ..BlockDef::solid("core:bedrock", "Bedrock", T::all("bedrock"))
        });
        reg.register(
            BlockDef::solid("core:gravel", "Gravel", T::all("gravel")).mined(0.6, Some(Shovel), 0),
        );
        reg.register(
            BlockDef::solid("core:cobblestone", "Cobblestone", T::all("cobblestone")).mined(
                2.0,
                Some(Pickaxe),
                1,
            ),
        );
        reg.register(BlockDef::cutout("core:glass", "Glass", T::all("glass")).mined(0.3, None, 0));
        reg.register(
            BlockDef::solid("core:bricks", "Bricks", T::all("bricks")).mined(2.0, Some(Pickaxe), 1),
        );
        reg.register(BlockDef {
            light: 15,
            ..BlockDef::solid("core:lamp", "Lamp", T::all("lamp")).mined(0.3, None, 0)
        });
        reg.register(BlockDef {
            render: RenderKind::Translucent,
            solid: false,
            breakable: false,
            liquid: true,
            ..BlockDef::solid("core:water", "Water", T::all("water"))
        });
        // ---- Milestone 11: ores, crafting ----
        let ore = |name: &str, display: &str, tex: &str, tier: u8| {
            BlockDef::solid(name, display, T::all(tex)).mined(3.0, Some(Pickaxe), tier)
        };
        reg.register(BlockDef {
            drops: Drops::Item("core:coal".into(), 1),
            ..ore("core:coal_ore", "Coal Ore", "coal_ore", 1)
        });
        reg.register(ore("core:iron_ore", "Iron Ore", "iron_ore", 2));
        reg.register(ore("core:gold_ore", "Gold Ore", "gold_ore", 3));
        reg.register(BlockDef {
            drops: Drops::Item("core:diamond".into(), 1),
            ..ore("core:diamond_ore", "Diamond Ore", "diamond_ore", 3)
        });
        reg.register(BlockDef {
            drops: Drops::Item("core:clay_ball".into(), 4),
            ..BlockDef::solid("core:clay", "Clay", T::all("clay")).mined(0.6, Some(Shovel), 0)
        });
        reg.register(
            BlockDef::solid(
                "core:crafting_table",
                "Crafting Table",
                T::top_bottom_side("crafting_table_top", "planks", "crafting_table_side"),
            )
            .mined(2.5, Some(Axe), 0),
        );
        reg.register(
            BlockDef::solid(
                "core:furnace",
                "Furnace",
                T::top_bottom_side("furnace_top", "furnace_top", "furnace_front"),
            )
            .mined(3.5, Some(Pickaxe), 1),
        );
        reg.register(
            BlockDef::solid("core:iron_block", "Block of Iron", T::all("iron_block")).mined(
                5.0,
                Some(Pickaxe),
                2,
            ),
        );
        reg.register(
            BlockDef::solid("core:gold_block", "Block of Gold", T::all("gold_block")).mined(
                4.0,
                Some(Pickaxe),
                3,
            ),
        );
        reg.register(
            BlockDef::solid(
                "core:diamond_block",
                "Block of Diamond",
                T::all("diamond_block"),
            )
            .mined(5.0, Some(Pickaxe), 3),
        );
        // ---- Milestone 12: animals ----
        reg.register(BlockDef::solid("core:wool", "Wool", T::all("wool")).mined(0.8, None, 0));
        // ---- Milestones 13 and 14: torches and shaped blocks ----
        crate::shapes::register_shaped_blocks(&mut reg);
        // ---- Milestone 15: flowing water and lava ----
        crate::fluids::register_fluids(&mut reg);
        for b in [blocks::SAND, blocks::GRAVEL] {
            reg.defs_mut(b).falls = true;
        }
        // ---- Later additions go last, so earlier block numbers stay put ----
        crate::shapes::register_fence_gates(&mut reg);
        // ---- Milestone 17: farming ----
        crate::farming::register_farming(&mut reg);
        // ---- Milestone 18: colours ----
        crate::colours::register_colours(&mut reg);
        // ---- Milestone 19: trees and stone ----
        crate::woods::register_woods(&mut reg);
        crate::stones::register_stones(&mut reg);
        reg
    }

    /// A registry from a full list of definitions, in id order (as sent by
    /// the server). The first must be air.
    pub fn from_defs(defs: Vec<BlockDef>) -> Option<Self> {
        let air_first = defs
            .first()
            .is_some_and(|d| d.render == RenderKind::Invisible);
        let names = defs
            .iter()
            .enumerate()
            .map(|(i, d)| (d.name.clone(), BlockId(i as u16)))
            .collect();
        (air_first && defs.len() <= u16::MAX as usize).then_some(Self { defs, names })
    }

    /// All definitions in id order.
    pub fn defs(&self) -> &[BlockDef] {
        &self.defs
    }

    /// Add a block made from a mod's description. Returns `None` if the name
    /// is taken.
    pub fn register_spec(&mut self, spec: &ourcraft_mod_api::BlockSpec) -> Option<BlockId> {
        use ourcraft_mod_api::Render;
        if self.by_name(&spec.name).is_some() {
            return None;
        }
        Some(self.register(BlockDef {
            name: spec.name.clone(),
            display_name: spec.display_name.clone(),
            render: match spec.render {
                Render::Opaque => RenderKind::Opaque,
                Render::Cutout => RenderKind::Cutout,
                Render::Translucent => RenderKind::Translucent,
            },
            solid: spec.solid,
            breakable: spec.breakable,
            liquid: spec.render == Render::Translucent && !spec.solid,
            light: spec.light.min(15),
            drops: Drops::Itself,
            textures: Some(BlockTextures::top_bottom_side(
                &spec.texture_top,
                &spec.texture_bottom,
                &spec.texture_side,
            )),
            hardness: 1.0,
            tool: None,
            min_tier: 0,
            bonus_drop: Vec::new(),
            shape: BlockShape::Cube,
            hidden: false,
            variant: None,
            fluid: None,
            falls: false,
            stage: None,
        }))
    }

    pub(crate) fn defs_mut(&mut self, id: BlockId) -> &mut BlockDef {
        &mut self.defs[id.0 as usize]
    }

    pub fn register(&mut self, def: BlockDef) -> BlockId {
        let id = BlockId(u16::try_from(self.defs.len()).expect("too many block types"));
        self.names.entry(def.name.clone()).or_insert(id);
        self.defs.push(def);
        id
    }

    pub fn get(&self, id: BlockId) -> &BlockDef {
        &self.defs[id.0 as usize]
    }

    /// Opaque blocks hide their neighbours' faces, block light and cast
    /// ambient occlusion. (Only full cubes can be opaque.)
    pub fn is_opaque(&self, id: BlockId) -> bool {
        self.defs
            .get(id.0 as usize)
            .is_some_and(|d| d.render == RenderKind::Opaque && d.shape.is_cube())
    }

    pub fn shape(&self, id: BlockId) -> &BlockShape {
        static CUBE: BlockShape = BlockShape::Cube;
        self.defs.get(id.0 as usize).map_or(&CUBE, |d| &d.shape)
    }

    /// Light given off, 0–15.
    pub fn light(&self, id: BlockId) -> u8 {
        self.defs.get(id.0 as usize).map_or(0, |d| d.light)
    }

    /// Solid blocks stop movement.
    pub fn is_solid(&self, id: BlockId) -> bool {
        self.defs.get(id.0 as usize).is_some_and(|d| d.solid)
    }

    pub fn is_breakable(&self, id: BlockId) -> bool {
        self.defs.get(id.0 as usize).is_some_and(|d| d.breakable)
    }

    pub fn is_liquid(&self, id: BlockId) -> bool {
        self.defs.get(id.0 as usize).is_some_and(|d| d.liquid)
    }

    pub fn render_kind(&self, id: BlockId) -> RenderKind {
        self.defs
            .get(id.0 as usize)
            .map_or(RenderKind::Invisible, |d| d.render)
    }

    /// Look up a block by its namespaced name.
    pub fn by_name(&self, name: &str) -> Option<BlockId> {
        self.names.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// Iterate over `(id, definition)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (BlockId, &BlockDef)> {
        self.defs
            .iter()
            .enumerate()
            .map(|(i, d)| (BlockId(i as u16), d))
    }
}

impl Default for BlockRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_ids_match_constants() {
        let reg = BlockRegistry::with_builtin_blocks();
        assert_eq!(reg.get(blocks::AIR).name, "core:air");
        assert_eq!(reg.get(blocks::STONE).name, "core:stone");
        assert_eq!(reg.get(blocks::GRASS).name, "core:grass");
        assert_eq!(reg.get(blocks::PLANKS).name, "core:planks");
        assert_eq!(reg.get(blocks::LAMP).name, "core:lamp");
        assert_eq!(reg.by_name("core:glass"), Some(blocks::GLASS));
        assert_eq!(reg.get(blocks::COAL_ORE).name, "core:coal_ore");
        assert_eq!(reg.get(blocks::FURNACE).name, "core:furnace");
        assert_eq!(reg.get(blocks::DIAMOND_BLOCK).name, "core:diamond_block");
        assert_eq!(reg.get(blocks::WOOL).name, "core:wool");
        assert_eq!(reg.get(blocks::LAVA).name, "core:lava");
        assert!(!reg.get(blocks::LEAVES).bonus_drop.is_empty(), "apples");
        assert_eq!(reg.get(blocks::TORCH).name, "core:torch");
        assert_eq!(
            reg.get(blocks::WALL_TORCH_NORTH).name,
            "core:wall_torch_north"
        );
    }

    #[test]
    fn air_is_not_opaque_and_stone_is() {
        let reg = BlockRegistry::with_builtin_blocks();
        assert!(!reg.is_opaque(blocks::AIR));
        assert!(reg.is_opaque(blocks::STONE));
        // Unknown ids are treated as not opaque instead of panicking.
        assert!(!reg.is_opaque(BlockId(9999)));
        // Leaves are solid to walk on but see-through.
        assert!(!reg.is_opaque(blocks::LEAVES));
        assert!(reg.is_solid(blocks::LEAVES));
        assert!(!reg.is_solid(blocks::AIR));
    }

    #[test]
    fn grass_has_different_top_and_side() {
        let reg = BlockRegistry::with_builtin_blocks();
        let tex = reg.get(blocks::GRASS).textures.as_ref().unwrap();
        assert_eq!(tex.for_face(Face::PosY), "grass_top");
        assert_eq!(tex.for_face(Face::NegY), "dirt");
        assert_eq!(tex.for_face(Face::PosX), "grass_side");
    }
}

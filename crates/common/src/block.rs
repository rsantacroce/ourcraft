//! Block types and the registry that describes them.
//!
//! A block in the world is just a [`BlockId`] (a `u16`). Everything else about
//! it (its name, whether it's solid, which textures it uses) lives once in the
//! [`BlockRegistry`]. Mods will add entries to this registry in M8.

/// Identifies a kind of block. Index into [`BlockRegistry`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
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
/// textures later without clashing.
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drops {
    /// The block itself.
    Itself,
    /// A different block (stone drops cobblestone).
    Block(BlockId),
    Nothing,
}

#[derive(Clone, Debug)]
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
}

impl BlockRegistry {
    /// A registry containing only air.
    pub fn new() -> Self {
        Self {
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
            }],
        }
    }

    /// A registry with all built-in blocks, matching the ids in [`blocks`].
    pub fn with_builtin_blocks() -> Self {
        use BlockTextures as T;
        let mut reg = Self::new();
        reg.register(BlockDef {
            drops: Drops::Block(blocks::COBBLESTONE),
            ..BlockDef::solid("core:stone", "Stone", T::all("stone"))
        });
        reg.register(BlockDef::solid("core:dirt", "Dirt", T::all("dirt")));
        reg.register(BlockDef {
            drops: Drops::Block(blocks::DIRT),
            ..BlockDef::solid(
                "core:grass",
                "Grass",
                T::top_bottom_side("grass_top", "dirt", "grass_side"),
            )
        });
        reg.register(BlockDef::solid("core:sand", "Sand", T::all("sand")));
        reg.register(BlockDef::solid(
            "core:log",
            "Log",
            T::top_bottom_side("log_top", "log_top", "log_side"),
        ));
        reg.register(BlockDef::cutout("core:leaves", "Leaves", T::all("leaves")));
        reg.register(BlockDef::solid("core:planks", "Planks", T::all("planks")));
        reg.register(BlockDef {
            drops: Drops::Block(blocks::DIRT),
            ..BlockDef::solid(
                "core:snow",
                "Snowy Grass",
                T::top_bottom_side("snow", "dirt", "snow_side"),
            )
        });
        reg.register(BlockDef::solid(
            "core:cactus",
            "Cactus",
            T::top_bottom_side("cactus_top", "cactus_top", "cactus_side"),
        ));
        reg.register(BlockDef {
            breakable: false,
            ..BlockDef::solid("core:bedrock", "Bedrock", T::all("bedrock"))
        });
        reg.register(BlockDef::solid("core:gravel", "Gravel", T::all("gravel")));
        reg.register(BlockDef::solid(
            "core:cobblestone",
            "Cobblestone",
            T::all("cobblestone"),
        ));
        reg.register(BlockDef::cutout("core:glass", "Glass", T::all("glass")));
        reg.register(BlockDef::solid("core:bricks", "Bricks", T::all("bricks")));
        reg.register(BlockDef {
            light: 15,
            ..BlockDef::solid("core:lamp", "Lamp", T::all("lamp"))
        });
        reg.register(BlockDef {
            render: RenderKind::Translucent,
            solid: false,
            breakable: false,
            liquid: true,
            ..BlockDef::solid("core:water", "Water", T::all("water"))
        });
        reg
    }

    pub fn register(&mut self, def: BlockDef) -> BlockId {
        let id = BlockId(u16::try_from(self.defs.len()).expect("too many block types"));
        self.defs.push(def);
        id
    }

    pub fn get(&self, id: BlockId) -> &BlockDef {
        &self.defs[id.0 as usize]
    }

    /// Opaque blocks hide their neighbours' faces and cast ambient occlusion.
    pub fn is_opaque(&self, id: BlockId) -> bool {
        self.defs
            .get(id.0 as usize)
            .is_some_and(|d| d.render == RenderKind::Opaque)
    }

    /// Solid blocks stop movement.
    pub fn is_solid(&self, id: BlockId) -> bool {
        self.defs.get(id.0 as usize).is_some_and(|d| d.solid)
    }

    /// The block added to your inventory when `id` is broken.
    pub fn drop_for(&self, id: BlockId) -> Option<BlockId> {
        match self.defs.get(id.0 as usize)?.drops {
            Drops::Itself => Some(id),
            Drops::Block(b) => Some(b),
            Drops::Nothing => None,
        }
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
        self.defs
            .iter()
            .position(|d| d.name == name)
            .map(|i| BlockId(i as u16))
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

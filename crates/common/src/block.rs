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

#[derive(Clone, Debug)]
pub struct BlockDef {
    /// Namespaced name, e.g. `"core:stone"`. Mods use their own namespace.
    pub name: String,
    /// Opaque blocks hide the faces of their neighbours.
    pub opaque: bool,
    /// `None` for air (nothing is drawn).
    pub textures: Option<BlockTextures>,
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
                opaque: false,
                textures: None,
            }],
        }
    }

    /// A registry with all built-in blocks, matching the ids in [`blocks`].
    pub fn with_builtin_blocks() -> Self {
        let mut reg = Self::new();
        let solid = |name: &str, textures: BlockTextures| BlockDef {
            name: name.to_owned(),
            opaque: true,
            textures: Some(textures),
        };
        reg.register(solid("core:stone", BlockTextures::all("stone")));
        reg.register(solid("core:dirt", BlockTextures::all("dirt")));
        reg.register(solid(
            "core:grass",
            BlockTextures::top_bottom_side("grass_top", "dirt", "grass_side"),
        ));
        reg.register(solid("core:sand", BlockTextures::all("sand")));
        reg.register(solid(
            "core:log",
            BlockTextures::top_bottom_side("log_top", "log_top", "log_side"),
        ));
        // Leaves are opaque for now; see-through leaves come with transparency later.
        reg.register(solid("core:leaves", BlockTextures::all("leaves")));
        reg.register(solid("core:planks", BlockTextures::all("planks")));
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

    pub fn is_opaque(&self, id: BlockId) -> bool {
        self.defs.get(id.0 as usize).is_some_and(|d| d.opaque)
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
    }

    #[test]
    fn air_is_not_opaque_and_stone_is() {
        let reg = BlockRegistry::with_builtin_blocks();
        assert!(!reg.is_opaque(blocks::AIR));
        assert!(reg.is_opaque(blocks::STONE));
        // Unknown ids are treated as not opaque instead of panicking.
        assert!(!reg.is_opaque(BlockId(9999)));
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

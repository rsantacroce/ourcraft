//! Blocks with shapes and ways of being placed: stairs, slabs, fences,
//! doors, trapdoors, ladders, signs, beds, chests and torches.
//!
//! A block id says everything about a block, so a stair facing east and
//! one facing west are different blocks. Each such *variant* remembers its
//! family and state in [`Variant`]. Only the family's first ("base")
//! variant shows up in the inventory; placing it picks the right variant
//! from where you click and which way you look, and every variant drops the
//! base one.
//!
//! Shapes are drawn for "north" (facing −z) and turned for the others.

use serde::{Deserialize, Serialize};

use crate::block::{
    BlockDef, BlockId, BlockRegistry, BlockShape, BlockTextures as T, Drops, RenderKind, ShapeBox,
    ToolKind,
};

/// The four horizontal directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dir4 {
    North,
    East,
    South,
    West,
}

impl Dir4 {
    pub const ALL: [Dir4; 4] = [Dir4::North, Dir4::East, Dir4::South, Dir4::West];

    pub fn vector(self) -> [i32; 3] {
        match self {
            Dir4::North => [0, 0, -1],
            Dir4::East => [1, 0, 0],
            Dir4::South => [0, 0, 1],
            Dir4::West => [-1, 0, 0],
        }
    }

    pub fn opposite(self) -> Dir4 {
        match self {
            Dir4::North => Dir4::South,
            Dir4::East => Dir4::West,
            Dir4::South => Dir4::North,
            Dir4::West => Dir4::East,
        }
    }

    fn turns(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Dir4::North => "north",
            Dir4::East => "east",
            Dir4::South => "south",
            Dir4::West => "west",
        }
    }

    /// The direction a horizontal block face points, if it's horizontal.
    pub fn from_normal(n: [i32; 3]) -> Option<Dir4> {
        Dir4::ALL.into_iter().find(|d| d.vector() == n)
    }

    /// Which way someone looking along `look` (x, y, z) mostly faces.
    pub fn from_look(look: [f32; 3]) -> Dir4 {
        if look[0].abs() > look[2].abs() {
            if look[0] > 0.0 {
                Dir4::East
            } else {
                Dir4::West
            }
        } else if look[2] > 0.0 {
            Dir4::South
        } else {
            Dir4::North
        }
    }
}

/// Turn a box a quarter turn clockwise (seen from above): north → east.
fn rotate(b: ShapeBox) -> ShapeBox {
    [16 - b[5], b[1], b[0], 16 - b[2], b[4], b[3]]
}

fn facing(boxes: &[ShapeBox], d: Dir4) -> Vec<ShapeBox> {
    boxes
        .iter()
        .map(|b| (0..d.turns()).fold(*b, |b, _| rotate(b)))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariantKind {
    Torch,
    WallTorch,
    Stairs,
    Slab,
    Door,
    Trapdoor,
    Ladder,
    Sign,
    WallSign,
    Bed,
    FenceGate,
    /// A standing banner (its wall version is `WallBanner`, the "upper"
    /// state, like signs).
    Banner,
    WallBanner,
    /// Hangs on a wall: paintings and item frames.
    WallHanging,
    // ---- Circuits and rails (see `circuits`); `open` means on. ----
    Wire,
    Lever,
    Button,
    PressurePlate,
    SparkLamp,
    Repeater,
    Piston,
    PistonHead,
    Rail,
    PoweredRail,
}

/// Which family a block belongs to and its state in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    pub kind: VariantKind,
    /// The base block's name (the one in the inventory).
    pub family: String,
    pub facing: Dir4,
    /// Doors, trapdoors and fence gates.
    pub open: bool,
    /// The upper half of a door, the head of a bed, a top slab.
    pub upper: bool,
}

/// What placing a block puts in the world: the block, and maybe a second
/// one next to it (a door's top half, a bed's head).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub block: BlockId,
    pub extra: Option<([i32; 3], BlockId)>,
}

impl BlockRegistry {
    fn variant(&self, b: BlockId) -> Option<&Variant> {
        self.get(b).variant.as_ref()
    }

    /// The variant of `family` with this state.
    pub fn find_variant(
        &self,
        family: &str,
        facing: Dir4,
        open: bool,
        upper: bool,
    ) -> Option<BlockId> {
        self.iter()
            .find(|(_, d)| {
                d.variant.as_ref().is_some_and(|v| {
                    v.family == family && v.facing == facing && v.open == open && v.upper == upper
                })
            })
            .map(|(id, _)| id)
    }

    /// What goes in the world when you place `block` against a face whose
    /// outward direction is `normal`, looking along `look`. `None` if it
    /// can't go there (torches on ceilings, say).
    pub fn placement(&self, block: BlockId, normal: [i32; 3], look: [f32; 3]) -> Option<Placement> {
        let single = |b| {
            Some(Placement {
                block: b,
                extra: None,
            })
        };
        let Some(v) = self.variant(block) else {
            return single(block);
        };
        let fam = v.family.as_str();
        let looking = Dir4::from_look(look);
        match v.kind {
            VariantKind::Torch | VariantKind::WallTorch => match Dir4::from_normal(normal) {
                Some(d) => single(self.find_variant("core:torch", d, false, true)?),
                None if normal == [0, 1, 0] => {
                    single(self.find_variant(fam, Dir4::North, false, false)?)
                }
                None => None,
            },
            VariantKind::Repeater | VariantKind::Piston => {
                single(self.find_variant(fam, looking, false, false)?)
            }
            // Rails start along the way you look (the server joins them up).
            VariantKind::Rail | VariantKind::PoweredRail => {
                let facing = match looking {
                    Dir4::North | Dir4::South => Dir4::North,
                    _ => Dir4::East,
                };
                single(self.find_variant(fam, facing, false, false)?)
            }
            VariantKind::Wire
            | VariantKind::Lever
            | VariantKind::Button
            | VariantKind::PressurePlate
            | VariantKind::SparkLamp
            | VariantKind::PistonHead => single(block),
            VariantKind::Stairs | VariantKind::Trapdoor | VariantKind::FenceGate => {
                single(self.find_variant(fam, looking, false, false)?)
            }
            VariantKind::Slab => {
                let top = normal == [0, -1, 0];
                single(self.find_variant(fam, Dir4::North, false, top)?)
            }
            VariantKind::Ladder => {
                single(self.find_variant(fam, Dir4::from_normal(normal)?, false, false)?)
            }
            VariantKind::WallHanging => {
                single(self.find_variant(fam, Dir4::from_normal(normal)?, false, false)?)
            }
            VariantKind::Sign
            | VariantKind::WallSign
            | VariantKind::Banner
            | VariantKind::WallBanner => match Dir4::from_normal(normal) {
                Some(d) => single(self.find_variant(fam, d, false, true)?),
                // A standing sign's writing faces whoever put it there.
                None if normal == [0, 1, 0] => {
                    single(self.find_variant(fam, looking.opposite(), false, false)?)
                }
                None => None,
            },
            VariantKind::Door => Some(Placement {
                block: self.find_variant(fam, looking, false, false)?,
                extra: Some(([0, 1, 0], self.find_variant(fam, looking, false, true)?)),
            }),
            VariantKind::Bed => Some(Placement {
                block: self.find_variant(fam, looking, false, false)?,
                extra: Some((
                    looking.vector(),
                    self.find_variant(fam, looking, false, true)?,
                )),
            }),
        }
    }

    /// Using (right-clicking) a door or trapdoor opens or closes it: the
    /// blocks to change, as offsets from the one clicked.
    pub fn toggled(&self, block: BlockId) -> Option<Vec<([i32; 3], BlockId)>> {
        let v = self.variant(block)?;
        let flip = |upper| self.find_variant(&v.family, v.facing, !v.open, upper);
        match v.kind {
            VariantKind::Trapdoor | VariantKind::FenceGate => Some(vec![([0, 0, 0], flip(false)?)]),
            VariantKind::Door => {
                let other = if v.upper { [0, -1, 0] } else { [0, 1, 0] };
                Some(vec![([0, 0, 0], flip(v.upper)?), (other, flip(!v.upper)?)])
            }
            _ => None,
        }
    }

    /// The other half of a two-block thing (door, bed), as an offset.
    pub fn other_half(&self, block: BlockId) -> Option<[i32; 3]> {
        let v = self.variant(block)?;
        match v.kind {
            VariantKind::Door => Some(if v.upper { [0, -1, 0] } else { [0, 1, 0] }),
            VariantKind::Bed => {
                let f = v.facing.vector();
                Some(if v.upper { [-f[0], 0, -f[2]] } else { f })
            }
            _ => None,
        }
    }

    /// The block something hangs on (torches, ladders, signs), as an
    /// offset: when that block goes, this one falls off.
    pub fn attached_to(&self, block: BlockId) -> Option<[i32; 3]> {
        // Plants and cakes stand on the block below.
        if self.sits_on_ground(block) {
            return Some([0, -1, 0]);
        }
        let v = self.variant(block)?;
        let back = || {
            let f = v.facing.vector();
            [-f[0], 0, -f[2]]
        };
        match v.kind {
            VariantKind::Torch
            | VariantKind::Sign
            | VariantKind::Banner
            | VariantKind::Wire
            | VariantKind::Lever
            | VariantKind::Button
            | VariantKind::PressurePlate
            | VariantKind::Repeater
            | VariantKind::Rail
            | VariantKind::PoweredRail => Some([0, -1, 0]),
            VariantKind::WallTorch
            | VariantKind::Ladder
            | VariantKind::WallSign
            | VariantKind::WallBanner
            | VariantKind::WallHanging => Some(back()),
            _ => None,
        }
    }

    pub fn variant_kind(&self, block: BlockId) -> Option<VariantKind> {
        self.variant(block).map(|v| v.kind)
    }

    /// Can you climb it (ladders)?
    pub fn is_climbable(&self, block: BlockId) -> bool {
        self.variant_kind(block) == Some(VariantKind::Ladder)
            || self.get(block).name == "core:scaffolding"
    }

    /// The boxes a block is made of right here, in block units. Fences reach
    /// out to the fences and solid blocks next to them; `neighbour` gives
    /// the block at an offset.
    pub fn boxes_at(
        &self,
        block: BlockId,
        neighbour: impl Fn([i32; 3]) -> BlockId,
    ) -> Vec<([f32; 3], [f32; 3])> {
        let shape = self.shape(block);
        let wall = *shape == BlockShape::Wall;
        let pane = *shape == BlockShape::Pane;
        if *shape != BlockShape::Fence && !wall && !pane {
            return shape.boxes();
        }
        let joins = |n: BlockId| {
            if pane {
                return *self.shape(n) == BlockShape::Pane || self.is_opaque(n);
            }
            let gate = self.variant_kind(n) == Some(VariantKind::FenceGate);
            matches!(self.shape(n), BlockShape::Fence | BlockShape::Wall)
                || gate
                || self.is_opaque(n)
        };
        let mut boxes: Vec<ShapeBox> = if wall {
            vec![[4, 0, 4, 12, 16, 12]]
        } else if pane {
            vec![[7, 0, 7, 9, 16, 9]]
        } else {
            vec![[6, 0, 6, 10, 16, 10]]
        };
        for d in Dir4::ALL {
            if !joins(neighbour(d.vector())) {
                continue;
            }
            if pane {
                // A thin sheet out to the neighbour.
                boxes.extend(facing(&[[7, 0, 0, 9, 16, 7]], d));
            } else if wall {
                // A low wall towards the neighbour.
                boxes.extend(facing(&[[5, 0, 0, 11, 14, 4]], d));
            } else {
                // Two rails towards the neighbour.
                boxes.extend(facing(&[[7, 6, 0, 9, 9, 6], [7, 12, 0, 9, 15, 6]], d));
            }
        }
        BlockShape::Boxes(boxes).boxes()
    }
}

/// One state of a family: (facing, open, upper, boxes).
pub(crate) type State = (Dir4, bool, bool, Vec<ShapeBox>);

/// Register a family of blocks; the first state is the base (the one in
/// the inventory, which every other state drops).
pub(crate) fn register_family(
    reg: &mut BlockRegistry,
    kind: VariantKind,
    base: &str,
    display: &str,
    def: &dyn Fn(&str) -> BlockDef,
    states: Vec<State>,
    no_drop_upper: bool,
) {
    let mut base_id = None;
    for (i, (dir, open, upper, boxes)) in states.into_iter().enumerate() {
        let name = if i == 0 {
            base.to_owned()
        } else {
            let mut n = format!("{base}_{}", dir.name());
            if open {
                n.push_str("_open");
            }
            if upper {
                n.push_str("_upper");
            }
            n
        };
        let mut d = def(&name);
        d.display_name = display.to_owned();
        d.shape = BlockShape::Boxes(boxes);
        d.hidden = i > 0;
        d.variant = Some(Variant {
            kind,
            family: base.to_owned(),
            facing: dir,
            open,
            upper,
        });
        if let Some(b) = base_id {
            d.drops = if upper && no_drop_upper {
                Drops::Nothing
            } else {
                Drops::Block(b)
            };
        }
        let id = reg.register(d);
        base_id.get_or_insert(id);
    }
}

/// The same boxes facing each of the four ways.
pub(crate) fn all_dirs(boxes: &[ShapeBox], open: bool, upper: bool) -> Vec<State> {
    Dir4::ALL
        .into_iter()
        .map(|d| (d, open, upper, facing(boxes, d)))
        .collect()
}

fn cutout(name: &str, display: &str, tex: T) -> BlockDef {
    BlockDef {
        render: RenderKind::Cutout,
        ..BlockDef::solid(name, display, tex)
    }
}

fn wooden(d: BlockDef) -> BlockDef {
    d.mined(2.0, Some(ToolKind::Axe), 0)
}

/// Stairs and a slab of a material: `core:{key}_stairs`, `core:{key}_slab`.
pub fn register_stairs_and_slab(
    reg: &mut BlockRegistry,
    key: &str,
    display: &str,
    tex: &str,
    tool: ToolKind,
    tier: u8,
) {
    let def = |name: &str| cutout(name, "", T::all(tex)).mined(2.0, Some(tool), tier);
    register_family(
        reg,
        VariantKind::Stairs,
        &format!("core:{key}_stairs"),
        &format!("{display} Stairs"),
        &def,
        all_dirs(&[[0, 0, 0, 16, 8, 16], [0, 8, 0, 16, 16, 8]], false, false),
        false,
    );
    register_family(
        reg,
        VariantKind::Slab,
        &format!("core:{key}_slab"),
        &format!("{display} Slab"),
        &def,
        vec![
            (Dir4::North, false, false, vec![[0, 0, 0, 16, 8, 16]]),
            (Dir4::North, false, true, vec![[0, 8, 0, 16, 16, 16]]),
        ],
        false,
    );
}

/// A fence whose rails reach out to its neighbours.
pub fn register_fence(reg: &mut BlockRegistry, name: &str, display: &str, tex: &str) {
    reg.register(wooden(BlockDef {
        shape: BlockShape::Fence,
        ..cutout(name, display, T::all(tex))
    }));
}

/// A door, two blocks tall: `planks` on top and bottom, `lower` and
/// `upper` on the sides.
pub fn register_door(
    reg: &mut BlockRegistry,
    family: &str,
    display: &str,
    planks: &str,
    lower: &str,
    upper_tex: &str,
) {
    let door = |name: &str| {
        let side = if name.ends_with("_upper") {
            upper_tex
        } else {
            lower
        };
        wooden(cutout(name, "", T::top_bottom_side(planks, planks, side)))
    };
    let closed: [ShapeBox; 1] = [[0, 0, 0, 16, 16, 3]];
    let opened: [ShapeBox; 1] = [[0, 0, 0, 3, 16, 16]];
    let mut states = Vec::new();
    for upper in [false, true] {
        states.extend(all_dirs(&closed, false, upper));
        states.extend(all_dirs(&opened, true, upper));
    }
    register_family(reg, VariantKind::Door, family, display, &door, states, true);
}

/// A trapdoor that opens upwards.
pub fn register_trapdoor(reg: &mut BlockRegistry, family: &str, display: &str, tex: &str) {
    let trapdoor = |name: &str| wooden(cutout(name, "", T::all(tex)));
    let mut states = all_dirs(&[[0, 0, 0, 16, 3, 16]], false, false);
    states.extend(all_dirs(&[[0, 0, 0, 16, 16, 3]], true, false));
    register_family(
        reg,
        VariantKind::Trapdoor,
        family,
        display,
        &trapdoor,
        states,
        false,
    );
}

/// Fence gates: a fence you can open (right click). Closed, they span the
/// way they face sideways; open, the two halves swing back against the
/// posts.
pub fn register_fence_gates(reg: &mut BlockRegistry) {
    register_fence_gate(reg, "core:fence_gate", "Fence Gate", "planks");
}

/// A fence gate called `family`, made of `tex`.
pub fn register_fence_gate(reg: &mut BlockRegistry, family: &str, display: &str, tex: &str) {
    let closed: [ShapeBox; 4] = [
        [0, 0, 7, 2, 16, 9],
        [14, 0, 7, 16, 16, 9],
        [2, 6, 7, 14, 9, 9],
        [2, 12, 7, 14, 15, 9],
    ];
    let opened: [ShapeBox; 4] = [
        [0, 0, 7, 2, 16, 9],
        [14, 0, 7, 16, 16, 9],
        [0, 6, 1, 2, 15, 7],
        [14, 6, 1, 16, 15, 7],
    ];
    let mut base = None;
    for open in [false, true] {
        for d in Dir4::ALL {
            let first = base.is_none();
            let name = if first {
                family.to_owned()
            } else if open {
                format!("{family}_{}_open", d.name())
            } else {
                format!("{family}_{}", d.name())
            };
            let boxes = if open { &opened } else { &closed };
            let mut def = BlockDef {
                render: RenderKind::Cutout,
                solid: !open,
                ..BlockDef::solid(&name, display, T::all(tex)).mined(2.0, Some(ToolKind::Axe), 0)
            };
            def.shape = BlockShape::Boxes(facing(boxes, d));
            def.hidden = !first;
            def.variant = Some(Variant {
                kind: VariantKind::FenceGate,
                family: family.into(),
                facing: d,
                open,
                upper: false,
            });
            if let Some(b) = base {
                def.drops = Drops::Block(b);
            }
            let id = reg.register(def);
            base.get_or_insert(id);
        }
    }
}

/// A bed family: the foot where you place it, the head beyond. Its
/// textures are `bed_head`, `bed_foot` and `bed_side` plus `texture_suffix`
/// (coloured beds use `"_blue"` and so on).
pub fn register_bed(reg: &mut BlockRegistry, family: &str, display: &str, texture_suffix: &str) {
    let mut base = None;
    for upper in [false, true] {
        for d in Dir4::ALL {
            let first = base.is_none();
            let mut name = if first {
                family.to_owned()
            } else {
                format!("{family}_{}", d.name())
            };
            if upper {
                name.push_str("_upper");
            }
            let top = if upper { "bed_head" } else { "bed_foot" };
            let tex = |t: &str| format!("{t}{texture_suffix}");
            let mut def = BlockDef {
                render: RenderKind::Cutout,
                shape: BlockShape::Boxes(facing(&[[0, 0, 0, 16, 9, 16]], d)),
                hidden: !first,
                variant: Some(Variant {
                    kind: VariantKind::Bed,
                    family: family.to_owned(),
                    facing: d,
                    open: false,
                    upper,
                }),
                ..BlockDef::solid(
                    &name,
                    display,
                    T::top_bottom_side(&tex(top), "planks", &tex("bed_side")),
                )
                .mined(0.6, None, 0)
            };
            if let Some(b) = base {
                def.drops = if upper {
                    Drops::Nothing
                } else {
                    Drops::Block(b)
                };
            }
            let id = reg.register(def);
            base.get_or_insert(id);
        }
    }
}

/// Add the shaped blocks (milestones 13 and 14) to the built-in list.
pub fn register_shaped_blocks(reg: &mut BlockRegistry) {
    let wood = |d: BlockDef| d.mined(2.0, Some(ToolKind::Axe), 0);
    let base_def = |name: &str, display: &str, tex: T| BlockDef {
        render: RenderKind::Cutout,
        ..BlockDef::solid(name, display, tex)
    };

    // ---- Torches (milestone 13): kept as the first family so their ids
    // match `blocks::TORCH` and friends. ----
    let torch_def = |name: &str| BlockDef {
        render: RenderKind::Cutout,
        solid: false,
        light: 14,
        ..BlockDef::solid(
            name,
            "Torch",
            T::top_bottom_side("torch_top", "torch", "torch"),
        )
        .mined(0.0, None, 0)
    };
    register_family(
        reg,
        VariantKind::Torch,
        "core:torch",
        "Torch",
        &torch_def,
        vec![(Dir4::North, false, false, vec![[7, 0, 7, 9, 10, 9]])],
        false,
    );
    // Wall torches belong to the torch family (they drop torches).
    let torch_id = reg.by_name("core:torch").expect("just added");
    for d in [Dir4::East, Dir4::West, Dir4::South, Dir4::North] {
        let mut def = torch_def(&format!("core:wall_torch_{}", d.name()));
        def.shape = BlockShape::Boxes(facing(&[[7, 3, 13, 9, 13, 15], [7, 4, 15, 9, 6, 16]], d));
        def.hidden = true;
        def.drops = Drops::Block(torch_id);
        def.variant = Some(Variant {
            kind: VariantKind::WallTorch,
            family: "core:torch".into(),
            facing: d,
            open: false,
            upper: true,
        });
        reg.register(def);
    }

    // ---- Stairs and slabs in four materials. ----
    for (mat, display, tex, tool, tier) in [
        ("planks", "Wooden", "planks", ToolKind::Axe, 0),
        (
            "cobblestone",
            "Cobblestone",
            "cobblestone",
            ToolKind::Pickaxe,
            1,
        ),
        ("stone", "Stone", "stone", ToolKind::Pickaxe, 1),
        ("bricks", "Brick", "bricks", ToolKind::Pickaxe, 1),
    ] {
        register_stairs_and_slab(reg, mat, display, tex, tool, tier);
    }

    // ---- Fence: its rails depend on its neighbours. ----
    register_fence(reg, "core:fence", "Fence", "planks");

    // ---- Door (two blocks tall) and trapdoor. ----
    register_door(
        reg,
        "core:door",
        "Door",
        "planks",
        "door_lower",
        "door_upper",
    );
    register_trapdoor(reg, "core:trapdoor", "Trapdoor", "trapdoor");

    // ---- Ladder (climbable; hangs on the wall behind it). ----
    let ladder = |name: &str| BlockDef {
        solid: false,
        ..base_def(name, "", T::all("ladder")).mined(0.6, Some(ToolKind::Axe), 0)
    };
    register_family(
        reg,
        VariantKind::Ladder,
        "core:ladder",
        "Ladder",
        &ladder,
        all_dirs(&[[0, 0, 15, 16, 16, 16]], false, false),
        false,
    );

    // ---- Signs: standing (on a post) and on walls. ----
    let sign = |name: &str| BlockDef {
        solid: false,
        ..wood(base_def(name, "", T::all("planks")))
    };
    let mut states = all_dirs(&[[7, 0, 7, 9, 9, 9], [0, 9, 7, 16, 16, 9]], false, false);
    states.extend(all_dirs(&[[0, 4, 14, 16, 12, 16]], false, true));
    register_family(
        reg,
        VariantKind::Sign,
        "core:sign",
        "Sign",
        &sign,
        states,
        false,
    );
    // Wall signs are the "upper" half of the sign family.
    for id in 0..reg.len() as u16 {
        let d = reg.defs_mut(BlockId(id));
        if let Some(v) = d.variant.as_mut()
            && v.family == "core:sign"
            && v.upper
        {
            v.kind = VariantKind::WallSign;
        }
    }

    // ---- Bed: foot where you place it, head beyond. ----
    register_bed(reg, "core:bed", "Bed", "");

    // ---- Chest (holds 27 stacks; see containers). ----
    reg.register(wood(BlockDef {
        shape: BlockShape::Boxes(vec![[1, 0, 1, 15, 14, 15]]),
        ..base_def(
            "core:chest",
            "Chest",
            T::top_bottom_side("chest_top", "chest_top", "chest_side"),
        )
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks;

    fn reg() -> BlockRegistry {
        BlockRegistry::with_builtin_blocks()
    }

    #[test]
    fn rotating_four_times_comes_back() {
        let b = [0, 0, 0, 16, 8, 3];
        let mut r = b;
        for _ in 0..4 {
            r = rotate(r);
        }
        assert_eq!(r, b);
        // North to east: a box along the north edge goes to the east edge.
        assert_eq!(rotate([0, 0, 0, 16, 16, 3]), [13, 0, 0, 16, 16, 16]);
    }

    #[test]
    fn torches_keep_their_ids_and_go_on_walls() {
        let r = reg();
        assert_eq!(r.get(blocks::TORCH).name, "core:torch");
        let p = r
            .placement(blocks::TORCH, [1, 0, 0], [0.0, 0.0, -1.0])
            .unwrap();
        assert_eq!(p.block, blocks::WALL_TORCH_EAST);
        assert_eq!(r.attached_to(p.block), Some([-1, 0, 0]));
        assert!(r.placement(blocks::TORCH, [0, -1, 0], [0.0; 3]).is_none());
        assert_eq!(
            r.get(blocks::WALL_TORCH_NORTH).drops,
            Drops::Block(blocks::TORCH)
        );
    }

    #[test]
    fn stairs_face_where_you_look() {
        let r = reg();
        let stairs = r.by_name("core:planks_stairs").unwrap();
        let p = r.placement(stairs, [0, 1, 0], [1.0, -0.3, 0.2]).unwrap();
        assert_eq!(r.get(p.block).name, "core:planks_stairs_east");
        // Every variant drops the base stairs.
        assert_eq!(r.get(p.block).drops, Drops::Block(stairs));
        assert!(r.get(p.block).hidden && !r.get(stairs).hidden);
    }

    #[test]
    fn doors_are_two_tall_and_open_together() {
        let r = reg();
        let door = r.by_name("core:door").unwrap();
        let p = r.placement(door, [0, 1, 0], [0.0, 0.0, 1.0]).unwrap();
        let (offset, top) = p.extra.unwrap();
        assert_eq!(offset, [0, 1, 0]);
        assert_eq!(r.other_half(top), Some([0, -1, 0]));
        let changes = r.toggled(top).unwrap();
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().all(|(_, b)| r.variant(*b).unwrap().open));
        // Only the bottom half drops a door.
        assert_eq!(r.get(top).drops, Drops::Nothing);
        assert!(r.get(p.block).solid && !r.is_opaque(p.block));
    }

    #[test]
    fn fence_gates_open_and_join_fences() {
        let r = reg();
        let gate = r.by_name("core:fence_gate").unwrap();
        let placed = r.placement(gate, [0, 1, 0], [1.0, 0.0, 0.0]).unwrap().block;
        assert!(r.is_solid(placed));
        let toggled = r.toggled(placed).unwrap();
        let open = toggled[0].1;
        assert!(!r.is_solid(open), "you walk through an open gate");
        assert_eq!(r.toggled(open).unwrap()[0].1, placed);
        assert_eq!(r.get(open).drops, Drops::Block(gate));
        // A fence next to a gate reaches out to it.
        let fence = r.by_name("core:fence").unwrap();
        let alone = r.boxes_at(fence, |_| BlockId::AIR).len();
        let joined = r.boxes_at(
            fence,
            |o| if o == [1, 0, 0] { placed } else { BlockId::AIR },
        );
        assert!(joined.len() > alone);
    }

    #[test]
    fn beds_put_their_head_beyond_the_foot() {
        let r = reg();
        let bed = r.by_name("core:bed").unwrap();
        let p = r.placement(bed, [0, 1, 0], [-1.0, 0.0, 0.0]).unwrap();
        let (offset, head) = p.extra.unwrap();
        assert_eq!(offset, [-1, 0, 0]);
        assert_eq!(r.other_half(head), Some([1, 0, 0]));
        assert_eq!(r.other_half(p.block), Some([-1, 0, 0]));
    }

    #[test]
    fn slabs_go_on_top_when_placed_under_something() {
        let r = reg();
        let slab = r.by_name("core:stone_slab").unwrap();
        let low = r.placement(slab, [0, 1, 0], [0.0; 3]).unwrap().block;
        let high = r.placement(slab, [0, -1, 0], [0.0; 3]).unwrap().block;
        assert_eq!(low, slab);
        assert_eq!(r.get(high).name, "core:stone_slab_north_upper");
    }

    #[test]
    fn fences_join_up() {
        let r = reg();
        let fence = r.by_name("core:fence").unwrap();
        let alone = r.boxes_at(fence, |_| BlockId::AIR);
        assert_eq!(alone.len(), 1, "just the post");
        let joined = r.boxes_at(fence, |o| if o == [1, 0, 0] { fence } else { BlockId::AIR });
        assert_eq!(joined.len(), 3, "post and two rails");
    }

    #[test]
    fn signs_and_ladders() {
        let r = reg();
        let sign = r.by_name("core:sign").unwrap();
        let wall = r.placement(sign, [0, 0, 1], [0.0; 3]).unwrap().block;
        assert_eq!(r.variant_kind(wall), Some(VariantKind::WallSign));
        assert_eq!(r.attached_to(wall), Some([0, 0, -1]));
        let ladder = r.by_name("core:ladder").unwrap();
        let on_wall = r.placement(ladder, [-1, 0, 0], [0.0; 3]).unwrap().block;
        assert!(r.is_climbable(on_wall));
        assert!(r.placement(ladder, [0, 1, 0], [0.0; 3]).is_none());
    }
}

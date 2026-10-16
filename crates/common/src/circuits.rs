//! Circuits and transport (milestone 22).
//!
//! **Spark** is our own kind of power: spark ore deep underground gives
//! spark dust, which you lay on the ground as wire. Levers, buttons,
//! pressure plates and spark blocks power the wire next to them; the power
//! fades one step per block of wire (up to 15), and repeaters give it a
//! fresh start. Powered wire lights spark lamps, opens doors, trapdoors
//! and gates, pushes pistons and plays note blocks.
//!
//! **Rails** carry mine carts. Rails join up with the rails next to them
//! (straight or round a corner), and powered rails speed carts up when
//! they have power (and brake them when they don't).
//!
//! Every state is its own block (a variant), like doors: `open` means on,
//! pressed, powered or (pistons) pushed out.

use crate::block::{
    BlockDef, BlockId, BlockRegistry, BlockTextures as T, Drops, RenderKind, ShapeBox, ToolKind,
};
use crate::shapes::{Dir4, VariantKind, all_dirs, register_family};

/// Power fades one step per block of wire, from this.
pub const MAX_POWER: u8 = 15;

/// Add the circuit and rail blocks (milestone 22).
pub fn register_circuits(reg: &mut BlockRegistry) {
    let cutout = |name: &str, tex: T| BlockDef {
        render: RenderKind::Cutout,
        ..BlockDef::solid(name, "", tex)
    };
    let flat = |states: &[(bool, ShapeBox)]| {
        states
            .iter()
            .map(|(on, b)| (Dir4::North, *on, false, vec![*b]))
            .collect::<Vec<_>>()
    };
    // ---- Spark ore and spark blocks. ----
    reg.register(BlockDef {
        light: 4,
        drops: Drops::Item("core:spark_dust".into(), 4),
        ..BlockDef::solid("core:spark_ore", "Spark Ore", T::all("spark_ore")).mined(
            3.0,
            Some(ToolKind::Pickaxe),
            3,
        )
    });
    reg.register(BlockDef {
        light: 8,
        ..BlockDef::solid("core:spark_block", "Block of Spark", T::all("spark_block")).mined(
            2.0,
            Some(ToolKind::Pickaxe),
            1,
        )
    });
    // ---- Wire (spark dust on the ground): off and powered. ----
    let wire = |name: &str| BlockDef {
        solid: false,
        drops: Drops::Item("core:spark_dust".into(), 1),
        light: if name.ends_with("_open") { 6 } else { 0 },
        ..cutout(
            name,
            T::all(if name.ends_with("_open") {
                "spark_wire_on"
            } else {
                "spark_wire"
            }),
        )
        .mined(0.0, None, 0)
    };
    register_family(
        reg,
        VariantKind::Wire,
        "core:spark_wire",
        "Spark Wire",
        &wire,
        flat(&[(false, [0, 0, 0, 16, 1, 16]), (true, [0, 0, 0, 16, 1, 16])]),
        false,
    );
    // ---- Lever, button and pressure plate (on the ground). ----
    let lever = |name: &str| {
        cutout(
            name,
            T::top_bottom_side(
                if name.ends_with("_open") {
                    "lever_on"
                } else {
                    "lever"
                },
                "cobblestone",
                "cobblestone",
            ),
        )
        .mined(0.3, None, 0)
    };
    register_family(
        reg,
        VariantKind::Lever,
        "core:lever",
        "Lever",
        &lever,
        vec![
            (
                Dir4::North,
                false,
                false,
                vec![[5, 0, 4, 11, 3, 12], [7, 3, 9, 9, 10, 11]],
            ),
            (
                Dir4::North,
                true,
                false,
                vec![[5, 0, 4, 11, 3, 12], [7, 3, 5, 9, 10, 7]],
            ),
        ],
        false,
    );
    let stone = |name: &str| cutout(name, T::all("stone")).mined(0.3, Some(ToolKind::Pickaxe), 0);
    register_family(
        reg,
        VariantKind::Button,
        "core:button",
        "Button",
        &stone,
        flat(&[(false, [5, 0, 6, 11, 2, 10]), (true, [5, 0, 6, 11, 1, 10])]),
        false,
    );
    let plate = |name: &str| BlockDef {
        solid: false,
        ..stone(name)
    };
    register_family(
        reg,
        VariantKind::PressurePlate,
        "core:pressure_plate",
        "Pressure Plate",
        &plate,
        flat(&[(false, [1, 0, 1, 15, 2, 15]), (true, [1, 0, 1, 15, 1, 15])]),
        false,
    );
    // ---- Things that react: a spark lamp, a repeater, pistons. ----
    let lamp = |name: &str| BlockDef {
        light: if name.ends_with("_open") { 15 } else { 0 },
        ..BlockDef::solid(
            name,
            "",
            T::all(if name.ends_with("_open") {
                "spark_lamp_on"
            } else {
                "spark_lamp"
            }),
        )
        .mined(0.3, None, 0)
    };
    register_family(
        reg,
        VariantKind::SparkLamp,
        "core:spark_lamp",
        "Spark Lamp",
        &lamp,
        flat(&[
            (false, [0, 0, 0, 16, 16, 16]),
            (true, [0, 0, 0, 16, 16, 16]),
        ]),
        false,
    );
    let repeater = |name: &str| {
        cutout(
            name,
            T::top_bottom_side(
                if name.contains("_open") {
                    "repeater_on"
                } else {
                    "repeater"
                },
                "stone",
                "stone",
            ),
        )
        .mined(0.0, None, 0)
    };
    let mut states = all_dirs(&[[0, 0, 0, 16, 2, 16], [7, 2, 1, 9, 7, 3]], false, false);
    states.extend(all_dirs(
        &[[0, 0, 0, 16, 2, 16], [7, 2, 1, 9, 7, 3]],
        true,
        false,
    ));
    register_family(
        reg,
        VariantKind::Repeater,
        "core:repeater",
        "Repeater",
        &repeater,
        states,
        false,
    );
    for (family, display, head) in [
        ("core:piston", "Piston", "core:piston_head"),
        (
            "core:sticky_piston",
            "Sticky Piston",
            "core:sticky_piston_head",
        ),
    ] {
        let face = if family.contains("sticky") {
            "sticky_piston_front"
        } else {
            "piston_front"
        };
        let def = |name: &str| {
            cutout(
                name,
                T::top_bottom_side("piston_side", "piston_side", "piston_side"),
            )
            .mined(1.0, Some(ToolKind::Pickaxe), 0)
        };
        let mut states = all_dirs(&[[0, 0, 0, 16, 16, 16]], false, false);
        states.extend(all_dirs(&[[0, 0, 4, 16, 16, 16]], true, false));
        // Facing up and down: the "upper" states (north = up, south =
        // down; see `piston_push`).
        states.extend([
            (Dir4::North, false, true, vec![[0, 0, 0, 16, 16, 16]]),
            (Dir4::South, false, true, vec![[0, 0, 0, 16, 16, 16]]),
            (Dir4::North, true, true, vec![[0, 0, 0, 16, 12, 16]]),
            (Dir4::South, true, true, vec![[0, 4, 0, 16, 16, 16]]),
        ]);
        register_family(
            reg,
            VariantKind::Piston,
            family,
            display,
            &def,
            states,
            false,
        );
        // The head that comes out (it drops nothing; the piston does).
        let head_def = |name: &str| BlockDef {
            drops: Drops::Nothing,
            ..cutout(name, T::all(face)).mined(1.0, Some(ToolKind::Pickaxe), 0)
        };
        register_family(
            reg,
            VariantKind::PistonHead,
            head,
            display,
            &head_def,
            {
                let mut heads =
                    all_dirs(&[[0, 0, 0, 16, 16, 4], [6, 6, 4, 10, 10, 16]], false, false);
                heads.extend([
                    (
                        Dir4::North,
                        false,
                        true,
                        vec![[0, 12, 0, 16, 16, 16], [6, 0, 6, 10, 12, 10]],
                    ),
                    (
                        Dir4::South,
                        false,
                        true,
                        vec![[0, 0, 0, 16, 4, 16], [6, 4, 6, 10, 16, 10]],
                    ),
                ]);
                heads
            },
            false,
        );
    }
    // A dispenser: throws out (or shoots) what's inside when powered.
    let dispenser = |name: &str| {
        cutout(
            name,
            T::top_bottom_side("dispenser_top", "dispenser_top", "dispenser_front"),
        )
        .mined(3.5, Some(ToolKind::Pickaxe), 0)
    };
    let mut states = all_dirs(&[[0, 0, 0, 16, 16, 16]], false, false);
    states.extend(all_dirs(&[[0, 0, 0, 16, 16, 16]], true, false));
    register_family(
        reg,
        VariantKind::Dispenser,
        "core:dispenser",
        "Dispenser",
        &dispenser,
        states,
        false,
    );
    reg.register(
        BlockDef::solid("core:note_block", "Note Block", T::all("note_block")).mined(
            0.8,
            Some(ToolKind::Axe),
            0,
        ),
    );

    // ---- Rails: straight north–south (the first), east–west, and the
    // four corners ("upper", from `facing` round to the next way
    // clockwise), and four slopes ("open", going up towards `facing`).
    // Powered rails don't turn corners, so for them "upper" is a slope.
    let rail = |name: &str| {
        let tex = if name.ends_with("_open") {
            if name.contains("_east") || name.contains("_west") {
                "rail_ew".to_owned()
            } else {
                "rail_ns".to_owned()
            }
        } else if name.ends_with("_upper") {
            let corner = name
                .trim_end_matches("_upper")
                .rsplit('_')
                .next()
                .unwrap_or("north");
            format!("rail_corner_{corner}")
        } else if name.ends_with("_east") {
            "rail_ew".to_owned()
        } else {
            "rail_ns".to_owned()
        };
        BlockDef {
            solid: false,
            ..cutout(name, T::all(&tex)).mined(0.7, Some(ToolKind::Pickaxe), 0)
        }
    };
    let flat_rail: ShapeBox = [0, 0, 0, 16, 1, 16];
    let mut states = vec![
        (Dir4::North, false, false, vec![flat_rail]),
        (Dir4::East, false, false, vec![flat_rail]),
    ];
    for d in Dir4::ALL {
        states.push((d, false, true, vec![flat_rail]));
    }
    // Slopes are drawn as a ramp; this box is for clicking on them.
    let slope: ShapeBox = [0, 0, 0, 16, 8, 16];
    for d in Dir4::ALL {
        states.push((d, true, false, vec![slope]));
    }
    register_family(
        reg,
        VariantKind::Rail,
        "core:rail",
        "Rail",
        &rail,
        states,
        false,
    );
    let powered = |name: &str| {
        let on = name.contains("_open");
        let ew = name.contains("_east") || name.contains("_west");
        let tex = match (ew, on) {
            (false, false) => "powered_rail_ns",
            (true, false) => "powered_rail_ew",
            (false, true) => "powered_rail_ns_on",
            (true, true) => "powered_rail_ew_on",
        };
        BlockDef {
            solid: false,
            light: if on { 5 } else { 0 },
            ..cutout(name, T::all(tex)).mined(0.7, Some(ToolKind::Pickaxe), 0)
        }
    };
    register_family(
        reg,
        VariantKind::PoweredRail,
        "core:powered_rail",
        "Powered Rail",
        &powered,
        vec![
            (Dir4::North, false, false, vec![flat_rail]),
            (Dir4::East, false, false, vec![flat_rail]),
            (Dir4::North, true, false, vec![flat_rail]),
            (Dir4::East, true, false, vec![flat_rail]),
        ]
        .into_iter()
        .chain(
            Dir4::ALL
                .into_iter()
                .flat_map(|d| [(d, false, true, vec![slope]), (d, true, true, vec![slope])]),
        )
        .collect(),
        false,
    );
}

impl BlockRegistry {
    /// The same block switched on (`true`) or off: a powered wire, a
    /// pressed button, a lit lamp, a pushed-out piston, an open door.
    pub fn powered(&self, b: BlockId, on: bool) -> Option<BlockId> {
        let v = self.get(b).variant.as_ref()?;
        if v.open == on {
            return Some(b);
        }
        self.find_variant(&v.family, v.facing, on, v.upper)
    }

    pub fn is_on(&self, b: BlockId) -> bool {
        self.get(b).variant.as_ref().is_some_and(|v| v.open)
    }

    /// How much power a block gives to the blocks around it all by itself
    /// (levers, buttons and plates when on, spark blocks always).
    pub fn source_power(&self, b: BlockId) -> u8 {
        match self.variant_kind(b) {
            Some(VariantKind::Lever | VariantKind::Button | VariantKind::PressurePlate)
                if self.is_on(b) =>
            {
                MAX_POWER
            }
            _ if self.get(b).name == "core:spark_block" => MAX_POWER,
            _ => 0,
        }
    }

    pub fn is_wire(&self, b: BlockId) -> bool {
        self.variant_kind(b) == Some(VariantKind::Wire)
    }

    /// Does power change this block (lamps, doors, pistons, note blocks…)?
    pub fn reacts_to_power(&self, b: BlockId) -> bool {
        matches!(
            self.variant_kind(b),
            Some(
                VariantKind::SparkLamp
                    | VariantKind::Door
                    | VariantKind::Trapdoor
                    | VariantKind::FenceGate
                    | VariantKind::Piston
                    | VariantKind::PoweredRail
                    | VariantKind::Dispenser
            )
        ) || self.get(b).name == "core:note_block"
    }

    pub fn is_rail(&self, b: BlockId) -> bool {
        matches!(
            self.variant_kind(b),
            Some(VariantKind::Rail | VariantKind::PoweredRail)
        )
    }

    /// The two ways a rail goes (straight or round a corner).
    pub fn rail_ends(&self, b: BlockId) -> Option<[Dir4; 2]> {
        let v = self.get(b).variant.as_ref()?;
        if !self.is_rail(b) {
            return None;
        }
        if let Some(up) = self.rail_slope(b) {
            return Some([up, up.opposite()]);
        }
        Some(if v.upper {
            [v.facing, v.facing.clockwise()]
        } else if v.facing == Dir4::North {
            [Dir4::North, Dir4::South]
        } else {
            [Dir4::East, Dir4::West]
        })
    }

    /// Which way a sloping rail goes up, if it slopes.
    pub fn rail_slope(&self, b: BlockId) -> Option<Dir4> {
        let v = self.get(b).variant.as_ref()?;
        match v.kind {
            VariantKind::Rail if v.open => Some(v.facing),
            VariantKind::PoweredRail if v.upper => Some(v.facing),
            _ => None,
        }
    }

    /// The rail of `family` sloping up towards `up` (powered rails keep
    /// whether they're on).
    pub fn rail_sloping(&self, family: &str, up: Dir4, on: bool) -> Option<BlockId> {
        if family == "core:rail" {
            self.find_variant(family, up, true, false)
        } else {
            self.find_variant(family, up, on, true)
        }
    }

    /// The rail of `family` going these two ways (plain rails can turn
    /// corners; powered rails only go straight).
    pub fn rail_joining(&self, family: &str, a: Dir4, b: Dir4) -> Option<BlockId> {
        let straight = |d: Dir4| matches!(d, Dir4::North | Dir4::South);
        if a == b.opposite() {
            let facing = if straight(a) { Dir4::North } else { Dir4::East };
            return self.find_variant(family, facing, false, false);
        }
        if family != "core:rail" {
            return None;
        }
        // A corner is named after the way it comes from, going clockwise.
        let from = if a.clockwise() == b { a } else { b };
        self.find_variant(family, from, false, true)
    }
}

impl Dir4 {
    /// A quarter turn clockwise (seen from above).
    pub fn clockwise(self) -> Dir4 {
        match self {
            Dir4::North => Dir4::East,
            Dir4::East => Dir4::South,
            Dir4::South => Dir4::West,
            Dir4::West => Dir4::North,
        }
    }
}

/// Which way a piston (or its head) pushes. Pistons facing up or down
/// are the "upper" states: north means up, south means down.
pub fn piston_push(v: &crate::shapes::Variant) -> [i32; 3] {
    match (v.upper, v.facing) {
        (true, Dir4::North) => [0, 1, 0],
        (true, _) => [0, -1, 0],
        (false, d) => d.vector(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pistons_push_up_and_down() {
        let reg = BlockRegistry::with_builtin_blocks();
        let piston = reg.by_name("core:piston").unwrap();
        let push = |look: [f32; 3]| {
            let b = reg.placement(piston, [0, 1, 0], look).unwrap().block;
            piston_push(reg.get(b).variant.as_ref().unwrap())
        };
        assert_eq!(push([0.0, -1.0, 0.1]), [0, 1, 0], "looking at the floor");
        assert_eq!(push([0.0, 1.0, 0.1]), [0, -1, 0], "looking at the ceiling");
        assert_eq!(push([1.0, 0.0, 0.0]), [1, 0, 0]);
        // Every way it can face has a head and an out state.
        for (id, def) in reg.iter() {
            let Some(v) = def
                .variant
                .as_ref()
                .filter(|v| v.family == "core:sticky_piston")
            else {
                continue;
            };
            assert!(reg.powered(id, true).is_some(), "{}", def.name);
            assert!(
                reg.find_variant("core:sticky_piston_head", v.facing, false, v.upper)
                    .is_some()
            );
        }
    }

    #[test]
    fn levers_power_and_rails_turn() {
        let r = BlockRegistry::with_builtin_blocks();
        let lever = r.by_name("core:lever").unwrap();
        assert_eq!(r.source_power(lever), 0);
        let on = r.powered(lever, true).unwrap();
        assert_eq!(r.source_power(on), MAX_POWER);
        assert_eq!(r.powered(on, false), Some(lever));
        let wire = r.by_name("core:spark_wire").unwrap();
        assert!(r.is_wire(wire) && r.light(r.powered(wire, true).unwrap()) > 0);
        // Rails.
        let ns = r
            .rail_joining("core:rail", Dir4::North, Dir4::South)
            .unwrap();
        assert_eq!(r.rail_ends(ns), Some([Dir4::North, Dir4::South]));
        let corner = r
            .rail_joining("core:rail", Dir4::South, Dir4::East)
            .unwrap();
        let ends = r.rail_ends(corner).unwrap();
        assert!(ends.contains(&Dir4::South) && ends.contains(&Dir4::East));
        // Slopes, plain and powered (which stay on or off).
        for up in Dir4::ALL {
            let s = r.rail_sloping("core:rail", up, false).unwrap();
            assert_eq!(r.rail_slope(s), Some(up));
            assert_eq!(r.rail_ends(s), Some([up, up.opposite()]));
            let p = r.rail_sloping("core:powered_rail", up, true).unwrap();
            assert_eq!(r.rail_slope(p), Some(up));
            assert!(r.is_on(p));
            let off = r.powered(p, false).unwrap();
            assert_eq!(r.rail_slope(off), Some(up));
            assert_eq!(r.get(s).drops, Drops::Block(ns));
        }
        assert_eq!(r.rail_slope(ns), None);
        assert_eq!(r.rail_slope(corner), None);
        assert!(
            r.rail_joining("core:powered_rail", Dir4::North, Dir4::East)
                .is_none()
        );
    }
}

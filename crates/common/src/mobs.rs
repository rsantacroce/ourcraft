//! Animals ("mobs"): what kinds there are, their size, speed and looks.
//!
//! The server moves them around; the client draws them from these
//! definitions (sent in the content pack, since mods can add their own).
//! Models are made of coloured boxes, like everything else that moves:
//! a body, a head and legs that swing when walking.

use serde::{Deserialize, Serialize};

/// One box of an animal, in blocks, around the feet (`y = 0` is the
/// ground; the animal faces `+z`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MobPart {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub color: [u8; 4],
    /// Swings back and forth when walking. `1.0` and `-1.0` swing in
    /// opposite directions (left front with right back, like real legs).
    pub leg: f32,
    /// Wool: drawn in `skin` colour, a little thinner, once sheared.
    pub wool: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MobDef {
    /// `"core:sheep"`, `"yourmod:thing"`.
    pub name: String,
    pub display_name: String,
    /// Width and height of the body, for collisions and aiming.
    pub size: [f32; 2],
    /// Walking speed in blocks per second.
    pub speed: f32,
    pub parts: Vec<MobPart>,
    /// Colour under the wool (sheep).
    pub skin: [u8; 4],
    /// The grass-like block it appears on (by name), if it appears on its own.
    pub spawns_on: Option<String>,
    /// The item it loves: hold it and it follows you; feed two and they
    /// have a baby.
    pub food: Option<String>,
    /// Can be sheared for this item (by name).
    pub shears_into: Option<String>,
    /// Lays this item now and then (by name).
    pub lays: Option<String>,
    /// The sound it makes (one of the client's built-in sounds).
    pub sound: String,
    /// Monsters: they come out at night, chase players and bonk them.
    pub hostile: Option<Hostile>,
}

/// How a monster fights (gently: it's a game for kids).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hostile {
    /// Health taken from a player per bonk (out of 20).
    pub damage: f32,
    /// Its health: hands hit for 2, tools for more.
    pub health: f32,
    /// Moves by hopping (bouncy things) rather than walking.
    pub hops: bool,
}

fn part(min: [f32; 3], max: [f32; 3], color: [u8; 4]) -> MobPart {
    MobPart {
        min,
        max,
        color,
        leg: 0.0,
        wool: false,
    }
}

fn leg(min: [f32; 3], max: [f32; 3], color: [u8; 4], swing: f32) -> MobPart {
    MobPart {
        leg: swing,
        ..part(min, max, color)
    }
}

/// Four legs at the corners of a body. `w`/`d` = half width/depth to the
/// leg centres, `t` = leg thickness, `h` = leg height.
fn four_legs(w: f32, d: f32, t: f32, h: f32, color: [u8; 4]) -> Vec<MobPart> {
    let r = t / 2.0;
    [(-w, d, 1.0), (w, d, -1.0), (-w, -d, -1.0), (w, -d, 1.0)]
        .into_iter()
        .map(|(x, z, s)| leg([x - r, 0.0, z - r], [x + r, h, z + r], color, s))
        .collect()
}

/// Two black eyes on the front of a head whose front is at `z`.
fn eyes(x: f32, y: f32, z: f32, s: f32) -> Vec<MobPart> {
    let black = [25, 25, 30, 255];
    vec![
        part([-x - s, y, z], [-x, y + s, z + 0.02], black),
        part([x, y, z], [x + s, y + s, z + 0.02], black),
    ]
}

/// The animals that come with the game.
pub fn builtin_mobs() -> Vec<MobDef> {
    let base = |name: &str, display: &str, size: [f32; 2], speed: f32, sound: &str| MobDef {
        name: name.into(),
        display_name: display.into(),
        size,
        speed,
        parts: Vec::new(),
        skin: [0; 4],
        spawns_on: Some("core:grass".into()),
        food: Some("core:apple".into()),
        shears_into: None,
        lays: None,
        sound: sound.into(),
        hostile: None,
    };

    let wool = [242, 242, 236, 255];
    let face = [196, 178, 160, 255];
    let mut sheep = base("core:sheep", "Sheep", [0.8, 1.1], 1.1, "baa");
    sheep.skin = [232, 206, 186, 255];
    sheep.shears_into = Some("core:wool".into());
    sheep.parts = vec![
        MobPart {
            wool: true,
            ..part([-0.36, 0.45, -0.5], [0.36, 1.0, 0.45], wool)
        },
        part([-0.2, 0.7, 0.42], [0.2, 1.05, 0.78], face),
        MobPart {
            wool: true,
            ..part([-0.22, 1.0, 0.44], [0.22, 1.1, 0.7], wool)
        },
    ];
    sheep.parts.extend(eyes(0.08, 0.9, 0.78, 0.07));
    sheep.parts.extend(four_legs(0.22, 0.32, 0.14, 0.45, face));

    let pink = [242, 168, 176, 255];
    let mut pig = base("core:pig", "Pig", [0.8, 0.9], 1.2, "oink");
    pig.parts = vec![
        part([-0.32, 0.32, -0.5], [0.32, 0.82, 0.45], pink),
        part([-0.27, 0.4, 0.42], [0.27, 0.9, 0.82], pink),
        part([-0.13, 0.5, 0.82], [0.13, 0.68, 0.92], [250, 130, 150, 255]),
        // A curly tail.
        part(
            [-0.04, 0.68, -0.56],
            [0.04, 0.76, -0.5],
            [250, 140, 160, 255],
        ),
    ];
    pig.parts.extend(eyes(0.1, 0.74, 0.82, 0.07));
    pig.parts.extend(four_legs(0.2, 0.32, 0.16, 0.32, pink));

    let brown = [112, 80, 58, 255];
    let white = [238, 236, 230, 255];
    let mut cow = base("core:cow", "Cow", [0.9, 1.35], 0.9, "moo");
    cow.parts = vec![
        part([-0.4, 0.6, -0.62], [0.4, 1.22, 0.55], brown),
        // White patches on both sides.
        part([0.4, 0.75, -0.3], [0.42, 1.08, 0.1], white),
        part([-0.42, 0.85, -0.05], [-0.4, 1.12, 0.32], white),
        part([-0.24, 0.85, 0.5], [0.24, 1.32, 0.9], brown),
        part([-0.2, 0.85, 0.88], [0.2, 1.02, 0.98], [236, 196, 186, 255]),
        // Horns.
        part([-0.3, 1.26, 0.62], [-0.22, 1.4, 0.7], white),
        part([0.22, 1.26, 0.62], [0.3, 1.4, 0.7], white),
    ];
    cow.parts.extend(eyes(0.1, 1.14, 0.9, 0.07));
    cow.parts.extend(four_legs(0.26, 0.42, 0.18, 0.6, brown));

    let feathers = [248, 248, 244, 255];
    let orange = [244, 160, 40, 255];
    let mut chicken = base("core:chicken", "Chicken", [0.45, 0.9], 1.3, "cluck");
    chicken.lays = Some("core:egg".into());
    chicken.parts = vec![
        part([-0.18, 0.3, -0.25], [0.18, 0.62, 0.2], feathers),
        part([-0.12, 0.58, 0.1], [0.12, 0.88, 0.3], feathers),
        part([-0.05, 0.68, 0.3], [0.05, 0.75, 0.4], orange),
        part([-0.03, 0.88, 0.14], [0.03, 0.96, 0.26], [220, 40, 40, 255]),
        // Wings.
        part(
            [-0.21, 0.36, -0.18],
            [-0.18, 0.56, 0.12],
            [232, 232, 226, 255],
        ),
        part(
            [0.18, 0.36, -0.18],
            [0.21, 0.56, 0.12],
            [232, 232, 226, 255],
        ),
        leg([-0.09, 0.0, -0.02], [-0.04, 0.3, 0.03], orange, 1.0),
        leg([0.04, 0.0, -0.02], [0.09, 0.3, 0.03], orange, -1.0),
    ];
    chicken.parts.extend(eyes(0.05, 0.78, 0.3, 0.05));

    // ---- Monsters (milestone 15): cartoon and gentle. ----
    let mut grumble = base("core:grumble", "Grumble", [0.7, 1.5], 1.5, "grumble");
    grumble.spawns_on = None;
    grumble.food = None;
    grumble.hostile = Some(Hostile {
        damage: 2.0,
        health: 8.0,
        hops: false,
    });
    let teal = [70, 160, 150, 255];
    let purple = [120, 80, 170, 255];
    let white = [250, 250, 250, 255];
    grumble.parts = vec![
        part([-0.32, 0.55, -0.22], [0.32, 1.15, 0.22], teal),
        // A big round head with a grumpy brow and two huge eyes.
        part([-0.3, 1.1, -0.25], [0.3, 1.5, 0.27], teal),
        part([-0.24, 1.32, 0.27], [-0.05, 1.45, 0.29], white),
        part([0.05, 1.32, 0.27], [0.24, 1.45, 0.29], white),
        part([-0.17, 1.34, 0.29], [-0.1, 1.41, 0.31], [20, 20, 30, 255]),
        part([0.1, 1.34, 0.29], [0.17, 1.41, 0.31], [20, 20, 30, 255]),
        part([-0.26, 1.45, 0.27], [0.26, 1.5, 0.3], purple),
        // Purple shorts, and arms that swing like legs.
        part([-0.33, 0.55, -0.23], [0.33, 0.72, 0.23], purple),
        leg([-0.42, 0.6, -0.08], [-0.32, 1.1, 0.08], teal, -1.0),
        leg([0.32, 0.6, -0.08], [0.42, 1.1, 0.08], teal, 1.0),
        leg([-0.25, 0.0, -0.1], [-0.05, 0.55, 0.1], purple, 1.0),
        leg([0.05, 0.0, -0.1], [0.25, 0.55, 0.1], purple, -1.0),
    ];

    let mut bouncer = base("core:bouncer", "Bouncer", [0.8, 0.8], 1.8, "boing");
    bouncer.spawns_on = None;
    bouncer.food = None;
    bouncer.hostile = Some(Hostile {
        damage: 1.0,
        health: 6.0,
        hops: true,
    });
    let jelly = [190, 100, 220, 255];
    bouncer.parts = vec![
        part([-0.4, 0.0, -0.4], [0.4, 0.8, 0.4], jelly),
        part([-0.28, 0.45, 0.4], [-0.08, 0.65, 0.42], white),
        part([0.08, 0.45, 0.4], [0.28, 0.65, 0.42], white),
        part([-0.2, 0.5, 0.42], [-0.12, 0.6, 0.44], [20, 20, 30, 255]),
        part([0.12, 0.5, 0.42], [0.2, 0.6, 0.44], [20, 20, 30, 255]),
    ];

    vec![sheep, pig, cow, chicken, grumble, bouncer]
}

/// The model name clients recognise as "draw this mob":
/// `@mob:<name>`, plus `:baby` and/or `:sheared`.
pub fn mob_model(name: &str, baby: bool, sheared: bool) -> String {
    let mut s = format!("@mob:{name}");
    if baby {
        s.push_str(":baby");
    }
    if sheared {
        s.push_str(":sheared");
    }
    s
}

/// Undo [`mob_model`]: (name, baby, sheared).
pub fn parse_mob_model(model: &str) -> Option<(&str, bool, bool)> {
    let rest = model.strip_prefix("@mob:")?;
    let mut baby = false;
    let mut sheared = false;
    let mut name = rest;
    loop {
        if let Some(n) = name.strip_suffix(":baby") {
            baby = true;
            name = n;
        } else if let Some(n) = name.strip_suffix(":sheared") {
            sheared = true;
            name = n;
        } else {
            break;
        }
    }
    Some((name, baby, sheared))
}

/// How big a baby is compared with a grown-up.
pub const BABY_SCALE: f32 = 0.55;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_round_trip() {
        for (baby, sheared) in [(false, false), (true, false), (false, true), (true, true)] {
            let m = mob_model("core:sheep", baby, sheared);
            assert_eq!(parse_mob_model(&m), Some(("core:sheep", baby, sheared)));
        }
        assert_eq!(parse_mob_model("splat:flag_red"), None);
    }

    #[test]
    fn animals_stand_on_their_legs() {
        for m in builtin_mobs() {
            let lowest = m.parts.iter().map(|p| p.min[1]).fold(f32::MAX, f32::min);
            let highest = m.parts.iter().map(|p| p.max[1]).fold(0.0, f32::max);
            assert_eq!(lowest, 0.0, "{} floats", m.name);
            assert!(
                highest <= m.size[1] + 0.1,
                "{} is taller than its box",
                m.name
            );
            let jelly = m.hostile.is_some_and(|h| h.hops);
            assert!(
                jelly || m.parts.iter().any(|p| p.leg != 0.0),
                "{} has no legs",
                m.name
            );
        }
    }
}

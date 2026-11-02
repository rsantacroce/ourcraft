//! The game's own player skins, drawn by code like the block textures.
//!
//! Each skin is an [`Outfit`] (skin tone, hair, shirt, trousers, shoes and
//! maybe a hat), painted onto the classic 64×64 skin layout so it wraps
//! around the avatar exactly like a skin file would.
//!
//! Players can also make their own outfit (see [`MAKER_PARTS`]): the
//! choices are packed into the skin's name (`made_` and one letter or
//! digit per choice), so every game can draw it from the name alone.

use ourcraft_common::skin::{BodyPart, Face, SKIN_BYTES, SkinBox, face_rects, put_pixel};

use super::textures::{Rgba, hash, rgb, shift, speckle};

/// The built-in skins: (name saved in settings, name shown to players).
pub const BUILTIN_SKINS: [(&str, &str); 6] = [
    ("explorer", "Explorer"),
    ("miner", "Miner"),
    ("gardener", "Gardener"),
    ("knight", "Knight"),
    ("astronaut", "Astronaut"),
    ("robot", "Robot"),
];

/// What's on the head's outer layer.
#[derive(Clone, Copy, PartialEq)]
enum Hat {
    None,
    /// A yellow hard hat with a lamp.
    Hard,
    /// A straw sun hat with a red band.
    Straw,
    /// A steel helmet with a slit to see through and a red plume.
    Knight,
    /// A white space helmet with a glass visor.
    Space,
}

struct Outfit {
    seed: u32,
    skin: Rgba,
    hair: Rgba,
    /// Hair down to the shoulders (otherwise short).
    long_hair: bool,
    eyes: Rgba,
    shirt: Rgba,
    /// Rows of sleeve on the arms (out of 12); the rest is hands.
    sleeves: u32,
    /// Hands, if gloved (otherwise skin).
    gloves: Option<Rgba>,
    trousers: Rgba,
    shoes: Rgba,
    belt: Option<Rgba>,
    /// Dungarees over the shirt (front and back of the body, with straps).
    overalls: Option<Rgba>,
    /// A tabard down the front of the body.
    tabard: Option<Rgba>,
    hat: Hat,
    /// Metal face with glowing eyes and a grille for a mouth.
    robot: bool,
    /// An open shirt collar (a little skin showing at the neck).
    collar: bool,
    /// How much the cloth's colour varies from pixel to pixel.
    texture: f32,
    face: FaceStyle,
}

/// The look on a face.
#[derive(Clone, Copy, PartialEq)]
enum FaceStyle {
    Smile,
    Grin,
    Surprised,
    Freckles,
    Glasses,
}

fn outfit(name: &str) -> Option<Outfit> {
    let base = Outfit {
        seed: 1,
        skin: rgb(226, 178, 136),
        hair: rgb(92, 60, 36),
        long_hair: false,
        eyes: rgb(60, 90, 160),
        shirt: rgb(40, 160, 160),
        sleeves: 4,
        gloves: None,
        trousers: rgb(60, 70, 150),
        shoes: rgb(80, 80, 86),
        belt: None,
        overalls: None,
        tabard: None,
        hat: Hat::None,
        robot: false,
        collar: true,
        texture: 8.0,
        face: FaceStyle::Smile,
    };
    Some(match name {
        "explorer" => Outfit {
            belt: Some(rgb(110, 72, 40)),
            ..base
        },
        "miner" => Outfit {
            seed: 2,
            skin: rgb(196, 140, 100),
            hair: rgb(40, 32, 28),
            eyes: rgb(70, 50, 30),
            shirt: rgb(200, 190, 170),
            overalls: Some(rgb(222, 120, 40)),
            trousers: rgb(222, 120, 40),
            shoes: rgb(70, 50, 34),
            sleeves: 6,
            gloves: Some(rgb(120, 96, 60)),
            hat: Hat::Hard,
            ..base
        },
        "gardener" => Outfit {
            seed: 3,
            skin: rgb(150, 100, 70),
            hair: rgb(30, 22, 18),
            long_hair: true,
            eyes: rgb(60, 40, 24),
            shirt: rgb(90, 170, 70),
            trousers: rgb(120, 96, 70),
            shoes: rgb(70, 110, 50),
            belt: None,
            hat: Hat::Straw,
            ..base
        },
        "knight" => Outfit {
            seed: 4,
            skin: rgb(236, 196, 160),
            hair: rgb(200, 160, 70),
            eyes: rgb(40, 110, 60),
            shirt: rgb(150, 154, 162),
            sleeves: 10,
            gloves: Some(rgb(90, 90, 96)),
            trousers: rgb(130, 134, 142),
            shoes: rgb(70, 70, 76),
            tabard: Some(rgb(170, 40, 44)),
            collar: false,
            belt: Some(rgb(70, 50, 30)),
            hat: Hat::Knight,
            texture: 18.0,
            ..base
        },
        "astronaut" => Outfit {
            seed: 5,
            skin: rgb(240, 200, 170),
            hair: rgb(170, 70, 40),
            eyes: rgb(50, 120, 90),
            shirt: rgb(232, 234, 238),
            sleeves: 10,
            gloves: Some(rgb(200, 202, 210)),
            trousers: rgb(232, 234, 238),
            shoes: rgb(160, 164, 172),
            belt: Some(rgb(70, 110, 200)),
            hat: Hat::Space,
            collar: false,
            texture: 4.0,
            ..base
        },
        "robot" => Outfit {
            seed: 6,
            skin: rgb(150, 158, 170),
            hair: rgb(110, 118, 130),
            eyes: rgb(90, 230, 255),
            shirt: rgb(130, 138, 150),
            sleeves: 12,
            trousers: rgb(120, 128, 140),
            shoes: rgb(70, 74, 84),
            belt: Some(rgb(230, 170, 40)),
            robot: true,
            collar: false,
            texture: 10.0,
            ..base
        },
        _ => return made_outfit(name),
    })
}

/// Name prefix of a skin made in the skin maker.
pub const MADE_PREFIX: &str = "made_";

/// The skin maker's choices, in the order they're packed into the name:
/// (what's being chosen, the options).
pub const MAKER_PARTS: [(&str, &[&str]); 9] = [
    (
        "Skin",
        &[
            "Peach", "Tan", "Brown", "Dark", "Pale", "Olive", "Green", "Blue",
        ],
    ),
    (
        "Hair colour",
        &[
            "Brown", "Black", "Blonde", "Ginger", "Grey", "White", "Pink", "Blue", "Green",
            "Purple",
        ],
    ),
    ("Hair", &["Short", "Long"]),
    (
        "Eyes",
        &["Blue", "Brown", "Green", "Grey", "Hazel", "Violet"],
    ),
    (
        "Face",
        &["Smile", "Grin", "Surprised", "Freckles", "Glasses"],
    ),
    ("Shirt", &COLOUR_NAMES),
    ("Trousers", &COLOUR_NAMES),
    ("Shoes", &["Grey", "Brown", "Black", "White", "Red", "Blue"]),
    (
        "Hat",
        &[
            "None",
            "Hard hat",
            "Straw hat",
            "Knight's helmet",
            "Space helmet",
        ],
    ),
];

const COLOUR_NAMES: [&str; 16] = [
    "White",
    "Light grey",
    "Grey",
    "Black",
    "Brown",
    "Red",
    "Orange",
    "Yellow",
    "Lime",
    "Green",
    "Cyan",
    "Light blue",
    "Blue",
    "Purple",
    "Magenta",
    "Pink",
];

const SKIN_TONES: [[u8; 3]; 8] = [
    [226, 178, 136],
    [200, 150, 106],
    [150, 100, 70],
    [100, 66, 46],
    [244, 210, 186],
    [196, 160, 110],
    [120, 190, 110],
    [120, 170, 230],
];
const HAIR: [[u8; 3]; 10] = [
    [92, 60, 36],
    [36, 30, 28],
    [226, 196, 110],
    [190, 90, 40],
    [140, 140, 146],
    [236, 236, 230],
    [240, 130, 180],
    [70, 110, 220],
    [70, 170, 80],
    [130, 70, 190],
];
const EYES: [[u8; 3]; 6] = [
    [60, 90, 160],
    [70, 50, 30],
    [40, 120, 60],
    [110, 120, 130],
    [130, 100, 50],
    [130, 80, 190],
];
const CLOTH: [[u8; 3]; 16] = [
    [236, 236, 236],
    [160, 160, 166],
    [90, 92, 98],
    [36, 36, 40],
    [120, 80, 50],
    [190, 50, 50],
    [230, 130, 40],
    [236, 200, 50],
    [130, 200, 60],
    [60, 130, 60],
    [40, 160, 160],
    [110, 170, 230],
    [60, 70, 170],
    [130, 60, 170],
    [200, 70, 180],
    [240, 150, 180],
];
const SHOES: [[u8; 3]; 6] = [
    [80, 80, 86],
    [90, 60, 40],
    [30, 30, 34],
    [230, 230, 230],
    [190, 50, 50],
    [60, 80, 170],
];

/// The skin name for these choices (one index per [`MAKER_PARTS`]).
pub fn made_skin_name(choices: [u8; 9]) -> String {
    let mut name = MADE_PREFIX.to_owned();
    for (c, (_, options)) in choices.iter().zip(MAKER_PARTS) {
        let c = (*c as usize).min(options.len() - 1) as u32;
        name.push(std::char::from_digit(c, 36).unwrap_or('0'));
    }
    name
}

/// The choices packed into a made skin's name, if it is one.
pub fn made_skin_choices(name: &str) -> Option<[u8; 9]> {
    let code = name.strip_prefix(MADE_PREFIX)?;
    if code.len() != MAKER_PARTS.len() {
        return None;
    }
    let mut choices = [0u8; 9];
    for (i, ch) in code.chars().enumerate() {
        let c = ch.to_digit(36)? as usize;
        if ch.is_ascii_uppercase() || c >= MAKER_PARTS[i].1.len() {
            return None;
        }
        choices[i] = c as u8;
    }
    Some(choices)
}

fn made_outfit(name: &str) -> Option<Outfit> {
    let c = made_skin_choices(name)?.map(usize::from);
    let col = |a: [u8; 3]| rgb(a[0], a[1], a[2]);
    let seed = name
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32))
        % 10_000;
    Some(Outfit {
        seed,
        skin: col(SKIN_TONES[c[0]]),
        hair: col(HAIR[c[1]]),
        long_hair: c[2] == 1,
        eyes: col(EYES[c[3]]),
        face: [
            FaceStyle::Smile,
            FaceStyle::Grin,
            FaceStyle::Surprised,
            FaceStyle::Freckles,
            FaceStyle::Glasses,
        ][c[4]],
        shirt: col(CLOTH[c[5]]),
        sleeves: 4,
        gloves: None,
        trousers: col(CLOTH[c[6]]),
        shoes: col(SHOES[c[7]]),
        belt: None,
        overalls: None,
        tabard: None,
        hat: [Hat::None, Hat::Hard, Hat::Straw, Hat::Knight, Hat::Space][c[8]],
        robot: false,
        collar: true,
        texture: 8.0,
    })
}

/// A flat picture of a skin from the front (16×32 RGBA), for showing in
/// menus: head (with hat) on top, then body and arms, then legs.
pub fn front_view(skin: &[u8]) -> Vec<u8> {
    const W: u32 = 16;
    let mut out = vec![0u8; (W * 32 * 4) as usize];
    let front = Face::ALL
        .iter()
        .position(|f| *f == Face::Front)
        .unwrap_or(0);
    let mut copy = |b: SkinBox, at_x: u32, at_y: u32| {
        let (fx, fy, w, h) = face_rects(b)[front];
        for y in 0..h {
            for x in 0..w {
                let c = ourcraft_common::skin::get_pixel(skin, fx + x, fy + y);
                if c[3] > 0 {
                    let i = (((at_y + y) * W + at_x + x) * 4) as usize;
                    out[i..i + 4].copy_from_slice(&c);
                }
            }
        }
    };
    copy(BodyPart::Head.inner(), 4, 0);
    copy(BodyPart::Head.outer(), 4, 0);
    copy(BodyPart::Body.inner(), 4, 8);
    // The right arm and leg are on the left as you look at someone.
    copy(BodyPart::RightArm.inner(), 0, 8);
    copy(BodyPart::LeftArm.inner(), 12, 8);
    copy(BodyPart::RightLeg.inner(), 4, 20);
    copy(BodyPart::LeftLeg.inner(), 8, 20);
    out
}

/// A built-in skin's 64×64 RGBA pixels, if `name` is one.
pub fn builtin_skin(name: &str) -> Option<Vec<u8>> {
    let o = outfit(name)?;
    let mut skin = vec![0; SKIN_BYTES];
    for part in BodyPart::ALL {
        paint_box(&mut skin, part.inner(), |face, x, y, w, h| {
            Some(inner_pixel(&o, part, face, x, y, w, h))
        });
    }
    if o.hat != Hat::None {
        paint_box(&mut skin, BodyPart::Head.outer(), |face, x, y, _, _| {
            hat_pixel(&o, face, x, y)
        });
    }
    Some(skin)
}

/// Fill each face of a box with `f(face, x, y, width, height)` (`x, y`
/// counted from the face's top left); `None` leaves a pixel see-through.
fn paint_box(skin: &mut [u8], b: SkinBox, f: impl Fn(Face, u32, u32, u32, u32) -> Option<Rgba>) {
    for (face, (fx, fy, w, h)) in Face::ALL.into_iter().zip(face_rects(b)) {
        for y in 0..h {
            for x in 0..w {
                if let Some(c) = f(face, x, y, w, h) {
                    put_pixel(skin, fx + x, fy + y, c);
                }
            }
        }
    }
}

/// Cloth: a colour with a little random variation (and seams on the edges).
fn cloth(o: &Outfit, c: Rgba, x: u32, y: u32, salt: u32) -> Rgba {
    speckle(
        c,
        x as usize,
        y as usize,
        o.seed.wrapping_mul(31).wrapping_add(salt),
        o.texture,
    )
}

fn inner_pixel(o: &Outfit, part: BodyPart, face: Face, x: u32, y: u32, w: u32, h: u32) -> Rgba {
    match part {
        BodyPart::Head => head_pixel(o, face, x, y),
        BodyPart::Body => body_pixel(o, face, x, y, w),
        BodyPart::RightArm | BodyPart::LeftArm => {
            let hand = o.gloves.unwrap_or(o.skin);
            let c = match face {
                Face::Top => o.shirt,
                Face::Bottom => hand,
                _ if y < o.sleeves => o.shirt,
                _ => hand,
            };
            // The cuff of the sleeve is a shade darker.
            let c = if face != Face::Top && face != Face::Bottom && y + 1 == o.sleeves {
                shift(c, -18.0)
            } else {
                c
            };
            cloth(o, c, x + face as u32 * 8, y, 1)
        }
        BodyPart::RightLeg | BodyPart::LeftLeg => {
            let shoe_rows = 3;
            let c = match face {
                Face::Top => o.trousers,
                Face::Bottom => shift(o.shoes, -20.0),
                _ if y + shoe_rows >= h => o.shoes,
                _ => o.trousers,
            };
            // The inside of each leg is a little darker, so they read as two.
            let inside = matches!(
                (part, face),
                (BodyPart::RightLeg, Face::Left) | (BodyPart::LeftLeg, Face::Right)
            );
            let c = if inside { shift(c, -14.0) } else { c };
            cloth(o, c, x + face as u32 * 8, y, 2 + part as u32)
        }
    }
}

fn head_pixel(o: &Outfit, face: Face, x: u32, y: u32) -> Rgba {
    let hair = |x: u32, y: u32| speckle(o.hair, x as usize, y as usize, o.seed + 40, 14.0);
    let skin = speckle(o.skin, x as usize, y as usize, o.seed + 41, 4.0);
    match face {
        Face::Top => hair(x, y),
        Face::Bottom => shift(o.skin, -16.0),
        Face::Back => {
            let ends = if o.long_hair { 8 } else { 6 };
            if y < ends { hair(x, y) } else { skin }
        }
        Face::Right | Face::Left => {
            // Hair down the side; long hair reaches the shoulders except
            // near the face.
            let near_face = if face == Face::Right { x >= 6 } else { x <= 1 };
            let ends = match (o.long_hair, near_face) {
                (true, false) => 8,
                (true, true) => 3,
                _ => 2 + (hash(x as usize, 0, o.seed) % 2),
            };
            if y < ends { hair(x, y) } else { skin }
        }
        Face::Front => front_of_head(o, x, y, skin, hair(x, y)),
    }
}

/// The face: hair along the top, eyes on row 4, a mouth on row 6.
fn front_of_head(o: &Outfit, x: u32, y: u32, skin: Rgba, hair: Rgba) -> Rgba {
    if o.robot {
        return match (x, y) {
            (0 | 7, _) | (_, 0) => shift(o.skin, -24.0),
            (1..=2 | 5..=6, 3) => o.eyes,
            (1..=2 | 5..=6, 4) => shift(o.eyes, -50.0),
            (2..=5, 6) if x.is_multiple_of(2) => rgb(40, 44, 52),
            (2..=5, 6) => rgb(70, 76, 86),
            _ => skin,
        };
    }
    let fringe =
        y == 0 || (y == 1 && (x == 0 || x == 7 || hash(x as usize, 1, o.seed).is_multiple_of(3)));
    if fringe || (o.long_hair && (x == 0 || x == 7) && y < 6) {
        return hair;
    }
    let frame = rgb(40, 36, 40);
    match (o.face, x, y) {
        // Glasses: a dark frame round the eyes and across the nose.
        (FaceStyle::Glasses, 0..=7, 3) => frame,
        (FaceStyle::Glasses, 0 | 3 | 4 | 7, 4) => frame,
        // Surprised: eyebrows up, and an "o" of a mouth.
        (FaceStyle::Surprised, 1..=2 | 5..=6, 2) => shift(o.hair, -10.0),
        (FaceStyle::Surprised, 1..=2 | 5..=6, 3) => skin,
        (FaceStyle::Surprised, 3..=4, 6) => rgb(90, 40, 40),
        (FaceStyle::Surprised, 3..=4, 7) => shift(o.skin, -30.0),
        (FaceStyle::Surprised, 2 | 5, 6) => skin,
        // Eyebrows, then eyes: white on the outside, colour inside.
        (_, 1..=2 | 5..=6, 3) => shift(o.hair, -10.0),
        (_, 1 | 6, 4) => rgb(250, 250, 250),
        (_, 2 | 5, 4) => o.eyes,
        // Freckles on the cheeks.
        (FaceStyle::Freckles, 0 | 2 | 5 | 7, 5) => shift(o.skin, -45.0),
        // A little nose.
        (_, 3..=4, 5) => shift(o.skin, -22.0),
        // A big grin: a wide mouth with teeth.
        (FaceStyle::Grin, 1..=6, 6) => {
            if x == 1 || x == 6 {
                shift(o.skin, -30.0)
            } else {
                rgb(250, 250, 250)
            }
        }
        (FaceStyle::Grin, 2..=5, 7) => rgb(150, 70, 70),
        // A smile.
        (_, 2..=5, 6) => {
            if x == 2 || x == 5 {
                shift(o.skin, -30.0)
            } else {
                rgb(150, 70, 70)
            }
        }
        _ => skin,
    }
}

fn body_pixel(o: &Outfit, face: Face, x: u32, y: u32, w: u32) -> Rgba {
    let salt = 10 + face as u32;
    let shirt = cloth(o, o.shirt, x, y, salt);
    let sides = matches!(face, Face::Front | Face::Back);
    // A belt round the middle.
    if let Some(belt) = o.belt
        && face != Face::Top
        && face != Face::Bottom
        && y == 10
    {
        let buckle = face == Face::Front && (x == 3 || x == 4);
        return if buckle { rgb(220, 190, 80) } else { belt };
    }
    // Dungarees: a bib below the chest and two straps over the shoulders.
    if let Some(over) = o.overalls {
        let strap = sides && (x == 1 || x == w - 2);
        let bib = face != Face::Top && y >= 6;
        if bib || strap || (face == Face::Top && (x == 1 || x == w - 2)) {
            let c = cloth(o, over, x, y, salt + 20);
            let button = face == Face::Front && y == 6 && strap;
            return if button { rgb(230, 210, 120) } else { c };
        }
    }
    // A tabard with a gold stripe.
    if let Some(tabard) = o.tabard
        && sides
        && (2..=5).contains(&x)
        && y >= 1
    {
        let stripe = face == Face::Front && (x == 3 || x == 4) && (3..=6).contains(&y);
        return if stripe {
            rgb(230, 190, 70)
        } else {
            cloth(o, tabard, x, y, salt + 30)
        };
    }
    // An open collar on the front.
    if o.collar && face == Face::Front && y == 0 && (x == 3 || x == 4) {
        return o.skin;
    }
    // A glowing panel on a robot's chest.
    if o.robot && face == Face::Front && (2..=5).contains(&x) && (2..=5).contains(&y) {
        return if (x + y).is_multiple_of(2) {
            rgb(90, 230, 255)
        } else {
            rgb(50, 140, 170)
        };
    }
    shirt
}

fn hat_pixel(o: &Outfit, face: Face, x: u32, y: u32) -> Option<Rgba> {
    let px = |c: Rgba, salt: u32| Some(cloth(o, c, x, y, 50 + salt));
    match o.hat {
        Hat::None => None,
        Hat::Hard => {
            let yellow = rgb(240, 200, 40);
            match face {
                Face::Top => px(yellow, 0),
                Face::Bottom => None,
                // A lamp on the front.
                Face::Front if (3..=4).contains(&x) && y == 1 => Some(rgb(255, 250, 200)),
                Face::Front if (2..=5).contains(&x) && y <= 2 => Some(rgb(90, 90, 90)),
                _ if y <= 1 => px(yellow, 1),
                _ if y == 2 => px(shift(yellow, -40.0), 2),
                _ => None,
            }
        }
        Hat::Straw => {
            let straw = rgb(226, 196, 110);
            match face {
                Face::Top => px(straw, 0),
                Face::Bottom => None,
                _ if y <= 1 => px(straw, 1),
                _ if y == 2 => Some(rgb(190, 50, 50)),
                _ => None,
            }
        }
        Hat::Knight => {
            let steel = rgb(176, 180, 188);
            match face {
                // A red plume down the middle of the top.
                Face::Top if x == 3 || x == 4 => Some(rgb(200, 40, 44)),
                Face::Top => px(steel, 0),
                Face::Bottom => None,
                Face::Front if y == 4 && (1..=6).contains(&x) => Some(rgb(24, 24, 28)),
                Face::Front if y >= 6 && (x + y).is_multiple_of(2) => px(shift(steel, -30.0), 1),
                _ => px(steel, 2),
            }
        }
        Hat::Space => {
            let white = rgb(236, 238, 242);
            match face {
                Face::Bottom => None,
                Face::Front if (1..=6).contains(&x) && (2..=6).contains(&y) => {
                    // Dark glass with a shine in one corner.
                    let shine = (x == 2 && y == 3) || (x == 3 && y == 2) || (x == 2 && y == 2);
                    Some(if shine {
                        rgb(200, 230, 255)
                    } else {
                        rgb(30, 60, 110)
                    })
                }
                Face::Right | Face::Left if (3..=4).contains(&x) && (3..=4).contains(&y) => {
                    Some(rgb(70, 110, 200))
                }
                _ => px(white, 3),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ourcraft_common::skin::get_pixel;

    #[test]
    fn every_builtin_skin_is_drawn() {
        for (name, _) in BUILTIN_SKINS {
            let skin = builtin_skin(name).unwrap_or_else(|| panic!("no skin {name}"));
            assert_eq!(skin.len(), SKIN_BYTES);
            // The whole inner layer is painted (no holes in the body).
            for part in BodyPart::ALL {
                for (x, y, w, h) in face_rects(part.inner()) {
                    for py in y..y + h {
                        for px in x..x + w {
                            assert_eq!(get_pixel(&skin, px, py)[3], 255, "{name} {part:?}");
                        }
                    }
                }
            }
        }
        assert!(builtin_skin("nobody").is_none());
        assert!(builtin_skin("made_000000000").is_some());
        // They don't all look the same.
        assert_ne!(builtin_skin("explorer"), builtin_skin("miner"));
    }

    #[test]
    fn made_skins_round_trip_through_their_names() {
        let choices = [3, 9, 1, 5, 4, 15, 12, 5, 4];
        let name = made_skin_name(choices);
        assert_eq!(name, "made_39154fc54");
        assert!(name.len() <= ourcraft_common::skin::MAX_SKIN_NAME);
        assert!(ourcraft_common::skin::NetSkin::Builtin(name.clone()).is_valid());
        assert_eq!(made_skin_choices(&name), Some(choices));
        let skin = builtin_skin(&name).unwrap();
        assert_ne!(skin, builtin_skin(&made_skin_name([0; 9])).unwrap());
        // Out of range, too short or upper case: not a made skin.
        for bad in ["made_99999999z", "made_000", "made_00000000A", "made_"] {
            assert!(made_skin_choices(bad).is_none(), "{bad}");
        }
        // Every face is different.
        let faces: Vec<_> = (0..5u8)
            .map(|f| {
                front_view(&builtin_skin(&made_skin_name([0, 0, 0, 0, f, 0, 0, 0, 0])).unwrap())
            })
            .collect();
        for (i, a) in faces.iter().enumerate() {
            for b in &faces[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn front_view_has_a_whole_body() {
        let view = front_view(&builtin_skin("explorer").unwrap());
        assert_eq!(view.len(), 16 * 32 * 4);
        // Head, body and legs are all there (not see-through).
        for (x, y) in [(8, 4), (8, 14), (6, 26), (10, 26), (1, 10), (14, 10)] {
            assert_eq!(view[((y * 16 + x) * 4 + 3) as usize], 255, "({x}, {y})");
        }
    }
}

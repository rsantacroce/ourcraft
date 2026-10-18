//! The game's own player skins, drawn by code like the block textures.
//!
//! Each skin is an [`Outfit`] (skin tone, hair, shirt, trousers, shoes and
//! maybe a hat), painted onto the classic 64×64 skin layout so it wraps
//! around the avatar exactly like a skin file would.

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
        _ => return None,
    })
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
    speckle(c, x as usize, y as usize, o.seed * 31 + salt, o.texture)
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
    match (x, y) {
        // Eyebrows, then eyes: white on the outside, colour inside.
        (1..=2 | 5..=6, 3) => shift(o.hair, -10.0),
        (1 | 6, 4) => rgb(250, 250, 250),
        (2 | 5, 4) => o.eyes,
        // A little nose and a smile.
        (3..=4, 5) => shift(o.skin, -22.0),
        (2..=5, 6) => {
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
        // They don't all look the same.
        assert_ne!(builtin_skin("explorer"), builtin_skin("miner"));
    }
}

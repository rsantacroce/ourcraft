//! Farming textures (milestone 17): farmland, crops at every stage, stems,
//! pumpkins, melons, sugar cane, tall grass, mushrooms, cake and the
//! composter. Drawn by code like the other block textures.
//!
//! Plants are drawn on two crossed panels whose height grows with the
//! stage; a panel shows the bottom rows of its texture, so plants are
//! drawn from the bottom up.

use super::textures::{Rgba, dirt, hash, one_in, planks, rgb, shift, speckle};

const CLEAR: Rgba = [0, 0, 0, 0];

fn lerp(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let m = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8;
    [m(0), m(1), m(2), 255]
}

/// The pixel at `x, y` of a farming texture, or `None` if `name` isn't one.
pub fn pixel(name: &str, x: usize, y: usize) -> Option<Rgba> {
    // Stage textures: "wheat_3", "stem_5"…
    if let Some((plant, stage)) = name.rsplit_once('_')
        && let Ok(stage) = stage.parse::<u32>()
    {
        return Some(match plant {
            "wheat" => wheat(x, y, stage, 7),
            "carrots" => leafy(x, y, stage, rgb(236, 128, 30)),
            "potatoes" => leafy(x, y, stage, rgb(196, 160, 96)),
            "beetroots" => leafy(x, y, stage, rgb(150, 30, 50)),
            "stem" => stem(x, y, stage, false),
            _ => return None,
        });
    }
    Some(match name {
        "farmland" => farmland(x, y, false),
        "farmland_wet" => farmland(x, y, true),
        "stem_ripe" => stem(x, y, 7, true),
        "pumpkin_side" => pumpkin_side(x, y),
        "pumpkin_top" => pumpkin_top(x, y),
        "pumpkin_face" => pumpkin_face(x, y),
        "melon_side" => melon_side(x, y),
        "melon_top" => melon_top(x, y),
        "sugar_cane" => sugar_cane(x, y),
        "tall_grass" => tall_grass(x, y),
        "red_mushroom" => mushroom(x, y, rgb(214, 44, 40), true),
        "brown_mushroom" => mushroom(x, y, rgb(150, 108, 76), false),
        "cake_top" => cake_top(x, y),
        "cake_side" => cake_side(x, y),
        "cake_bottom" => speckle(rgb(170, 110, 60), x, y, 61, 8.0),
        "compost" => compost(x, y),
        "composter_side" => composter_side(x, y),
        _ => return None,
    })
}

fn farmland(x: usize, y: usize, wet: bool) -> Rgba {
    let c = shift(dirt(x, y), if wet { -45.0 } else { -12.0 });
    // Furrows across the field.
    if y.is_multiple_of(4) {
        shift(c, -22.0)
    } else {
        c
    }
}

/// Stalks of wheat: green when young, golden with ears of grain when ripe.
fn wheat(x: usize, y: usize, stage: u32, max: u32) -> Rgba {
    let height = 3 + 13 * stage / max;
    let top = 16 - height as usize;
    let stalk = x % 3 == (hash(x, 0, 40) % 2) as usize;
    if y < top || !stalk {
        return CLEAR;
    }
    let ripe = stage as f32 / max as f32;
    let c = lerp(rgb(70, 150, 40), rgb(214, 180, 70), ripe * ripe);
    // Ears of grain near the top once it's growing up.
    if stage >= 4 && y < top + 4 {
        return shift(
            c,
            if (x + y).is_multiple_of(2) {
                -30.0
            } else {
                10.0
            },
        );
    }
    speckle(c, x, y, 41, 10.0)
}

/// Leafy tufts (carrots, potatoes, beetroots), with the root showing at
/// the bottom when ripe.
fn leafy(x: usize, y: usize, stage: u32, root: Rgba) -> Rgba {
    let height = 4 + stage * 3;
    let top = 16 - height as usize;
    if stage >= 3 && y >= 13 && (4..12).contains(&x) && one_in(x, y, 42, 2) {
        return speckle(root, x, y, 43, 14.0);
    }
    let leaf = !(x + y / 3).is_multiple_of(3) && x > 1 && x < 15;
    if y < top || !leaf {
        return CLEAR;
    }
    speckle(rgb(60, 160, 50), x, y, 44, 18.0)
}

/// A pumpkin or melon stem: grows taller, and bends brown when ripe.
fn stem(x: usize, y: usize, stage: u32, ripe: bool) -> Rgba {
    let height = 2 + stage * 2;
    let top = 16usize.saturating_sub(height as usize);
    let lean = if ripe { (15 - y) / 4 } else { 0 };
    if y < top || x != 7 + lean.min(6) {
        return CLEAR;
    }
    let c = if ripe {
        rgb(170, 130, 60)
    } else {
        lerp(rgb(80, 170, 60), rgb(150, 170, 60), stage as f32 / 7.0)
    };
    speckle(c, x, y, 45, 8.0)
}

fn pumpkin_side(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(226, 132, 30), x, y, 46, 10.0);
    if x.is_multiple_of(4) {
        shift(c, -30.0)
    } else {
        c
    }
}

fn pumpkin_top(x: usize, y: usize) -> Rgba {
    if (7..=8).contains(&x) && (7..=8).contains(&y) {
        return rgb(90, 120, 40);
    }
    let ring = ((x as f32 - 7.5).abs() + (y as f32 - 7.5).abs()) as usize;
    let c = speckle(rgb(220, 128, 30), x, y, 47, 10.0);
    if ring.is_multiple_of(4) {
        shift(c, -25.0)
    } else {
        c
    }
}

/// A friendly glowing face.
fn pumpkin_face(x: usize, y: usize) -> Rgba {
    let glow = rgb(255, 220, 90);
    let eye = (4..=5).contains(&y) && ((3..=5).contains(&x) || (10..=12).contains(&x));
    let smile = (y == 10 && (4..=11).contains(&x))
        || (y == 9 && (x == 3 || x == 12))
        || (y == 11 && (6..=9).contains(&x));
    if eye || smile {
        glow
    } else {
        pumpkin_side(x, y)
    }
}

fn melon_side(x: usize, y: usize) -> Rgba {
    let stripe = (x + (hash(0, y / 2, 48) % 2) as usize) % 4 < 2;
    let c = if stripe {
        rgb(80, 150, 40)
    } else {
        rgb(120, 180, 50)
    };
    speckle(c, x, y, 49, 8.0)
}

fn melon_top(x: usize, y: usize) -> Rgba {
    let d = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
    if d < 2.0 {
        rgb(170, 140, 60)
    } else {
        melon_side(x, y)
    }
}

fn sugar_cane(x: usize, y: usize) -> Rgba {
    let stalk = matches!(x, 3 | 4 | 8 | 9 | 12);
    if !stalk {
        // A few leaves sticking out.
        return if one_in(x, y, 50, 9) && y > 2 {
            rgb(110, 180, 70)
        } else {
            CLEAR
        };
    }
    let joint = (y + x).is_multiple_of(5);
    let c = speckle(rgb(160, 210, 110), x, y, 51, 10.0);
    if joint { shift(c, -40.0) } else { c }
}

fn tall_grass(x: usize, y: usize) -> Rgba {
    // Blades of different heights.
    let height = 6 + (hash(x, 0, 52) % 10) as usize;
    if 16 - y > height || one_in(x, 0, 53, 4) {
        return CLEAR;
    }
    speckle(rgb(90, 160, 52), x, y, 54, 20.0)
}

/// A little mushroom in the bottom half of the texture.
fn mushroom(x: usize, y: usize, cap: Rgba, spots: bool) -> Rgba {
    let stem = (7..=8).contains(&x) && (12..=15).contains(&y);
    let cap_px =
        (9..=11).contains(&y) && (4..=11).contains(&x) || (y == 8 && (5..=10).contains(&x));
    if cap_px {
        if spots && one_in(x, y, 55, 4) {
            rgb(250, 248, 240)
        } else {
            speckle(cap, x, y, 56, 10.0)
        }
    } else if stem {
        rgb(232, 226, 210)
    } else {
        CLEAR
    }
}

fn cake_top(x: usize, y: usize) -> Rgba {
    if one_in(x, y, 57, 13) {
        return rgb(220, 40, 60); // sprinkles of jam
    }
    speckle(rgb(250, 246, 240), x, y, 58, 5.0)
}

fn cake_side(x: usize, y: usize) -> Rgba {
    // The panel shows the bottom half: frosting on top, then sponge, a
    // stripe of jam in the middle.
    let drip = 9 + (hash(x, 0, 59) % 2) as usize;
    if y < drip {
        cake_top(x, y)
    } else if y == 12 {
        rgb(200, 40, 60)
    } else {
        speckle(rgb(196, 134, 72), x, y, 60, 8.0)
    }
}

fn compost(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(96, 70, 40), x, y, 62, 18.0);
    if one_in(x, y, 63, 7) {
        rgb(80, 120, 40)
    } else {
        c
    }
}

fn composter_side(x: usize, y: usize) -> Rgba {
    // Planks with gaps between the slats.
    if y % 4 == 3 {
        return shift(planks(x, y), -60.0);
    }
    planks(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plants_grow_taller_and_have_gaps() {
        let filled = |stage| {
            (0..16)
                .flat_map(|y| (0..16).map(move |x| (x, y)))
                .filter(|(x, y)| wheat(*x, *y, stage, 7)[3] > 0)
                .count()
        };
        assert!(filled(7) > filled(0));
        assert!(filled(7) < 256, "see-through between the stalks");
        assert!(pixel("carrots_2", 8, 15).is_some());
        assert!(pixel("nonsense", 0, 0).is_none());
    }
}

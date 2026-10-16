//! Wood textures (milestone 19): planks, logs, leaves, doors and trapdoors
//! for each wood (the oak ones recoloured), birch's white bark, and the
//! saplings.

use super::textures::{
    Rgba, door_lower, door_upper, hash, leaves, log_side, log_top, one_in, planks, rgb, shift,
    speckle, trapdoor,
};

const CLEAR: Rgba = [0, 0, 0, 0];

/// A wood's colours: planks, bark, leaves.
fn colours(wood: &str) -> Option<(Rgba, Rgba, Rgba)> {
    Some(match wood {
        "oak" => (rgb(182, 140, 88), rgb(105, 78, 48), rgb(60, 128, 44)),
        "birch" => (rgb(222, 204, 152), rgb(226, 224, 214), rgb(120, 170, 70)),
        "pine" => (rgb(124, 90, 54), rgb(70, 50, 32), rgb(46, 96, 60)),
        "jungle" => (rgb(180, 122, 84), rgb(96, 82, 42), rgb(42, 150, 32)),
        "cherry" => (rgb(228, 172, 162), rgb(72, 42, 48), rgb(240, 168, 200)),
        "dark" => (rgb(82, 56, 32), rgb(52, 38, 24), rgb(38, 88, 30)),
        _ => return None,
    })
}

/// The colour of a wood's planks (boats use it).
pub fn wood_colour(wood: &str) -> [u8; 4] {
    colours(wood).map_or([182, 140, 88, 255], |c| c.0)
}

/// Make a pixel drawn in oak's colours a pixel in another colour, keeping
/// its light and shade.
fn recolour(px: Rgba, from: Rgba, to: Rgba) -> Rgba {
    let f = |i: usize| ((px[i] as u32 * to[i] as u32) / from[i].max(1) as u32).min(255) as u8;
    [f(0), f(1), f(2), px[3]]
}

pub fn pixel(name: &str, x: usize, y: usize) -> Option<Rgba> {
    let (part, wood) = name.rsplit_once('_')?;
    let (planks_c, bark, leaf) = colours(wood)?;
    let (oak_planks, oak_bark, oak_leaf) = colours("oak").expect("oak");
    Some(match part {
        "planks" => recolour(planks(x, y), oak_planks, planks_c),
        "log_side" if wood == "birch" => birch_bark(x, y),
        "log_side" => recolour(log_side(x, y), oak_bark, bark),
        "log_top" => {
            let d = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
            if d > 7.0 && wood == "birch" {
                birch_bark(x, y)
            } else if d > 7.0 {
                recolour(log_side(x, y), oak_bark, bark)
            } else {
                recolour(log_top(x, y), oak_planks, planks_c)
            }
        }
        "leaves" => recolour(leaves(x, y), oak_leaf, leaf),
        "door_lower" => recolour(door_lower(x, y), oak_planks, planks_c),
        "door_upper" => recolour(door_upper(x, y), oak_planks, planks_c),
        "trapdoor" => recolour(trapdoor(x, y), oak_planks, planks_c),
        "sapling" => sapling(x, y, bark, leaf),
        _ => return None,
    })
}

/// White bark with dark marks.
fn birch_bark(x: usize, y: usize) -> Rgba {
    let mark = hash(x / 3, y, 80).is_multiple_of(7) && !y.is_multiple_of(3);
    if mark {
        rgb(40, 40, 36)
    } else {
        speckle(rgb(226, 224, 214), x, y, 81, 8.0)
    }
}

/// A little tree: a stem with a tuft of leaves (the panels are 14 tall,
/// so rows 2–15 show).
fn sapling(x: usize, y: usize, bark: Rgba, leaf: Rgba) -> Rgba {
    let stem = (7..=8).contains(&x) && y >= 10;
    let (cx, cy) = (x as i32 - 7, y as i32 - 7);
    let tuft = cx * cx + cy * cy * 2 <= 18 && y >= 3 && !one_in(x, y, 82, 5);
    if tuft {
        speckle(leaf, x, y, 83, 20.0)
    } else if stem {
        shift(bark, -10.0)
    } else {
        CLEAR
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wood_is_drawn() {
        for w in ourcraft_common::woods::Wood::ALL {
            for part in [
                "planks",
                "log_side",
                "log_top",
                "leaves",
                "door_lower",
                "door_upper",
                "trapdoor",
                "sapling",
            ] {
                assert!(
                    pixel(&format!("{part}_{}", w.key()), 4, 4).is_some(),
                    "{part} {w:?}"
                );
            }
        }
        // Birch planks are paler than dark planks.
        let sum = |p: Rgba| p[0] as u32 + p[1] as u32 + p[2] as u32;
        assert!(
            sum(pixel("planks_birch", 5, 5).unwrap()) > sum(pixel("planks_dark", 5, 5).unwrap())
        );
    }
}

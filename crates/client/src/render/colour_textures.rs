//! Colour textures (milestone 18): dyed wool, stained glass, clay tiles and
//! beds in sixteen colours, and the meadow flowers.

use ourcraft_common::colours::COLOURS;

use super::textures::{Rgba, hash, one_in, planks, rgb, shift, speckle, wool};

const CLEAR: Rgba = [0, 0, 0, 0];

/// A colour by name, as RGBA.
fn colour(name: &str) -> Option<Rgba> {
    COLOURS
        .iter()
        .find(|c| c.0 == name)
        .map(|c| [c.2[0], c.2[1], c.2[2], 255])
}

/// Paint a white-ish pixel in a colour, keeping its light and shade.
fn tint(base: Rgba, c: Rgba) -> Rgba {
    let f = |i: usize| ((base[i] as u32 * c[i] as u32) / 236).min(255) as u8;
    [f(0), f(1), f(2), base[3]]
}

pub fn pixel(name: &str, x: usize, y: usize) -> Option<Rgba> {
    let coloured = |prefix: &str| name.strip_prefix(prefix).and_then(colour);
    if let Some(c) = coloured("wool_") {
        return Some(tint(wool(x, y), c));
    }
    if let Some(c) = coloured("stained_glass_") {
        return Some(stained_glass(x, y, c));
    }
    if let Some(c) = coloured("clay_tiles_") {
        return Some(clay_tiles(x, y, c));
    }
    if let Some(c) = coloured("bed_head_") {
        return Some(bed_head(x, y, c));
    }
    if let Some(c) = coloured("bed_foot_") {
        return Some(bed_foot(x, y, c));
    }
    if let Some(c) = coloured("bed_side_") {
        return Some(bed_side(x, y, c));
    }
    flower(name, x, y)
}

fn stained_glass(x: usize, y: usize, c: Rgba) -> Rgba {
    let edge = x == 0 || y == 0 || x == 15 || y == 15;
    if edge {
        return shift(c, -30.0);
    }
    // A glint across the corner.
    let glint = x + y == 5 || x + y == 7;
    let mut p = if glint { shift(c, 60.0) } else { c };
    p[3] = if glint { 180 } else { 120 };
    p
}

fn clay_tiles(x: usize, y: usize, c: Rgba) -> Rgba {
    // Four tiles with grout lines between them.
    if x % 8 == 7 || y % 8 == 7 {
        return shift(c, -45.0);
    }
    speckle(c, x, y, 70, 9.0)
}

fn bed_head(x: usize, y: usize, c: Rgba) -> Rgba {
    if (3..13).contains(&x) && (4..12).contains(&y) {
        speckle(rgb(244, 244, 240), x, y, 65, 5.0) // the pillow
    } else {
        speckle(c, x, y, 66, 8.0)
    }
}

fn bed_foot(x: usize, y: usize, c: Rgba) -> Rgba {
    let p = speckle(c, x, y, 66, 8.0);
    if y % 6 == 2 { shift(p, 25.0) } else { p }
}

fn bed_side(x: usize, y: usize, c: Rgba) -> Rgba {
    if y < 7 {
        speckle(c, x, y, 67, 8.0)
    } else {
        planks(x, y)
    }
}

/// A flower: a stem with leaves, and a flower head at the top. The panels
/// are 12 tall, so rows 4–15 show.
fn flower(name: &str, x: usize, y: usize) -> Option<Rgba> {
    let (petal, middle, kind) = match name {
        "dandelion" => (rgb(250, 214, 40), rgb(230, 170, 20), 0),
        "poppy" => (rgb(220, 40, 40), rgb(40, 30, 30), 0),
        "cornflower" => (rgb(70, 100, 230), rgb(40, 50, 160), 1),
        "daisy" => (rgb(246, 246, 240), rgb(240, 200, 40), 0),
        "pink_tulip" => (rgb(240, 150, 190), rgb(220, 110, 160), 2),
        "orange_tulip" => (rgb(246, 140, 40), rgb(220, 100, 20), 2),
        "allium" => (rgb(190, 110, 230), rgb(150, 70, 200), 3),
        "forget_me_not" => (rgb(130, 190, 250), rgb(250, 230, 120), 1),
        _ => return None,
    };
    let green = rgb(60, 150, 50);
    let (cx, cy) = (x as i32 - 7, y as i32 - 7);
    let head = match kind {
        // A round flower with a middle.
        0 => {
            let d = cx * cx + cy * cy;
            if d <= 1 {
                Some(middle)
            } else if d <= 6 {
                Some(petal)
            } else {
                None
            }
        }
        // A spiky, star-ish one.
        1 => {
            let d = cx.abs() + cy.abs();
            if d == 0 {
                Some(middle)
            } else if d <= 3 && (cx == 0 || cy == 0 || cx.abs() == cy.abs()) {
                Some(petal)
            } else {
                None
            }
        }
        // A tulip cup.
        2 => {
            let cup = (5..=9).contains(&y) && cx.abs() <= 2 - (y < 6) as i32;
            let tips = y == 4 && (cx == -2 || cx == 0 || cx == 2);
            (cup || tips).then(|| if one_in(x, y, 71, 4) { middle } else { petal })
        }
        // A ball of little flowers, high up.
        _ => {
            let d = cx * cx + (cy + 1) * (cy + 1);
            (d <= 7).then(|| {
                if (x + y).is_multiple_of(2) {
                    petal
                } else {
                    middle
                }
            })
        }
    };
    if let Some(h) = head.filter(|_| y >= 3) {
        return Some(speckle(h, x, y, 72, 8.0));
    }
    let stem = x == 7 && y >= 9;
    let leaf = (y == 12 && (5..=6).contains(&x)) || (y == 11 && (8..=9).contains(&x));
    Some(if stem || leaf {
        shift(green, (hash(x, y, 73) % 20) as f32 - 10.0)
    } else {
        CLEAR
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_colour_and_flower_is_drawn() {
        for (c, _, _) in COLOURS {
            for prefix in [
                "wool_",
                "stained_glass_",
                "clay_tiles_",
                "bed_head_",
                "bed_foot_",
                "bed_side_",
            ] {
                assert!(
                    pixel(&format!("{prefix}{c}"), 3, 3).is_some(),
                    "{prefix}{c}"
                );
            }
        }
        for (f, _, _) in ourcraft_common::colours::FLOWERS {
            let name = f.trim_start_matches("core:");
            let filled = (0..16)
                .flat_map(|y| (0..16).map(move |x| (x, y)))
                .filter(|(x, y)| pixel(name, *x, *y).is_some_and(|p| p[3] > 0))
                .count();
            assert!(filled > 6, "{name} is (nearly) empty");
        }
        // See-through glass.
        assert!(pixel("stained_glass_blue", 8, 8).unwrap()[3] < 255);
    }
}

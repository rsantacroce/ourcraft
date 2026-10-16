//! Stone textures (milestone 19): smooth stone, stone bricks, sandstone,
//! granite, diorite, andesite, deep slate, obsidian and ice.

use super::textures::{Rgba, cobblestone, hash, noise, one_in, rgb, sand, shift, speckle};

/// Bricks in rows, `w` wide and four tall, in `c`, with darker mortar.
fn brick_grid(x: usize, y: usize, c: Rgba, w: usize, seed: u32) -> Rgba {
    let row = y / 4;
    let offset = if row.is_multiple_of(2) { 0 } else { w / 2 };
    if y % 4 == 3 || (x + offset) % w == w - 1 {
        return shift(c, -40.0);
    }
    let tone = (hash((x + offset) / w, row, seed) % 3) as f32 * 8.0 - 8.0;
    speckle(shift(c, tone), x, y, seed + 1, 8.0)
}

/// Green moss creeping over a pixel.
fn mossy(px: Rgba, x: usize, y: usize) -> Rgba {
    let patch = noise(x / 2, y / 2, 90) + noise(x, y, 91) * 0.4 > 0.35;
    if patch {
        speckle(rgb(76, 120, 50), x, y, 92, 18.0)
    } else {
        px
    }
}

/// A stone with spots of a lighter and a darker colour.
fn spotty(x: usize, y: usize, c: Rgba, seed: u32) -> Rgba {
    let p = speckle(c, x, y, seed, 12.0);
    if one_in(x, y, seed + 1, 6) {
        shift(p, 30.0)
    } else if one_in(x, y, seed + 2, 7) {
        shift(p, -30.0)
    } else {
        p
    }
}

/// The polished kind: smooth, with a thin frame.
fn polished(x: usize, y: usize, c: Rgba, seed: u32) -> Rgba {
    if x == 0 || y == 0 || x == 15 || y == 15 {
        return shift(c, -25.0);
    }
    speckle(c, x, y, seed, 5.0)
}

pub fn pixel(name: &str, x: usize, y: usize) -> Option<Rgba> {
    let stone_grey = rgb(128, 128, 132);
    let deep = rgb(72, 72, 80);
    let sand_c = rgb(222, 206, 150);
    Some(match name {
        "smooth_stone" => polished(x, y, rgb(156, 156, 158), 93),
        "stone_bricks" => brick_grid(x, y, stone_grey, 8, 94),
        "mossy_stone_bricks" => mossy(brick_grid(x, y, stone_grey, 8, 94), x, y),
        "cracked_stone_bricks" => {
            let crack = (x + y * 3).is_multiple_of(11) || (x * 2 + y).is_multiple_of(13);
            let b = brick_grid(x, y, stone_grey, 8, 94);
            if crack { shift(b, -45.0) } else { b }
        }
        "mossy_cobblestone" => mossy(cobblestone(x, y), x, y),
        "sandstone" => {
            let band = (y + (hash(x / 4, 0, 95) % 2) as usize).is_multiple_of(5);
            let c = sand(x, y);
            if band { shift(c, -20.0) } else { c }
        }
        "smooth_sandstone" => speckle(sand_c, x, y, 96, 4.0),
        "cut_sandstone" => {
            let line = y == 0 || y == 15 || y == 7 || x == 0 || x == 15;
            let c = speckle(sand_c, x, y, 97, 5.0);
            if line { shift(c, -25.0) } else { c }
        }
        "granite" => spotty(x, y, rgb(152, 102, 84), 100),
        "polished_granite" => polished(x, y, rgb(160, 108, 88), 101),
        "diorite" => {
            let c = speckle(rgb(206, 206, 204), x, y, 102, 10.0);
            if one_in(x, y, 103, 7) {
                rgb(60, 60, 64)
            } else {
                c
            }
        }
        "polished_diorite" => polished(x, y, rgb(212, 212, 210), 104),
        "andesite" => spotty(x, y, rgb(134, 136, 134), 105),
        "polished_andesite" => polished(x, y, rgb(140, 142, 140), 106),
        "deep_slate" => {
            let streak = (y + (hash(x / 5, 0, 107) % 3) as usize).is_multiple_of(4);
            let c = speckle(deep, x, y, 108, 8.0);
            if streak { shift(c, -15.0) } else { c }
        }
        "cobbled_deep_slate" => {
            let c = cobblestone(x, y);
            [c[0] / 2 + 8, c[1] / 2 + 8, c[2] / 2 + 14, 255]
        }
        "polished_deep_slate" => polished(x, y, rgb(82, 82, 90), 109),
        "deep_slate_bricks" => brick_grid(x, y, rgb(80, 80, 88), 8, 110),
        "obsidian" => {
            let c = speckle(rgb(24, 16, 36), x, y, 111, 8.0);
            if one_in(x, y, 112, 9) {
                rgb(90, 50, 140)
            } else {
                c
            }
        }
        "ice" => {
            let streak = (x + y).is_multiple_of(9);
            let mut c = if streak {
                rgb(240, 250, 255)
            } else {
                speckle(rgb(150, 196, 240), x, y, 113, 6.0)
            };
            c[3] = 170;
            c
        }
        "packed_ice" => {
            let c = speckle(rgb(140, 184, 236), x, y, 114, 8.0);
            if (x + y * 2).is_multiple_of(7) {
                shift(c, 30.0)
            } else {
                c
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stone_is_drawn() {
        for (_, _, tex) in ourcraft_common::stones::STONES {
            assert!(pixel(tex, 3, 3).is_some(), "{tex}");
        }
        for tex in ["deep_slate", "obsidian", "ice", "packed_ice"] {
            assert!(pixel(tex, 3, 3).is_some(), "{tex}");
        }
        assert!(pixel("ice", 3, 3).unwrap()[3] < 255, "ice is see-through");
    }
}

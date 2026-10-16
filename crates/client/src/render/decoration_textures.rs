//! Decoration textures (milestone 21): flower pots (with their plants),
//! eight paintings of our own, item frames, banners, lanterns, candles,
//! chains, iron bars, bookshelves, lecterns, barrels and scaffolding.

use ourcraft_common::colours::COLOURS;

use super::textures::{Rgba, hash, one_in, planks, rgb, shift, speckle, texture_pixel};

const CLEAR: Rgba = [0, 0, 0, 0];

pub fn pixel(name: &str, x: usize, y: usize) -> Option<Rgba> {
    if let Some(plant) = name.strip_prefix("potted_") {
        // The pot shows in the bottom six rows, the plant above it.
        return Some(if y >= 10 {
            pot_side(x, y)
        } else {
            texture_pixel(plant, x, y + 2).unwrap_or(CLEAR)
        });
    }
    if let Some(key) = name.strip_prefix("painting_") {
        return painting(key, x, y);
    }
    if let Some(c) = name.strip_prefix("banner_") {
        let c = COLOURS.iter().find(|k| k.0 == c)?.2;
        let cloth = rgb(c[0], c[1], c[2]);
        // A darker hem at the bottom and a stripe near the top.
        return Some(if y >= 14 || y == 2 {
            shift(cloth, -40.0)
        } else {
            speckle(cloth, x, y, 120, 6.0)
        });
    }
    Some(match name {
        "pot_side" => pot_side(x, y),
        "pot_top" => {
            if (5..11).contains(&x) && (5..11).contains(&y) {
                speckle(rgb(96, 66, 40), x, y, 121, 10.0) // soil
            } else {
                pot_side(x, y)
            }
        }
        "item_frame" => {
            let edge = x < 2 || y < 2 || x > 13 || y > 13;
            if edge {
                planks(x, y)
            } else {
                speckle(rgb(120, 86, 60), x, y, 122, 6.0)
            }
        }
        "lantern" => {
            let frame = x <= 1 || x >= 14 || y <= 1 || y >= 14 || x == 7 || x == 8;
            if frame {
                rgb(60, 62, 70)
            } else {
                speckle(rgb(255, 200, 90), x, y, 123, 20.0)
            }
        }
        "lantern_top" => rgb(60, 62, 70),
        "candle" => {
            if y <= 4 && (7..=8).contains(&x) {
                if y <= 2 {
                    rgb(255, 210, 80)
                } else {
                    rgb(255, 140, 40)
                }
            } else if y > 6 {
                speckle(rgb(244, 236, 214), x, y, 124, 6.0)
            } else {
                CLEAR
            }
        }
        "candle_top" => rgb(244, 236, 214),
        "chain" => {
            let link = (7..=8).contains(&x) && y % 4 != 3 || (y % 4 == 3 && (6..=9).contains(&x));
            if link { rgb(80, 82, 92) } else { CLEAR }
        }
        "iron_bars" => {
            let bar = x % 5 == 2 || y == 1 || y == 14;
            if bar {
                speckle(rgb(120, 122, 130), x, y, 125, 10.0)
            } else {
                CLEAR
            }
        }
        "bookshelf" => {
            if y <= 1 || y >= 14 || y == 7 || y == 8 {
                planks(x, y)
            } else {
                // Book spines in different colours.
                let book = x / 2;
                let c = COLOURS[(hash(book, y / 8, 126) % 16) as usize].2;
                let p = rgb(c[0], c[1], c[2]);
                if x % 2 == 1 { shift(p, -30.0) } else { p }
            }
        }
        "lectern_top" => {
            let page = (3..13).contains(&x) && (4..12).contains(&y);
            if page {
                if y.is_multiple_of(2) && x != 8 {
                    rgb(150, 150, 160)
                } else {
                    rgb(246, 242, 228)
                }
            } else {
                planks(x, y)
            }
        }
        "barrel_top" => {
            let d = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
            if d > 6.5 {
                rgb(70, 72, 80)
            } else {
                shift(planks(x, y), -20.0)
            }
        }
        "barrel_side" => {
            if y == 2 || y == 13 {
                rgb(70, 72, 80) // iron bands
            } else {
                let c = shift(planks(y, x), -15.0);
                if x.is_multiple_of(4) {
                    shift(c, -25.0)
                } else {
                    c
                }
            }
        }
        "beehive_top" => {
            let ring =
                ((x as f32 - 7.5).abs().max((y as f32 - 7.5).abs()) as usize).is_multiple_of(3);
            let c = speckle(rgb(222, 180, 90), x, y, 132, 8.0);
            if ring { shift(c, -30.0) } else { c }
        }
        "beehive_front" | "beehive_front_full" => {
            let hole = (6..10).contains(&x) && (8..11).contains(&y);
            let full = name.ends_with("_full");
            if hole {
                if full {
                    rgb(250, 190, 40)
                } else {
                    rgb(40, 30, 20)
                }
            } else if full && one_in(x, y, 133, 7) {
                rgb(250, 190, 40) // honey oozing
            } else {
                let c = speckle(rgb(214, 170, 84), x, y, 134, 8.0);
                if y.is_multiple_of(4) {
                    shift(c, -25.0)
                } else {
                    c
                }
            }
        }
        // A wish table: a purple cloth with a gold star on top, and
        // dark stone with glinting runes round the sides.
        "wish_table_top" => {
            let (fx, fy) = (x as f32 - 7.5, y as f32 - 7.5);
            let r = (fx * fx + fy * fy).sqrt();
            let a = fy.atan2(fx);
            let star = r < 2.0 + 2.5 * (a * 5.0).cos().max(0.0).powi(3);
            if x == 0 || y == 0 || x == 15 || y == 15 {
                rgb(36, 26, 48)
            } else if star {
                rgb(255, 214, 70)
            } else {
                speckle(rgb(120, 60, 170), x, y, 135, 10.0)
            }
        }
        "wish_table_side" => {
            if y <= 2 {
                speckle(rgb(120, 60, 170), x, y, 136, 10.0)
            } else if y > 4 && y < 10 && x % 4 == 1 && one_in(x, y, 137, 2) {
                rgb(200, 150, 255) // a glowing rune
            } else {
                speckle(rgb(36, 26, 48), x, y, 138, 8.0)
            }
        }
        "scaffolding" => {
            let frame = x <= 1 || x >= 14 || y <= 1 || y >= 14;
            let cross = x == y || x + y == 15;
            if frame || cross {
                speckle(rgb(200, 170, 100), x, y, 127, 10.0)
            } else {
                CLEAR
            }
        }
        _ => return None,
    })
}

fn pot_side(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(170, 90, 60), x, y, 128, 10.0);
    if y == 10 { shift(c, 25.0) } else { c }
}

/// Our own little pictures, in a wooden frame.
fn painting(key: &str, x: usize, y: usize) -> Option<Rgba> {
    if x == 0 || y == 0 || x == 15 || y == 15 {
        return Some(rgb(110, 76, 40));
    }
    let (fx, fy) = (x as f32, y as f32);
    Some(match key {
        "sunset" => {
            let sun = (fx - 8.0).powi(2) + (fy - 10.0).powi(2) < 9.0;
            if y >= 11 {
                rgb(40, 60, 110) // the sea
            } else if sun {
                rgb(255, 220, 90)
            } else {
                let t = y as u8 * 12;
                rgb(250, 140u8.saturating_sub(t / 2), 60 + t / 3)
            }
        }
        "mountain" => {
            let peak = 13.0 - (8.0 - (fx - 7.5).abs() * 1.3);
            if fy > peak {
                if fy < peak + 2.0 {
                    rgb(250, 250, 250)
                } else {
                    rgb(110, 110, 120)
                }
            } else {
                rgb(140, 190, 240)
            }
        }
        "sea" => {
            let wave = ((fx * 0.8).sin() * 1.5 + 8.0) as usize;
            if y > wave {
                speckle(rgb(40, 90, 180), x, y, 129, 14.0)
            } else if y == wave {
                rgb(240, 250, 255)
            } else {
                rgb(160, 210, 250)
            }
        }
        "flower" => {
            let d = (fx - 8.0).powi(2) + (fy - 6.0).powi(2);
            if d < 2.5 {
                rgb(250, 210, 50)
            } else if d < 12.0 {
                rgb(240, 120, 170)
            } else if x == 8 && y > 8 {
                rgb(60, 150, 50)
            } else {
                rgb(250, 244, 220)
            }
        }
        "cat" => {
            let face = (fx - 8.0).powi(2) / 1.2 + (fy - 9.0).powi(2) < 22.0;
            let ear = (y == 3 || y == 4) && (x == 4 || x == 5 || x == 11 || x == 12);
            let eye = y == 8 && (x == 6 || x == 10);
            if eye {
                rgb(40, 160, 60)
            } else if face || ear {
                rgb(240, 150, 60)
            } else {
                rgb(200, 220, 240)
            }
        }
        "house" => {
            let roof = (3..=7).contains(&y) && (fx - 8.0).abs() <= (y as f32 - 2.0) * 1.2;
            let wall = (8..14).contains(&y) && (3..13).contains(&x);
            let door = (10..14).contains(&y) && (7..9).contains(&x);
            if door {
                rgb(110, 70, 40)
            } else if roof {
                rgb(200, 50, 50)
            } else if wall {
                rgb(240, 230, 200)
            } else if y >= 14 {
                rgb(90, 170, 60)
            } else {
                rgb(150, 200, 250)
            }
        }
        "rainbow" => {
            let d = ((fx - 8.0).powi(2) + (fy - 15.0).powi(2)).sqrt() as usize;
            match d {
                10 => rgb(230, 50, 50),
                9 => rgb(250, 150, 40),
                8 => rgb(250, 230, 60),
                7 => rgb(80, 200, 80),
                6 => rgb(60, 120, 230),
                5 => rgb(140, 70, 200),
                _ => rgb(170, 215, 250),
            }
        }
        "night" => {
            let moon = (fx - 11.0).powi(2) + (fy - 4.0).powi(2) < 4.0;
            if moon {
                rgb(250, 246, 200)
            } else if one_in(x, y, 130, 9) {
                rgb(255, 255, 220)
            } else {
                speckle(rgb(20, 30, 80), x, y, 131, 10.0)
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorations_are_drawn() {
        for (key, _, _) in ourcraft_common::decoration::PAINTINGS {
            assert!(pixel(&format!("painting_{key}"), 5, 5).is_some(), "{key}");
        }
        for plant in ourcraft_common::decoration::pottable() {
            let short = plant.trim_start_matches("core:");
            // The pot's rows are always there.
            assert!(pixel(&format!("potted_{short}"), 8, 12).is_some_and(|p| p[3] == 255));
        }
        assert!(pixel("banner_blue", 3, 3).is_some());
        assert!(pixel("iron_bars", 0, 5).unwrap()[3] == 0, "bars have gaps");
    }
}

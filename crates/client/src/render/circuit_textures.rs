//! Circuit and rail textures (milestone 22): spark ore and blocks, spark
//! wire (dim and powered), levers, repeaters, spark lamps, pistons, note
//! blocks, and rails (straight, corners, powered).

use super::textures::{Rgba, hash, one_in, planks, rgb, shift, speckle, stone};

const CLEAR: Rgba = [0, 0, 0, 0];
const SPARK: Rgba = [60, 210, 255, 255];
const SPARK_DIM: Rgba = [40, 90, 120, 255];

pub fn pixel(name: &str, x: usize, y: usize) -> Option<Rgba> {
    let (cx, cy) = (x as i32 - 7, y as i32 - 7);
    Some(match name {
        "spark_ore" => {
            let c = stone(x, y);
            if one_in(x / 2, y / 2, 140, 5) {
                shift(SPARK, (hash(x, y, 141) % 40) as f32 - 20.0)
            } else {
                c
            }
        }
        "spark_block" => {
            let grid = x.is_multiple_of(4) || y.is_multiple_of(4);
            if grid {
                shift(SPARK, -60.0)
            } else {
                speckle(SPARK, x, y, 142, 18.0)
            }
        }
        // Wire: dust all over (its shape follows the way it joins).
        "spark_wire" | "spark_wire_on" => {
            let c = if name.ends_with("_on") {
                SPARK
            } else {
                SPARK_DIM
            };
            speckle(c, x, y, 143, 14.0)
        }
        "lever" | "lever_on" => {
            let knob = cx.abs() <= 1
                && (if name.ends_with("_on") {
                    cy < -3
                } else {
                    cy > 3
                });
            let base = stone(x, y);
            if knob { rgb(220, 40, 40) } else { base }
        }
        "repeater" | "repeater_on" => {
            // An arrow pointing the way the signal goes (north, the -z
            // side, is the top of the texture).
            let arrow =
                (cx.abs() <= 1 && (-5..=5).contains(&cy)) || (cy == -4 + cx.abs() && cx.abs() <= 4);
            let c = if name.ends_with("_on") {
                SPARK
            } else {
                SPARK_DIM
            };
            if arrow {
                c
            } else {
                speckle(rgb(160, 160, 164), x, y, 144, 8.0)
            }
        }
        "spark_lamp" | "spark_lamp_on" => {
            let frame = x == 0 || y == 0 || x == 15 || y == 15 || x == 7 || y == 7;
            if frame {
                rgb(90, 70, 50)
            } else if name.ends_with("_on") {
                speckle(rgb(255, 230, 140), x, y, 145, 20.0)
            } else {
                speckle(rgb(120, 100, 70), x, y, 146, 10.0)
            }
        }
        "piston_side" => {
            if y < 4 {
                planks(x, y)
            } else {
                speckle(rgb(120, 120, 124), x, y, 147, 10.0)
            }
        }
        "piston_front" | "sticky_piston_front" => {
            let edge = x == 0 || y == 0 || x == 15 || y == 15;
            if name.starts_with("sticky") && !edge && one_in(x, y, 148, 2) {
                rgb(120, 190, 90) // sticky clay
            } else if edge {
                rgb(90, 90, 96)
            } else {
                planks(x, y)
            }
        }
        // A dispenser: cobblestone with a dark round mouth.
        "dispenser_front" => {
            let d = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
            if d < 3.0 {
                rgb(24, 24, 28)
            } else if d < 4.0 {
                rgb(70, 70, 76)
            } else {
                speckle(rgb(122, 122, 128), x, y, 160, 14.0)
            }
        }
        "dispenser_top" => {
            let edge = x == 0 || y == 0 || x == 15 || y == 15;
            if edge {
                rgb(80, 80, 86)
            } else {
                speckle(rgb(122, 122, 128), x, y, 161, 14.0)
            }
        }
        "note_block" => {
            let note =
                (cx + 1).abs() <= 1 && (cy - 2).abs() <= 1 || (x == 8 && (3..=9).contains(&y));
            if note {
                rgb(30, 30, 30)
            } else {
                shift(planks(x, y), -20.0)
            }
        }
        // Rails: two iron rails on wooden sleepers.
        "rail_ns" | "rail_ew" | "powered_rail_ns" | "powered_rail_ew" | "powered_rail_ns_on"
        | "powered_rail_ew_on" => {
            let ew = name.contains("_ew");
            let (along, across) = if ew { (x, y) } else { (y, x) };
            let rail_colour = if name.starts_with("powered") {
                rgb(230, 190, 60)
            } else {
                rgb(170, 172, 180)
            };
            if across == 3 || across == 12 {
                rail_colour
            } else if name.ends_with("_on") && (across == 7 || across == 8) && along % 3 == 0 {
                SPARK
            } else if along % 4 == 1 && (1..15).contains(&across) {
                rgb(110, 76, 40)
            } else {
                CLEAR
            }
        }
        _ => return corner(name, x, y),
    })
}

/// A corner rail joining `from` and the next way clockwise (north joins
/// the top and right edges).
fn corner(name: &str, x: usize, y: usize) -> Option<Rgba> {
    let from = name.strip_prefix("rail_corner_")?;
    // Turn the north–east corner round to the one we want.
    let (mut fx, mut fy) = (x as f32 - 7.5, y as f32 - 7.5);
    for _ in 0..match from {
        "north" => 0,
        "east" => 1,
        "south" => 2,
        _ => 3,
    } {
        (fx, fy) = (fy, -fx);
    }
    // Circles around the top-right corner of the texture.
    let d = ((fx - 8.0).powi(2) + (fy + 8.0).powi(2)).sqrt();
    Some(if (d - 4.5).abs() < 0.7 || (d - 13.5).abs() < 0.7 {
        rgb(170, 172, 180)
    } else if (4.5..13.5).contains(&d) && ((fy + 8.0).atan2(fx - 8.0) * 4.0) as i32 % 2 == 0 {
        rgb(110, 76, 40)
    } else {
        CLEAR
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_textures_exist() {
        for name in [
            "spark_ore",
            "spark_block",
            "spark_wire",
            "spark_wire_on",
            "lever",
            "lever_on",
            "repeater",
            "repeater_on",
            "spark_lamp",
            "spark_lamp_on",
            "piston_side",
            "piston_front",
            "dispenser_front",
            "dispenser_top",
            "sticky_piston_front",
            "note_block",
            "rail_ns",
            "rail_ew",
            "rail_corner_north",
            "rail_corner_east",
            "rail_corner_south",
            "rail_corner_west",
            "powered_rail_ns",
            "powered_rail_ew_on",
        ] {
            assert!(pixel(name, 3, 3).is_some(), "{name}");
        }
    }
}

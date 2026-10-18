//! Textures for the Cloud Isles: minty sky grass on lilac sky soil, pale
//! skystone, fluffy clouds, glowing sunstone and the shimmering portal.

use super::textures::{Rgba, hash, noise, one_in, rgb, shift, speckle};

fn sky_grass(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(118, 212, 178), x, y, 300, 16.0);
    if one_in(x, y, 301, 7) {
        rgb(178, 242, 214)
    } else {
        c
    }
}

fn sky_soil(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(168, 140, 170), x, y, 302, 14.0);
    if one_in(x, y, 303, 9) {
        shift(c, -26.0)
    } else {
        c
    }
}

/// Mix two colours, `t` of the way from `a` to `b`.
fn blend(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let m = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8;
    [m(0), m(1), m(2), m(3)]
}

pub fn pixel(name: &str, x: usize, y: usize) -> Option<Rgba> {
    // Offsets from the middle of the tile.
    let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
    let d = (dx * dx + dy * dy).sqrt();
    Some(match name {
        "sky_grass_top" => sky_grass(x, y),
        "sky_soil" => sky_soil(x, y),
        "sky_grass_side" => {
            // Grass hangs a little way down the side, unevenly.
            let edge = 3 + (hash(x, 0, 304) % 3) as usize;
            if y < edge {
                sky_grass(x, y)
            } else {
                sky_soil(x, y)
            }
        }
        "skystone" => {
            let c = speckle(rgb(178, 188, 212), x, y, 305, 12.0);
            if (x + 2 * y).is_multiple_of(9) {
                shift(c, 18.0)
            } else if one_in(x, y, 306, 13) {
                shift(c, -30.0)
            } else {
                c
            }
        }
        "cloud" => {
            // Soft puffs: brighter in the middle of each, a little blue
            // in the shadows between them.
            let puff = noise(x / 3, y / 3, 307) * 0.6 + noise(x, y, 308) * 0.15;
            let mut c = blend(rgb(214, 222, 240), rgb(252, 252, 255), 0.6 + puff);
            c[3] = 225;
            c
        }
        "sunstone" => {
            // A little sun: a bright middle, rays, and an amber rim.
            let rim = x == 0 || y == 0 || x == 15 || y == 15;
            let ray = (dx.abs() < 1.0 || dy.abs() < 1.0 || (dx.abs() - dy.abs()).abs() < 1.0)
                && (3.5..6.5).contains(&d);
            let c = if rim {
                rgb(214, 146, 40)
            } else if d < 3.2 {
                rgb(255, 250, 206)
            } else if ray {
                rgb(255, 236, 150)
            } else {
                rgb(250, 204, 82)
            };
            speckle(c, x, y, 309, 8.0)
        }
        "cloud_portal" => {
            // Pale blue and pink, swirling round the middle.
            let angle = dy.atan2(dx);
            let swirl = (angle * 2.0 + d * 0.9).sin() * 0.5 + 0.5;
            let mut c = blend(rgb(168, 208, 255), rgb(255, 204, 238), swirl);
            if one_in(x, y, 310, 11) {
                c = rgb(255, 255, 255);
            }
            c[3] = 165;
            c
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clouds_and_portals_are_see_through() {
        for (name, solid) in [
            ("cloud", false),
            ("cloud_portal", false),
            ("sunstone", true),
            ("sky_grass_top", true),
        ] {
            let all_solid = (0..16).all(|y| (0..16).all(|x| pixel(name, x, y).unwrap()[3] == 255));
            assert_eq!(all_solid, solid, "{name}");
        }
        assert!(pixel("no_such_thing", 0, 0).is_none());
    }
}

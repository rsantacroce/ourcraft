//! Block textures, drawn by code instead of loaded from image files.
//!
//! This keeps every pixel original, means there are no asset files to manage
//! yet, and each texture is a few lines you can tweak. Mods can ship their own
//! textures as PNGs (M8).

/// Width and height of each block texture, in pixels.
pub const TEXTURE_SIZE: u32 = 16;
const N: usize = TEXTURE_SIZE as usize;

/// Red, green, blue, alpha (alpha 0 = fully see-through).
type Rgba = [u8; 4];

/// A cheap, deterministic pseudo-random number per pixel. The same inputs
/// always give the same "random" pattern, so textures never change.
fn hash(x: usize, y: usize, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

/// True for roughly one pixel in `n`.
fn one_in(x: usize, y: usize, seed: u32, n: u32) -> bool {
    hash(x, y, seed).is_multiple_of(n)
}

/// Random value in `-1.0..=1.0` for a pixel.
fn noise(x: usize, y: usize, seed: u32) -> f32 {
    (hash(x, y, seed) % 1001) as f32 / 500.0 - 1.0
}

fn rgb(r: u8, g: u8, b: u8) -> Rgba {
    [r, g, b, 255]
}

/// Brighten (positive) or darken (negative) a colour by `amount` (0–255 scale).
fn shift(c: Rgba, amount: f32) -> Rgba {
    let f = |v: u8| (v as f32 + amount).clamp(0.0, 255.0) as u8;
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

/// A colour with per-pixel random brightness variation of up to `spread`.
fn speckle(c: Rgba, x: usize, y: usize, seed: u32, spread: f32) -> Rgba {
    shift(c, noise(x, y, seed) * spread)
}

fn stone(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(128, 128, 132), x, y, 1, 14.0);
    if one_in(x, y, 2, 11) {
        shift(c, -28.0)
    } else {
        c
    }
}

fn dirt(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(134, 96, 67), x, y, 3, 16.0);
    if one_in(x, y, 4, 9) {
        shift(c, 22.0)
    } else {
        c
    }
}

fn grass_top(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(98, 168, 58), x, y, 5, 18.0);
    if one_in(x, y, 6, 7) {
        shift(c, 20.0)
    } else {
        c
    }
}

fn grass_side(x: usize, y: usize) -> Rgba {
    // Grass hangs 2–4 pixels down over the dirt, with a ragged edge.
    let depth = 2 + (hash(x, 0, 7) % 3) as usize;
    if y < depth {
        grass_top(x, y)
    } else {
        dirt(x, y)
    }
}

fn snow(x: usize, y: usize) -> Rgba {
    speckle(rgb(240, 246, 252), x, y, 20, 8.0)
}

fn snow_side(x: usize, y: usize) -> Rgba {
    let depth = 3 + (hash(x, 0, 21) % 3) as usize;
    if y < depth { snow(x, y) } else { dirt(x, y) }
}

fn sand(x: usize, y: usize) -> Rgba {
    speckle(rgb(222, 208, 160), x, y, 8, 9.0)
}

fn gravel(x: usize, y: usize) -> Rgba {
    // Pebbles of different greys.
    let tone = (hash(x / 2, y / 2, 22) % 4) as f32 * 18.0 - 27.0;
    speckle(shift(rgb(136, 128, 124), tone), x, y, 23, 10.0)
}

fn log_side(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(105, 78, 48), x, y, 9, 8.0);
    // Vertical bark grooves that wobble slightly.
    let wobble = (hash(0, y / 3, 10) % 2) as usize;
    if (x + wobble).is_multiple_of(4) {
        shift(c, -25.0)
    } else {
        c
    }
}

fn log_top(x: usize, y: usize) -> Rgba {
    let d = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
    if d > 7.0 {
        return log_side(x, y); // bark rim
    }
    let ring = ((d * 0.8) as u32).is_multiple_of(2);
    let base = if ring {
        rgb(184, 150, 98)
    } else {
        rgb(160, 126, 78)
    };
    speckle(base, x, y, 11, 5.0)
}

fn leaves(x: usize, y: usize) -> Rgba {
    // About one pixel in five is a gap you can see through.
    if one_in(x, y, 30, 5) {
        return [0, 0, 0, 0];
    }
    let c = speckle(rgb(60, 128, 44), x, y, 12, 26.0);
    if one_in(x, y, 13, 6) {
        shift(c, -40.0)
    } else {
        c
    }
}

fn planks(x: usize, y: usize) -> Rgba {
    let board = y / 4;
    // Each board has a slightly different tone and its own seam position.
    let tone = (hash(0, board, 14) % 3) as f32 * 8.0 - 8.0;
    let c = speckle(shift(rgb(182, 140, 88), tone), x, y, 15, 6.0);
    let seam_x = (hash(1, board, 16) % N as u32) as usize;
    if y % 4 == 3 || x == seam_x {
        shift(c, -35.0)
    } else {
        c
    }
}

fn cactus_side(x: usize, y: usize) -> Rgba {
    let c = speckle(rgb(76, 150, 64), x, y, 24, 10.0);
    if x % 4 == 1 {
        // Ridges with little light spines.
        if y % 3 == 0 {
            rgb(230, 230, 190)
        } else {
            shift(c, -30.0)
        }
    } else {
        c
    }
}

fn cactus_top(x: usize, y: usize) -> Rgba {
    let edge = x == 0 || y == 0 || x == N - 1 || y == N - 1;
    let c = speckle(rgb(96, 170, 80), x, y, 25, 8.0);
    if edge { shift(c, -35.0) } else { c }
}

fn bedrock(x: usize, y: usize) -> Rgba {
    let tone = (hash(x / 2, y / 2, 26) % 3) as f32 * 25.0;
    speckle(
        rgb(40 + tone as u8, 40 + tone as u8, 44 + tone as u8),
        x,
        y,
        27,
        10.0,
    )
}

fn cobblestone(x: usize, y: usize) -> Rgba {
    // Rounded stones separated by dark mortar, on an offset grid.
    let row = y / 4;
    let offset = if row % 2 == 0 { 0 } else { 3 };
    let cx = (x + offset) % 6;
    let cy = y % 4;
    let mortar = cx == 0 || cy == 0;
    let tone = (hash((x + offset) / 6, row, 28) % 3) as f32 * 12.0;
    if mortar {
        speckle(rgb(78, 78, 80), x, y, 29, 6.0)
    } else {
        speckle(shift(rgb(130, 130, 134), tone), x, y, 30, 10.0)
    }
}

fn glass(x: usize, y: usize) -> Rgba {
    let edge = x == 0 || y == 0 || x == N - 1 || y == N - 1;
    // A couple of diagonal glints.
    let glint = (x + y == 6 || x + y == 8) && (2..7).contains(&x);
    if edge {
        rgb(200, 225, 235)
    } else if glint {
        rgb(240, 250, 255)
    } else {
        [0, 0, 0, 0]
    }
}

fn bricks(x: usize, y: usize) -> Rgba {
    let row = y / 4;
    let offset = if row % 2 == 0 { 0 } else { 4 };
    let mortar = y % 4 == 3 || (x + offset) % 8 == 7;
    if mortar {
        speckle(rgb(200, 196, 186), x, y, 31, 6.0)
    } else {
        let tone = (hash((x + offset) / 8, row, 32) % 3) as f32 * 10.0 - 10.0;
        speckle(shift(rgb(170, 74, 56), tone), x, y, 33, 10.0)
    }
}

fn lamp(x: usize, y: usize) -> Rgba {
    // A warm glowing panel in a dark frame.
    let edge = x <= 1 || y <= 1 || x >= N - 2 || y >= N - 2;
    if edge {
        speckle(rgb(90, 70, 50), x, y, 34, 8.0)
    } else {
        let d = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
        shift(rgb(255, 214, 120), -d * 5.0)
    }
}

fn water(x: usize, y: usize) -> Rgba {
    // Gentle diagonal ripples; partly see-through.
    let wave = ((x as f32 * 0.8 + y as f32 * 0.4).sin() * 0.5 + 0.5) * 18.0;
    let c = speckle(shift(rgb(52, 110, 205), wave), x, y, 35, 6.0);
    [c[0], c[1], c[2], 170]
}

/// Magenta and black checkerboard: impossible to miss in-game.
fn missing(x: usize, y: usize) -> Rgba {
    if (x / 8 + y / 8).is_multiple_of(2) {
        rgb(255, 0, 255)
    } else {
        rgb(0, 0, 0)
    }
}

/// Look up a built-in texture's pixel function by name.
fn builtin(name: &str) -> Option<fn(usize, usize) -> Rgba> {
    Some(match name {
        "stone" => stone,
        "dirt" => dirt,
        "grass_top" => grass_top,
        "grass_side" => grass_side,
        "snow" => snow,
        "snow_side" => snow_side,
        "sand" => sand,
        "gravel" => gravel,
        "log_side" => log_side,
        "log_top" => log_top,
        "leaves" => leaves,
        "planks" => planks,
        "cactus_side" => cactus_side,
        "cactus_top" => cactus_top,
        "bedrock" => bedrock,
        "cobblestone" => cobblestone,
        "glass" => glass,
        "bricks" => bricks,
        "lamp" => lamp,
        "water" => water,
        _ => return None,
    })
}

/// True if `name` is one of the built-in textures.
pub fn is_builtin(name: &str) -> bool {
    builtin(name).is_some()
}

/// Generate the RGBA pixels (row by row, top row first) for a named texture.
/// Unknown names get the magenta "missing" texture.
pub fn generate_texture(name: &str) -> Vec<u8> {
    let pixel = builtin(name).unwrap_or(missing);
    let mut out = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            out.extend_from_slice(&pixel(x, y));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ourcraft_common::block::BlockRegistry;

    #[test]
    fn textures_have_the_right_size_and_are_deterministic() {
        let a = generate_texture("stone");
        assert_eq!(a.len(), N * N * 4);
        assert_eq!(a, generate_texture("stone"));
        assert_ne!(a, generate_texture("dirt"));
    }

    #[test]
    fn unknown_names_get_the_missing_texture() {
        assert_eq!(
            generate_texture("no_such_texture"),
            generate_texture("missing")
        );
    }

    #[test]
    fn every_builtin_block_has_a_texture() {
        let reg = BlockRegistry::with_builtin_blocks();
        for (_, def) in reg.iter() {
            if let Some(t) = &def.textures {
                for name in [&t.top, &t.bottom, &t.side] {
                    assert!(is_builtin(name), "{} uses missing texture {name}", def.name);
                }
            }
        }
    }

    #[test]
    fn leaves_have_holes() {
        let px = generate_texture("leaves");
        assert!(px.chunks(4).any(|p| p[3] == 0));
        assert!(generate_texture("stone").chunks(4).all(|p| p[3] == 255));
    }
}

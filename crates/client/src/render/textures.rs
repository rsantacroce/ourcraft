//! Block textures, drawn by code instead of loaded from image files.
//!
//! This keeps every pixel original, means there are no asset files to manage
//! yet, and each texture is a few lines you can tweak. Real image files (and
//! textures from mods) can replace these later.

/// Width and height of each block texture, in pixels.
pub const TEXTURE_SIZE: u32 = 16;
const N: usize = TEXTURE_SIZE as usize;

type Rgb = [u8; 3];

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

/// Random value in `-1.0..=1.0` for a pixel.
fn noise(x: usize, y: usize, seed: u32) -> f32 {
    (hash(x, y, seed) % 1001) as f32 / 500.0 - 1.0
}

/// Brighten (positive) or darken (negative) a colour by `amount` (0–255 scale).
fn shift(c: Rgb, amount: f32) -> Rgb {
    c.map(|v| (v as f32 + amount).clamp(0.0, 255.0) as u8)
}

/// A colour with per-pixel random brightness variation of up to `spread`.
fn speckle(c: Rgb, x: usize, y: usize, seed: u32, spread: f32) -> Rgb {
    shift(c, noise(x, y, seed) * spread)
}

fn stone(x: usize, y: usize) -> Rgb {
    let c = speckle([128, 128, 132], x, y, 1, 14.0);
    // Occasional darker flecks.
    if hash(x, y, 2).is_multiple_of(11) {
        shift(c, -28.0)
    } else {
        c
    }
}

fn dirt(x: usize, y: usize) -> Rgb {
    let c = speckle([134, 96, 67], x, y, 3, 16.0);
    if hash(x, y, 4).is_multiple_of(9) {
        shift(c, 22.0)
    } else {
        c
    }
}

fn grass_top(x: usize, y: usize) -> Rgb {
    let c = speckle([98, 168, 58], x, y, 5, 18.0);
    if hash(x, y, 6).is_multiple_of(7) {
        shift(c, 20.0)
    } else {
        c
    }
}

fn grass_side(x: usize, y: usize) -> Rgb {
    // Grass hangs 2–4 pixels down over the dirt, with a ragged edge.
    let depth = 2 + (hash(x, 0, 7) % 3) as usize;
    if y < depth {
        grass_top(x, y)
    } else {
        dirt(x, y)
    }
}

fn sand(x: usize, y: usize) -> Rgb {
    speckle([222, 208, 160], x, y, 8, 9.0)
}

fn log_side(x: usize, y: usize) -> Rgb {
    let c = speckle([105, 78, 48], x, y, 9, 8.0);
    // Vertical bark grooves that wobble slightly.
    let wobble = (hash(0, y / 3, 10) % 2) as usize;
    if (x + wobble).is_multiple_of(4) {
        shift(c, -25.0)
    } else {
        c
    }
}

fn log_top(x: usize, y: usize) -> Rgb {
    let d = ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)).sqrt();
    if d > 7.0 {
        return log_side(x, y); // bark rim
    }
    let ring = ((d * 0.8) as u32).is_multiple_of(2);
    let base = if ring { [184, 150, 98] } else { [160, 126, 78] };
    speckle(base, x, y, 11, 5.0)
}

fn leaves(x: usize, y: usize) -> Rgb {
    let c = speckle([60, 128, 44], x, y, 12, 26.0);
    if hash(x, y, 13).is_multiple_of(6) {
        shift(c, -40.0)
    } else {
        c
    }
}

fn planks(x: usize, y: usize) -> Rgb {
    let board = y / 4;
    // Each board has a slightly different tone and its own seam position.
    let tone = (hash(0, board, 14) % 3) as f32 * 8.0 - 8.0;
    let c = speckle(shift([182, 140, 88], tone), x, y, 15, 6.0);
    let seam_x = (hash(1, board, 16) % N as u32) as usize;
    if y % 4 == 3 || x == seam_x {
        shift(c, -35.0)
    } else {
        c
    }
}

/// Magenta and black checkerboard: impossible to miss in-game.
fn missing(x: usize, y: usize) -> Rgb {
    if (x / 8 + y / 8).is_multiple_of(2) {
        [255, 0, 255]
    } else {
        [0, 0, 0]
    }
}

/// Generate the RGBA pixels (row by row, top row first) for a named texture.
pub fn generate_texture(name: &str) -> Vec<u8> {
    let pixel: fn(usize, usize) -> Rgb = match name {
        "stone" => stone,
        "dirt" => dirt,
        "grass_top" => grass_top,
        "grass_side" => grass_side,
        "sand" => sand,
        "log_side" => log_side,
        "log_top" => log_top,
        "leaves" => leaves,
        "planks" => planks,
        _ => missing,
    };
    let mut out = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let [r, g, b] = pixel(x, y);
            out.extend_from_slice(&[r, g, b, 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

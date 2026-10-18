//! Player skins: the picture wrapped around a player's blocky body.
//!
//! We use the classic 64×64 skin layout, so skin files made for other
//! blocky games work as they are. Each body part is a box, and each box's
//! six faces are cut from the picture in a fixed pattern (see
//! [`face_rects`]). The older 64×32 layout (no separate left arm and leg,
//! no outer layer except the hat) is accepted too and widened to 64×64.
//!
//! Skins travel over the network as [`NetSkin`]: either the name of one of
//! the game's own skins, or the raw pixels of a player's own picture. We
//! send pixels rather than the PNG file so nobody else has to decode a file
//! from a stranger, and the size is always exactly [`SKIN_BYTES`].

use serde::{Deserialize, Serialize};

/// Width and height of a skin picture, in pixels.
pub const SKIN_SIZE: u32 = 64;
/// Bytes in a skin picture (RGBA, row by row).
pub const SKIN_BYTES: usize = (SKIN_SIZE * SKIN_SIZE * 4) as usize;
/// The biggest skin file we'll read from disk. A 64×64 PNG is a few
/// kilobytes; anything much bigger isn't a skin.
pub const MAX_SKIN_FILE: usize = 256 * 1024;
/// Max length of a built-in skin's name.
pub const MAX_SKIN_NAME: usize = 24;

/// How a player looks, as sent over the network.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum NetSkin {
    /// One of the game's own skins, by name (like `"explorer"`).
    Builtin(String),
    /// A player's own picture: [`SKIN_BYTES`] of 64×64 RGBA pixels.
    Custom(Vec<u8>),
}

impl Default for NetSkin {
    fn default() -> Self {
        NetSkin::Builtin("explorer".into())
    }
}

impl NetSkin {
    /// Is this something we're willing to pass on? (Never trust bytes from
    /// the network: the wrong number of pixels, or a silly name, is refused.)
    pub fn is_valid(&self) -> bool {
        match self {
            NetSkin::Builtin(name) => {
                !name.is_empty()
                    && name.len() <= MAX_SKIN_NAME
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            }
            NetSkin::Custom(pixels) => pixels.len() == SKIN_BYTES,
        }
    }
}

/// Why a skin picture can't be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkinError {
    /// Not 64×64 (or the old 64×32).
    WrongSize { width: u32, height: u32 },
    /// The pixel data doesn't match the size it claims.
    BadData,
    /// The file is far too big to be a skin.
    TooBig(usize),
    /// Couldn't read or decode the file.
    Unreadable(String),
}

impl std::fmt::Display for SkinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkinError::WrongSize { width, height } => {
                write!(f, "is {width}×{height}; skins must be 64×64 (or 64×32)")
            }
            SkinError::BadData => write!(f, "has broken pixel data"),
            SkinError::TooBig(n) => write!(f, "is too big for a skin ({} KB)", n / 1024),
            SkinError::Unreadable(e) => write!(f, "can't be read ({e})"),
        }
    }
}

/// Check a decoded picture and turn it into a 64×64 skin. The old 64×32
/// layout is widened: the left arm and leg copy the right ones, like the
/// games that used it did.
pub fn skin_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, SkinError> {
    if width != SKIN_SIZE || (height != SKIN_SIZE && height != SKIN_SIZE / 2) {
        return Err(SkinError::WrongSize { width, height });
    }
    if rgba.len() != (width * height * 4) as usize {
        return Err(SkinError::BadData);
    }
    let mut skin = vec![0; SKIN_BYTES];
    skin[..rgba.len()].copy_from_slice(rgba);
    if height == SKIN_SIZE / 2 {
        // Old skins often fill the hat area with a solid colour, meaning
        // "no hat". A hat with no see-through pixel at all is dropped.
        let hat = (32, 0, 32, 16);
        if area_pixels(hat).all(|i| skin[i + 3] == 255) {
            for i in area_pixels(hat) {
                skin[i..i + 4].fill(0);
            }
        }
        // Right leg (0,16) → left leg (16,48); right arm (40,16) → left
        // arm (32,48). Each is 16×16 pixels of the picture.
        copy_area(&mut skin, (0, 16), (16, 48), (16, 16));
        copy_area(&mut skin, (40, 16), (32, 48), (16, 16));
    }
    Ok(skin)
}

/// The width and height a PNG file says it has, read from its header
/// without decoding it (so a tiny file claiming to be enormous is caught
/// before we try to unpack it). `None` if it isn't a PNG.
pub fn png_size(file: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    if file.len() < 24 || file[..8] != SIGNATURE || &file[12..16] != b"IHDR" {
        return None;
    }
    let num = |at: usize| u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]);
    Some((num(16), num(20)))
}

/// Check a skin file before decoding it: small enough, a PNG, and the
/// right size.
pub fn check_skin_file(file: &[u8]) -> Result<(), SkinError> {
    if file.len() > MAX_SKIN_FILE {
        return Err(SkinError::TooBig(file.len()));
    }
    let (width, height) =
        png_size(file).ok_or_else(|| SkinError::Unreadable("not a PNG file".into()))?;
    if width != SKIN_SIZE || (height != SKIN_SIZE && height != SKIN_SIZE / 2) {
        return Err(SkinError::WrongSize { width, height });
    }
    Ok(())
}

/// Byte offsets of every pixel in an area `(x, y, w, h)`.
fn area_pixels((x, y, w, h): (u32, u32, u32, u32)) -> impl Iterator<Item = usize> {
    (y..y + h).flat_map(move |py| (x..x + w).map(move |px| ((py * SKIN_SIZE + px) * 4) as usize))
}

fn copy_area(skin: &mut [u8], from: (u32, u32), to: (u32, u32), (w, h): (u32, u32)) {
    for dy in 0..h {
        let src = (((from.1 + dy) * SKIN_SIZE + from.0) * 4) as usize;
        let dst = (((to.1 + dy) * SKIN_SIZE + to.0) * 4) as usize;
        let len = (w * 4) as usize;
        skin.copy_within(src..src + len, dst);
    }
}

/// A body part's box in the skin picture: the top-left corner of its
/// pattern, and its width, height and depth in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkinBox {
    pub u: u32,
    pub v: u32,
    pub w: u32,
    pub h: u32,
    pub d: u32,
}

const fn skin_box(u: u32, v: u32, w: u32, h: u32, d: u32) -> SkinBox {
    SkinBox { u, v, w, h, d }
}

/// The parts of the body. "Right" means the player's own right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyPart {
    Head,
    Body,
    RightArm,
    LeftArm,
    RightLeg,
    LeftLeg,
}

impl BodyPart {
    pub const ALL: [BodyPart; 6] = [
        BodyPart::Head,
        BodyPart::Body,
        BodyPart::RightArm,
        BodyPart::LeftArm,
        BodyPart::RightLeg,
        BodyPart::LeftLeg,
    ];

    /// Where the part is drawn in the skin.
    pub const fn inner(self) -> SkinBox {
        match self {
            BodyPart::Head => skin_box(0, 0, 8, 8, 8),
            BodyPart::Body => skin_box(16, 16, 8, 12, 4),
            BodyPart::RightArm => skin_box(40, 16, 4, 12, 4),
            BodyPart::LeftArm => skin_box(32, 48, 4, 12, 4),
            BodyPart::RightLeg => skin_box(0, 16, 4, 12, 4),
            BodyPart::LeftLeg => skin_box(16, 48, 4, 12, 4),
        }
    }

    /// The outer layer worn over the part (hat, jacket, sleeves, trousers),
    /// drawn a little bigger with see-through pixels left out.
    pub const fn outer(self) -> SkinBox {
        match self {
            BodyPart::Head => skin_box(32, 0, 8, 8, 8),
            BodyPart::Body => skin_box(16, 32, 8, 12, 4),
            BodyPart::RightArm => skin_box(40, 32, 4, 12, 4),
            BodyPart::LeftArm => skin_box(48, 48, 4, 12, 4),
            BodyPart::RightLeg => skin_box(0, 32, 4, 12, 4),
            BodyPart::LeftLeg => skin_box(0, 48, 4, 12, 4),
        }
    }
}

/// A face of a box, in the order [`face_rects`] returns them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Top,
    Bottom,
    /// The player's right side.
    Right,
    Front,
    /// The player's left side.
    Left,
    Back,
}

impl Face {
    pub const ALL: [Face; 6] = [
        Face::Top,
        Face::Bottom,
        Face::Right,
        Face::Front,
        Face::Left,
        Face::Back,
    ];
}

/// Where each face of a box is in the picture, as `(x, y, w, h)` pixels.
/// The classic pattern: top and bottom side by side on the first row,
/// then right, front, left and back in a strip below them.
///
/// ```text
///         d    w    w
///      +----+----+----+
///  d   |    | top|bot |
///      +----+----+----+----+
///  h   |rght|frnt|left|back|
///      +----+----+----+----+
///        d    w    d    w
/// ```
pub fn face_rects(b: SkinBox) -> [(u32, u32, u32, u32); 6] {
    let SkinBox { u, v, w, h, d } = b;
    [
        (u + d, v, w, d),
        (u + d + w, v, w, d),
        (u, v + d, d, h),
        (u + d, v + d, w, h),
        (u + d + w, v + d, d, h),
        (u + 2 * d + w, v + d, w, h),
    ]
}

/// Set one pixel of a 64×64 skin (for drawing skins by code).
pub fn put_pixel(skin: &mut [u8], x: u32, y: u32, rgba: [u8; 4]) {
    if x < SKIN_SIZE && y < SKIN_SIZE {
        let i = ((y * SKIN_SIZE + x) * 4) as usize;
        skin[i..i + 4].copy_from_slice(&rgba);
    }
}

/// One pixel of a 64×64 skin.
pub fn get_pixel(skin: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SKIN_SIZE + x) * 4) as usize;
    [skin[i], skin[i + 1], skin[i + 2], skin[i + 3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_64_wide_skins_are_accepted() {
        assert!(skin_from_rgba(64, 64, &[0; SKIN_BYTES]).is_ok());
        assert!(skin_from_rgba(64, 32, &[0; SKIN_BYTES / 2]).is_ok());
        assert_eq!(
            skin_from_rgba(128, 128, &[0; 128 * 128 * 4]),
            Err(SkinError::WrongSize {
                width: 128,
                height: 128
            })
        );
        assert_eq!(
            skin_from_rgba(16, 16, &[0; 16 * 16 * 4]),
            Err(SkinError::WrongSize {
                width: 16,
                height: 16
            })
        );
        // Claims to be 64×64 but has too few pixels.
        assert_eq!(skin_from_rgba(64, 64, &[0; 100]), Err(SkinError::BadData));
    }

    #[test]
    fn old_skins_get_a_left_arm_and_leg() {
        let mut old = vec![0u8; SKIN_BYTES / 2];
        // Paint the right arm's front red and fill the hat solid black.
        for y in 20..32 {
            for x in 44..48 {
                put_pixel(&mut old, x, y, [255, 0, 0, 255]);
            }
        }
        for y in 0..16 {
            for x in 32..64 {
                put_pixel(&mut old, x, y, [0, 0, 0, 255]);
            }
        }
        let skin = skin_from_rgba(64, 32, &old).unwrap();
        assert_eq!(skin.len(), SKIN_BYTES);
        // The left arm's front (36,52) is now red too.
        assert_eq!(get_pixel(&skin, 36, 52), [255, 0, 0, 255]);
        // The solid hat is gone.
        assert_eq!(get_pixel(&skin, 40, 8)[3], 0);
    }

    #[test]
    fn faces_follow_the_classic_pattern() {
        let head = face_rects(BodyPart::Head.inner());
        assert_eq!(head[Face::Top as usize], (8, 0, 8, 8));
        assert_eq!(head[Face::Front as usize], (8, 8, 8, 8));
        assert_eq!(head[Face::Back as usize], (24, 8, 8, 8));
        let leg = face_rects(BodyPart::RightLeg.inner());
        assert_eq!(leg[Face::Front as usize], (4, 20, 4, 12));
        // Every face of every part fits in the picture.
        for part in BodyPart::ALL {
            for b in [part.inner(), part.outer()] {
                for (x, y, w, h) in face_rects(b) {
                    assert!(x + w <= SKIN_SIZE && y + h <= SKIN_SIZE, "{part:?}");
                }
            }
        }
    }

    /// The start of a PNG file: signature and header chunk.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut f = vec![
            0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13,
        ];
        f.extend_from_slice(b"IHDR");
        f.extend_from_slice(&width.to_be_bytes());
        f.extend_from_slice(&height.to_be_bytes());
        f.extend_from_slice(&[8, 6, 0, 0, 0]);
        f
    }

    #[test]
    fn skin_files_are_checked_before_decoding() {
        assert_eq!(png_size(&png_header(64, 64)), Some((64, 64)));
        assert!(check_skin_file(&png_header(64, 64)).is_ok());
        assert!(check_skin_file(&png_header(64, 32)).is_ok());
        // A small file claiming to be huge is refused before decoding.
        assert_eq!(
            check_skin_file(&png_header(40_000, 40_000)),
            Err(SkinError::WrongSize {
                width: 40_000,
                height: 40_000
            })
        );
        assert!(matches!(
            check_skin_file(b"GIF89a not a png at all......"),
            Err(SkinError::Unreadable(_))
        ));
        assert!(matches!(
            check_skin_file(&vec![0; MAX_SKIN_FILE + 1]),
            Err(SkinError::TooBig(_))
        ));
    }

    #[test]
    fn net_skins_are_checked() {
        assert!(NetSkin::default().is_valid());
        assert!(NetSkin::Custom(vec![0; SKIN_BYTES]).is_valid());
        assert!(!NetSkin::Custom(vec![0; 10]).is_valid());
        assert!(!NetSkin::Builtin("".into()).is_valid());
        assert!(!NetSkin::Builtin("../../etc".into()).is_valid());
        assert!(!NetSkin::Builtin("x".repeat(100)).is_valid());
    }
}

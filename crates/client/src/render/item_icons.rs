//! Icons for items that aren't blocks (sticks, coal, tools…), drawn as
//! 16×16 pixel art. Each picture is 16 strings; each character is a colour
//! from the palette (`.` is see-through).

use crate::render::textures::TEXTURE_SIZE;

type Rgba = [u8; 4];

const STICK: Rgba = [150, 104, 58, 255];
const STICK_DARK: Rgba = [96, 64, 34, 255];

/// Turn pixel art into RGBA bytes.
fn draw(rows: &[&str; 16], palette: &[(char, Rgba)]) -> Vec<u8> {
    let n = TEXTURE_SIZE as usize;
    let mut out = Vec::with_capacity(n * n * 4);
    for row in rows {
        let mut chars = row.chars();
        for _ in 0..n {
            let c = chars.next().unwrap_or('.');
            let px = palette
                .iter()
                .find(|(k, _)| *k == c)
                .map_or([0, 0, 0, 0], |(_, v)| *v);
            out.extend_from_slice(&px);
        }
    }
    out
}

fn darker(c: Rgba, by: u8) -> Rgba {
    [
        c[0].saturating_sub(by),
        c[1].saturating_sub(by),
        c[2].saturating_sub(by),
        c[3],
    ]
}

fn lighter(c: Rgba, by: u8) -> Rgba {
    [
        c[0].saturating_add(by),
        c[1].saturating_add(by),
        c[2].saturating_add(by),
        c[3],
    ]
}

const PICKAXE: [&str; 16] = [
    "................",
    ".....hhhhhh.....",
    "...hhHHHHHHhh...",
    "..hHHhhhhSsHHh..",
    "..hHh...SsS.hHh.",
    "..hh...SsS...hh.",
    "......SsS.......",
    ".....SsS........",
    "....SsS.........",
    "...SsS..........",
    "..SsS...........",
    ".SsS............",
    ".ss.............",
    "................",
    "................",
    "................",
];

const AXE: [&str; 16] = [
    "................",
    ".........hhh....",
    "........hHHHh...",
    ".......hHHHHHh..",
    "......SHHHHHHh..",
    ".....SsSHHHHh...",
    "....SsS.hhhh....",
    "...SsS..........",
    "..SsS...........",
    ".SsS............",
    ".ss.............",
    "................",
    "................",
    "................",
    "................",
    "................",
];

const SHOVEL: [&str; 16] = [
    "................",
    "...........hh...",
    "..........hHHh..",
    ".........hHHHHh.",
    "........hHHHHh..",
    ".......sHHHHh...",
    "......SsShhh....",
    ".....SsS........",
    "....SsS.........",
    "...SsS..........",
    "..SsS...........",
    ".SsS............",
    ".ss.............",
    "................",
    "................",
    "................",
];

fn tool(shape: &[&str; 16], head: Rgba) -> Vec<u8> {
    draw(
        shape,
        &[
            ('H', head),
            ('h', darker(head, 70)),
            ('S', STICK),
            ('s', STICK_DARK),
        ],
    )
}

fn ingot(c: Rgba) -> Vec<u8> {
    draw(
        &[
            "................",
            "................",
            "................",
            "................",
            "................",
            "......dddddd....",
            ".....dLLLLLLd...",
            "....dLCCCCCCLd..",
            "...dCCCCCCCCCd..",
            "..dCCCCCCCCCd...",
            "..dDDDDDDDDd....",
            "...dddddddd.....",
            "................",
            "................",
            "................",
            "................",
        ],
        &[
            ('C', c),
            ('L', lighter(c, 50)),
            ('D', darker(c, 45)),
            ('d', darker(c, 90)),
        ],
    )
}

/// The icon for a built-in item (`name` without `core:`), if we have one.
pub fn item_icon(name: &str) -> Option<Vec<u8>> {
    let tier_color = |tier: &str| -> Option<Rgba> {
        Some(match tier {
            "wood" => [176, 132, 80, 255],
            "stone" => [140, 140, 146, 255],
            "iron" => [222, 224, 230, 255],
            "diamond" => [92, 230, 222, 255],
            _ => return None,
        })
    };
    if let Some((tier, kind)) = name.split_once('_') {
        let shape = match kind {
            "pickaxe" => Some(&PICKAXE),
            "axe" => Some(&AXE),
            "shovel" => Some(&SHOVEL),
            _ => None,
        };
        if let (Some(shape), Some(c)) = (shape, tier_color(tier)) {
            return Some(tool(shape, c));
        }
    }
    Some(match name {
        "stick" => draw(
            &[
                "................",
                "................",
                "...........sS...",
                "..........SsS...",
                ".........SsS....",
                "........SsS.....",
                ".......SsS......",
                "......SsS.......",
                ".....SsS........",
                "....SsS.........",
                "...SsS..........",
                "...ss...........",
                "................",
                "................",
                "................",
                "................",
            ],
            &[('S', STICK), ('s', STICK_DARK)],
        ),
        "coal" => draw(
            &[
                "................",
                "................",
                "................",
                "......kkkk......",
                "....kkCCCCkk....",
                "...kCCLCCCCCk...",
                "...kCLLCCCCCk...",
                "..kCCCCCCDCCCk..",
                "..kCCCCCCCCDCk..",
                "..kCCCDCCCCCCk..",
                "...kCCCCCCCDk...",
                "....kkCCCCkk....",
                "......kkkk......",
                "................",
                "................",
                "................",
            ],
            &[
                ('C', [52, 52, 58, 255]),
                ('L', [110, 110, 120, 255]),
                ('D', [28, 28, 32, 255]),
                ('k', [14, 14, 16, 255]),
            ],
        ),
        "iron_ingot" => ingot([214, 216, 222, 255]),
        "gold_ingot" => ingot([250, 204, 52, 255]),
        "diamond" => draw(
            &[
                "................",
                "................",
                "................",
                ".....dddddd.....",
                "....dLLCCCCd....",
                "...dLLCCCCCCd...",
                "..dddddddddddd..",
                "...dCCCCCCCDd...",
                "....dCCCCCDd....",
                ".....dCCCDd.....",
                "......dCDd......",
                ".......dd.......",
                "................",
                "................",
                "................",
                "................",
            ],
            &[
                ('C', [92, 230, 222, 255]),
                ('L', [210, 255, 252, 255]),
                ('D', [40, 160, 160, 255]),
                ('d', [20, 90, 96, 255]),
            ],
        ),
        "clay_ball" => draw(
            &[
                "................",
                "................",
                "................",
                "................",
                "......dddd......",
                ".....dLCCCd.....",
                "....dLCCCCCd....",
                "....dCCCCCCd....",
                "....dCCCCCDd....",
                ".....dCCCDd.....",
                "......dddd......",
                "................",
                "................",
                "................",
                "................",
                "................",
            ],
            &[
                ('C', [160, 166, 182, 255]),
                ('L', [210, 214, 226, 255]),
                ('D', [120, 124, 140, 255]),
                ('d', [80, 84, 98, 255]),
            ],
        ),
        "brick" => draw(
            &[
                "................",
                "................",
                "................",
                "................",
                "................",
                "................",
                "...dddddddddd...",
                "..dLLLLLLLLLCd..",
                "..dCCCCCCCCCCd..",
                "..dCCCCCCCCCDd..",
                "..dDDDDDDDDDDd..",
                "...dddddddddd...",
                "................",
                "................",
                "................",
                "................",
            ],
            &[
                ('C', [176, 82, 60, 255]),
                ('L', [214, 120, 92, 255]),
                ('D', [130, 56, 40, 255]),
                ('d', [80, 34, 24, 255]),
            ],
        ),
        "shears" => draw(
            &[
                "................",
                "................",
                "...........bb...",
                "..........bLb...",
                ".........bLb....",
                "...bb...bLb.....",
                "...bLb.bLb......",
                "....bLbLb.......",
                ".....bXb........",
                "....bLbRR.......",
                "...bLb.RrR......",
                "..bLb..R.rR.....",
                "..bb...RrrR.....",
                "........RR......",
                "................",
                "................",
            ],
            &[
                ('L', [222, 224, 230, 255]),
                ('b', [90, 92, 100, 255]),
                ('X', [60, 60, 66, 255]),
                ('R', [214, 60, 60, 255]),
                ('r', [160, 30, 30, 255]),
            ],
        ),
        "fried_egg" => draw(
            &[
                "................",
                "................",
                "................",
                "....wwwww.......",
                "..wwWWWWWww.....",
                ".wWWWWWWWWWww...",
                ".wWWWYYYWWWWWw..",
                "wWWWYYLYYWWWWw..",
                "wWWWYYYYYWWWWWw.",
                ".wWWWYYYWWWWWw..",
                ".wWWWWWWWWWWw...",
                "..wwWWWWWWww....",
                "....wwwwww......",
                "................",
                "................",
                "................",
            ],
            &[
                ('w', [210, 200, 180, 255]),
                ('W', [252, 250, 244, 255]),
                ('Y', [250, 190, 40, 255]),
                ('L', [255, 240, 160, 255]),
            ],
        ),
        "golden_apple" => draw(
            &[
                "................",
                "........s.gg....",
                "........sgGg....",
                "........s.......",
                ".....dddsddd....",
                "....dRRRRRRRd...",
                "...dRLLRRRRRRd..",
                "...dRLRRRRRRRd..",
                "...dRRRRRRRRRd..",
                "...dRRRRRRRRDd..",
                "...dRRRRRRRDDd..",
                "....dRRRRRDDd...",
                ".....ddRRddd....",
                ".......dd.......",
                "................",
                "................",
            ],
            &[
                ('R', [250, 200, 50, 255]),
                ('L', [255, 245, 190, 255]),
                ('D', [200, 140, 20, 255]),
                ('d', [130, 90, 10, 255]),
                ('s', STICK_DARK),
                ('g', [70, 150, 50, 255]),
                ('G', [110, 190, 70, 255]),
            ],
        ),
        "torch" => draw(
            &[
                "................",
                ".......yy.......",
                "......yYYy......",
                "......YOOY......",
                "......yOOy......",
                ".......ss.......",
                ".......Ss.......",
                ".......Ss.......",
                ".......Ss.......",
                ".......Ss.......",
                ".......Ss.......",
                ".......Ss.......",
                ".......Ss.......",
                ".......Ss.......",
                "................",
                "................",
            ],
            &[
                ('y', [255, 220, 90, 255]),
                ('Y', [255, 245, 170, 255]),
                ('O', [250, 150, 40, 255]),
                ('S', STICK),
                ('s', STICK_DARK),
            ],
        ),
        "door" => draw(
            &[
                "....dddddddd....",
                "....dWWWWWWd....",
                "....dWWdWWWd....",
                "....dWWdWWWd....",
                "....dddddddd....",
                "....dPPPPPPd....",
                "....dPPPPPPd....",
                "....dPPPPPPd....",
                "....dPPPPYPd....",
                "....dPPPPPPd....",
                "....dPPPPPPd....",
                "....dPPPPPPd....",
                "....dPPPPPPd....",
                "....dPPPPPPd....",
                "....dddddddd....",
                "................",
            ],
            &[
                ('d', [100, 70, 40, 255]),
                ('P', [168, 122, 74, 255]),
                ('W', [190, 226, 240, 255]),
                ('Y', [230, 200, 80, 255]),
            ],
        ),
        "sign" => draw(
            &[
                "................",
                "................",
                ".dddddddddddddd.",
                ".dPPPPPPPPPPPPd.",
                ".dPkkkPkkPkkkPd.",
                ".dPPPPPPPPPPPPd.",
                ".dPkkPkkkkPkkPd.",
                ".dPPPPPPPPPPPPd.",
                ".dddddddddddddd.",
                ".......sS.......",
                ".......sS.......",
                ".......sS.......",
                ".......sS.......",
                ".......sS.......",
                "................",
                "................",
            ],
            &[
                ('d', [100, 70, 40, 255]),
                ('P', [190, 146, 92, 255]),
                ('k', [70, 50, 30, 255]),
                ('S', STICK),
                ('s', STICK_DARK),
            ],
        ),
        "egg" => draw(
            &[
                "................",
                "................",
                "................",
                "......dddd......",
                ".....dLLCCd.....",
                "....dLLCCCCd....",
                "....dLCCCCCd....",
                "...dCCCCCCCCd...",
                "...dCCCCCCCCd...",
                "...dCCCCCCCDd...",
                "....dCCCCCDd....",
                ".....dDDDDd.....",
                "......dddd......",
                "................",
                "................",
                "................",
            ],
            &[
                ('C', [244, 232, 208, 255]),
                ('L', [255, 252, 240, 255]),
                ('D', [214, 196, 168, 255]),
                ('d', [150, 130, 104, 255]),
            ],
        ),
        "apple" => draw(
            &[
                "................",
                "........s.gg....",
                "........sgGg....",
                "........s.......",
                ".....dddsddd....",
                "....dRRRRRRRd...",
                "...dRLLRRRRRRd..",
                "...dRLRRRRRRRd..",
                "...dRRRRRRRRRd..",
                "...dRRRRRRRRDd..",
                "...dRRRRRRRDDd..",
                "....dRRRRRDDd...",
                ".....ddRRddd....",
                ".......dd.......",
                "................",
                "................",
            ],
            &[
                ('R', [220, 40, 46, 255]),
                ('L', [255, 150, 150, 255]),
                ('D', [160, 24, 30, 255]),
                ('d', [100, 14, 20, 255]),
                ('s', STICK_DARK),
                ('g', [70, 150, 50, 255]),
                ('G', [110, 190, 70, 255]),
            ],
        ),
        "bucket" => bucket(None),
        "water_bucket" => bucket(Some(([60, 110, 230, 255], [120, 170, 255, 255]))),
        "lava_bucket" => bucket(Some(([230, 100, 20, 255], [255, 200, 60, 255]))),
        _ => return None,
    })
}

/// An iron bucket, empty or full (`fill` is the liquid's dark and light
/// colour).
fn bucket(fill: Option<(Rgba, Rgba)>) -> Vec<u8> {
    let (dark, light) = fill.unwrap_or(([70, 72, 80, 255], [90, 92, 100, 255]));
    draw(
        &[
            "................",
            "................",
            "................",
            "...bbbbbbbbbb...",
            "..bLWWWWWWWWLb..",
            "..bLWwwwwwwWLb..",
            "..bLLWWWWWWLLb..",
            "...bLLLLLLLLb...",
            "...bLLLLLLLDb...",
            "...bLLLLLLLDb...",
            "....bLLLLLDb....",
            "....bLLLLLDb....",
            "....bLLLLDDb....",
            ".....bbbbbb.....",
            "................",
            "................",
        ],
        &[
            ('b', [70, 72, 80, 255]),
            ('L', [200, 204, 212, 255]),
            ('D', [150, 154, 162, 255]),
            ('W', dark),
            ('w', light),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ourcraft_common::block::BlockRegistry;
    use ourcraft_common::item::{ItemKind, ItemRegistry};

    #[test]
    fn every_builtin_item_has_an_icon() {
        let items = ItemRegistry::build(&BlockRegistry::with_builtin_blocks(), &[]);
        for (_, def) in items.iter() {
            if matches!(def.kind, ItemKind::Block(_)) {
                continue;
            }
            let short = def.name.trim_start_matches("core:");
            let icon = item_icon(short).unwrap_or_else(|| panic!("no icon for {}", def.name));
            assert_eq!(icon.len(), 16 * 16 * 4);
            assert!(icon.chunks(4).any(|p| p[3] == 255), "{} is empty", def.name);
        }
    }
}

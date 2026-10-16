//! Icons for our own magic (milestone 24): bottles, potions, the wish star,
//! and a sparkle drawn over sparkly tools and armour.

use super::item_icons::{Rgba, darker, draw, item_icon, lighter};

const GLASS: Rgba = [210, 230, 240, 200];
const GLASS_EDGE: Rgba = [150, 180, 200, 255];
const CORK: Rgba = [170, 120, 70, 255];

/// Where the sparkles go: little four-pointed stars in the corners.
const SPARKLES: [(usize, usize); 3] = [(2, 2), (12, 4), (4, 12)];

fn bottle(liquid: Option<Rgba>) -> Vec<u8> {
    let rows = [
        "................",
        "......kkkk......",
        "......kkkk......",
        ".......ee.......",
        ".......ee.......",
        "......eGGe......",
        ".....eGGGGe.....",
        "....eGLLLLGe....",
        "...eGLLLLLLGe...",
        "...eLLLLLLLLe...",
        "...eLLLLLLLLe...",
        "...eLLLLLLLLe...",
        "....eLLLLLLe....",
        ".....eeeeee.....",
        "................",
        "................",
    ];
    let l = liquid.unwrap_or(GLASS);
    draw(
        &rows,
        &[('k', CORK), ('e', GLASS_EDGE), ('G', GLASS), ('L', l)],
    )
}

/// Sprinkle sparkles (and a little extra shine) over an icon.
fn sparkle(mut icon: Vec<u8>) -> Vec<u8> {
    let n = 16;
    for px in icon.chunks_exact_mut(4) {
        if px[3] > 0 {
            let shiny = lighter([px[0], px[1], px[2], px[3]], 20);
            // A purple-ish tint, like a wish.
            px[0] = shiny[0].saturating_add(10);
            px[1] = shiny[1];
            px[2] = shiny[2].saturating_add(30);
        }
    }
    let mut put = |x: usize, y: usize, c: Rgba| {
        if x < n && y < n {
            icon[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&c);
        }
    };
    for (x, y) in SPARKLES {
        put(x, y, [255, 255, 255, 255]);
        for (dx, dy) in [(1, 0), (0, 1)] {
            put(x + dx, y + dy, [255, 236, 140, 255]);
            put(x.wrapping_sub(dx), y.wrapping_sub(dy), [255, 236, 140, 255]);
        }
    }
    icon
}

pub fn icon(name: &str) -> Option<Vec<u8>> {
    if let Some(base) = name.strip_prefix("sparkly_") {
        return item_icon(base).map(sparkle);
    }
    let potion = |c: Rgba| sparkle_small(bottle(Some(c)));
    Some(match name {
        "name_tag" => draw(
            &[
                "................",
                "................",
                "................",
                "...........ss...",
                "..........s..s..",
                ".........s....s.",
                "....dddddddd.s..",
                "...dPPPPPPPPdd..",
                "..dPPkPPPPPPPd..",
                "..dPPPPPPPPPPd..",
                "...dPPPPPPPPd...",
                "....dddddddd....",
                "................",
                "................",
                "................",
                "................",
            ],
            &[
                ('P', [238, 226, 196, 255]),
                ('d', [150, 120, 80, 255]),
                ('k', [90, 70, 50, 255]),
                ('s', [236, 236, 230, 255]),
            ],
        ),
        "bottle" => bottle(None),
        "hop_potion" => potion([110, 220, 90, 255]),
        "glow_potion" => potion([250, 210, 70, 255]),
        "swim_potion" => potion([70, 140, 240, 255]),
        "wish_star" => {
            let c = [255, 214, 70, 255];
            draw(
                &[
                    "................",
                    ".......dd.......",
                    ".......dd.......",
                    "......dCCd......",
                    "......dCCd......",
                    "dddddDCLLCDddddd",
                    ".dCCCCLLLLCCCCd.",
                    "..dCCCCLLCCCCd..",
                    "...dCCCCCCCCd...",
                    "....dCCCCCCd....",
                    "....dCCCCCCd....",
                    "...dCCCddCCCd...",
                    "...dCCd..dCCd...",
                    "..dCd......dCd..",
                    "..dd........dd..",
                    "................",
                ],
                &[
                    ('C', c),
                    ('L', lighter(c, 60)),
                    ('D', darker(c, 40)),
                    ('d', darker(c, 110)),
                ],
            )
        }
        _ => return None,
    })
}

/// One twinkle at the top of a potion bottle.
fn sparkle_small(mut icon: Vec<u8>) -> Vec<u8> {
    for (x, y) in [(12, 3), (11, 3), (13, 3), (12, 2), (12, 4)] {
        let c = if (x, y) == (12, 3) {
            [255, 255, 255, 255]
        } else {
            [255, 236, 140, 255]
        };
        icon[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].copy_from_slice(&c);
    }
    icon
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparkly_icons_sparkle() {
        let plain = item_icon("iron_pickaxe").unwrap();
        let shiny = icon("sparkly_iron_pickaxe").unwrap();
        assert_ne!(plain, shiny);
        assert_eq!(
            &shiny[(2 * 16 + 2) * 4..(2 * 16 + 2) * 4 + 4],
            &[255, 255, 255, 255]
        );
        assert!(icon("glow_potion").is_some());
    }
}

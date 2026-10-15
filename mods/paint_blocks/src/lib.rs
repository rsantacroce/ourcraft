//! # Paint Blocks: an example Ourcraft mod
//!
//! - Adds eight brightly coloured paint blocks and a glowing rainbow lamp.
//! - Adds a `/rainbow` chat command that builds a rainbow in front of you.
//!
//! Read this file top to bottom to see how a mod works: it reacts to
//! events (`on_event`) and calls functions from `guest` to change the game.

use ourcraft_mod_api::guest::{self, Mod};
use ourcraft_mod_api::{BlockSpec, Event, EventReply, Rgba, pixel_art, speckled};

/// (block name, display name, colour)
const PAINTS: [(&str, &str, Rgba); 8] = [
    ("red", "Red Paint", [230, 57, 70, 255]),
    ("orange", "Orange Paint", [244, 140, 6, 255]),
    ("yellow", "Yellow Paint", [255, 209, 26, 255]),
    ("lime", "Lime Paint", [128, 214, 46, 255]),
    ("blue", "Blue Paint", [55, 125, 245, 255]),
    ("purple", "Purple Paint", [150, 80, 220, 255]),
    ("pink", "Pink Paint", [255, 130, 190, 255]),
    ("white", "White Paint", [245, 245, 240, 255]),
];

/// The rainbow's colours, outside to inside.
const RAINBOW: [&str; 6] = ["red", "orange", "yellow", "lime", "blue", "purple"];

/// The mod's state: just whether another mod runs a game mode (then we
/// keep quiet instead of advertising building blocks).
#[derive(Default)]
struct PaintBlocks {
    game_mode: bool,
}

impl Mod for PaintBlocks {
    fn on_event(&mut self, event: Event) -> EventReply {
        match event {
                        Event::Init { game_mode, .. } => {
                self.game_mode = game_mode.is_some();
                register_content();
                guest::log("Paint Blocks loaded: 8 paints, a rainbow lamp and /rainbow");
                EventReply::Ok
            }
                        Event::PlayerJoined { player, .. } if !self.game_mode => {
                guest::chat(
                    Some(player),
                    "Paint Blocks is here! Find the paints in your inventory (E), or type /rainbow",
                );
                EventReply::Ok
            }
            Event::Chat { player, text } if text.trim() == "/rainbow" => {
                build_rainbow(player);
                EventReply::Handled
            }
            _ => EventReply::Ok,
        }
    }
}

fn register_content() {
    for (i, (name, display, color)) in PAINTS.iter().enumerate() {
        let texture = format!("paint_{name}");
        guest::register_texture(&texture, speckled(*color, i as u32));
        guest::register_block(BlockSpec::simple(&format!("paint:{name}"), display, &texture));
    }

    // A lamp with rainbow stripes that glows in the dark.
    let rows = [
        "################",
        "#RRRRRRRRRRRRRR#",
        "#RRRRRRRRRRRRRR#",
        "#OOOOOOOOOOOOOO#",
        "#OOOOOOOOOOOOOO#",
        "#YYYYYYYYYYYYYY#",
        "#YYYYYYYYYYYYYY#",
        "#GGGGGGGGGGGGGG#",
        "#GGGGGGGGGGGGGG#",
        "#BBBBBBBBBBBBBB#",
        "#BBBBBBBBBBBBBB#",
        "#PPPPPPPPPPPPPP#",
        "#PPPPPPPPPPPPPP#",
        "#WWWWWWWWWWWWWW#",
        "#WWWWWWWWWWWWWW#",
        "################",
    ];
    let palette = [
        ('#', [80, 70, 90, 255]),
        ('R', [255, 110, 110, 255]),
        ('O', [255, 180, 90, 255]),
        ('Y', [255, 240, 120, 255]),
        ('G', [150, 240, 120, 255]),
        ('B', [120, 180, 255, 255]),
        ('P', [200, 140, 255, 255]),
        ('W', [255, 255, 255, 255]),
    ];
    guest::register_texture("rainbow_lamp", pixel_art(&rows, &palette));
    guest::register_block(BlockSpec {
        light: 15,
        ..BlockSpec::simple("paint:rainbow_lamp", "Rainbow Lamp", "rainbow_lamp")
    });
}

/// Build a rainbow arch a few blocks in front of the player.
fn build_rainbow(player: u64) {
    let Some(p) = guest::players().into_iter().find(|p| p.id == player) else {
        return;
    };
    // Flatten the look direction so the rainbow stands upright.
    let (dx, dz) = (p.look[0], p.look[2]);
    let len = (dx * dx + dz * dz).sqrt().max(1e-3);
    let (fx, fz) = (dx / len, dz / len);
    // The rainbow runs sideways across our view.
    let (sx, sz) = (-fz, fx);
    let center = [
        p.position[0] + fx * 10.0,
        p.position[1] - 1.0,
        p.position[2] + fz * 10.0,
    ];

    let ids: Vec<u16> = RAINBOW
        .iter()
        .filter_map(|c| guest::block_id(&format!("paint:{c}")))
        .collect();
    let radius = 9.0f32;
    let mut placed = 0;
    for (band, id) in ids.iter().enumerate() {
        let r = radius - band as f32;
        // Step along the half circle finely enough to leave no gaps.
        let steps = (r * 4.0) as i32;
        for s in 0..=steps {
            let a = std::f32::consts::PI * s as f32 / steps as f32;
            let side = r * a.cos();
            let up = r * a.sin();
            let pos = [
                (center[0] + sx * side).floor() as i32,
                (center[1] + up).floor() as i32,
                (center[2] + sz * side).floor() as i32,
            ];
            // Only paint into empty air, never over someone's building.
            if guest::get_block(pos) == 0 {
                guest::set_block(pos, *id);
                placed += 1;
            }
        }
    }
    guest::chat(Some(player), &format!("Here's your rainbow! ({placed} blocks)"));
}

ourcraft_mod_api::export_mod!(PaintBlocks);

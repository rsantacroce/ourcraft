//! # Snow Fun: the modding guide's tutorial mod
//!
//! Built step by step in the modding guide
//! (<https://rsantacroce.github.io/ourcraft-site/modding.html>):
//!
//! 1. Say hello when a player joins.
//! 2. Add two blocks: Snow Bricks and a glowing Ice Lantern, plus a snow
//!    cone to eat, and recipes for survival.
//! 3. Add a `/snowman` chat command that builds a snowman.
//! 4. Add a snowball thrower: `/snowballs` puts it in your hotbar, and
//!    snowballs leave snow bricks where they land.
//!
//! It stays out of the way in game-mode worlds (like Splat Squad).

use std::collections::BTreeSet;

use ourcraft_mod_api::guest::{self, Mod};
use ourcraft_mod_api::{
    BlockPos, BlockSpec, EffectKind, EntityId, EntityMotion, EntitySpec, Event, EventReply, Hit,
    ItemSpec, ModelBox, ModelSpec, PlayerId, RecipeSpec, ToolSpec, Vec3, pixel_art, speckled,
};

const SNOW_WHITE: [u8; 4] = [240, 248, 255, 255];

/// Everything the mod remembers between events.
#[derive(Default)]
struct SnowFun {
    /// Off in game-mode worlds, so we don't interfere with the game.
    active: bool,
    /// Players holding the snowball thrower.
    throwers: BTreeSet<PlayerId>,
    /// Snowballs in the air. Every mod hears about every projectile hit,
    /// so we only react to our own.
    snowballs: BTreeSet<EntityId>,
}

impl Mod for SnowFun {
    fn on_event(&mut self, event: Event) -> EventReply {
        match event {
            // Step 2: content is registered during Init, in every world, so
            // block ids stay the same everywhere.
            Event::Init { game_mode, .. } => {
                register_content();
                self.active = game_mode.is_none();
                guest::log("Snow Fun loaded");
                EventReply::Ok
            }
            _ if !self.active => EventReply::Ok,

            // Step 1: hello!
            Event::PlayerJoined { player, name } => {
                guest::chat(
                    Some(player),
                    &format!("Brrr, hi {name}! Try /snowman and /snowballs"),
                );
                EventReply::Ok
            }
            Event::PlayerLeft { player } => {
                self.throwers.remove(&player);
                EventReply::Ok
            }

            // Step 3 and 4: chat commands. `Handled` hides them from chat.
            Event::Chat { player, text } => match text.trim() {
                "/snowman" => {
                    build_snowman(player);
                    EventReply::Handled
                }
                "/snowballs" => {
                    self.toggle_thrower(player);
                    EventReply::Handled
                }
                _ => EventReply::Ok,
            },

            // Step 4: throw a snowball.
            Event::UseTool {
                player,
                tool,
                origin,
                direction,
                ..
            } if tool == "snow_fun:thrower" => {
                let start = add(origin, scale(direction, 0.6));
                if let Some(id) = guest::spawn_entity(EntitySpec {
                    model: "snow_fun:snowball".into(),
                    position: start,
                    velocity: scale(direction, 22.0),
                    motion: EntityMotion::Projectile {
                        gravity: 12.0,
                        radius: 0.2,
                        hits_players: true,
                    },
                    lifetime: 4.0,
                    owner: Some(player),
                }) {
                    self.snowballs.insert(id);
                }
                EventReply::Ok
            }

            // Step 4: a snowball landed.
            Event::ProjectileHit { entity, hit, .. } if self.snowballs.remove(&entity) => {
                match hit {
                    Hit::Block { pos, normal } => {
                        // Put a snow brick on the face it hit, if that's air.
                        let target = [pos[0] + normal[0], pos[1] + normal[1], pos[2] + normal[2]];
                        if guest::get_block(target) == 0
                            && let Some(brick) = guest::block_id("snow_fun:snow_bricks")
                        {
                            guest::set_block(target, brick);
                        }
                        guest::effect(EffectKind::Splat, center(target), SNOW_WHITE);
                    }
                    Hit::Player { at, .. } => {
                        guest::effect(EffectKind::Poof, at, SNOW_WHITE);
                        guest::sound("pop", Some(at));
                    }
                }
                EventReply::Ok
            }

            _ => EventReply::Ok,
        }
    }
}

impl SnowFun {
    fn toggle_thrower(&mut self, player: PlayerId) {
        if self.throwers.remove(&player) {
            // An empty list gives the normal block hotbar back.
            guest::set_tools(player, Vec::new());
            guest::chat(Some(player), "Snowball thrower put away");
        } else {
            self.throwers.insert(player);
            guest::set_tools(player, vec!["snow_fun:thrower".into()]);
            guest::show_title(Some(player), "Snowball time!", 2.0);
        }
    }
}

fn register_content() {
    // Snow Bricks, drawn by hand: each letter is a colour from the palette.
    let rows = [
        "AAAAAAAAAAAAAAAA",
        "BBBBBBBBBBBBBBBA",
        "BBBBBBBBBBBBBBBA",
        "BBBBBBBBBBBBBBBA",
        "AAAAAAAAAAAAAAAA",
        "BBBBBBBABBBBBBBB",
        "BBBBBBBABBBBBBBB",
        "BBBBBBBABBBBBBBB",
        "AAAAAAAAAAAAAAAA",
        "BBBBBBBBBBBBBBBA",
        "BBBBBBBBBBBBBBBA",
        "BBBBBBBBBBBBBBBA",
        "AAAAAAAAAAAAAAAA",
        "BBBBBBBABBBBBBBB",
        "BBBBBBBABBBBBBBB",
        "BBBBBBBABBBBBBBB",
    ];
    let palette = [('A', [190, 210, 230, 255]), ('B', SNOW_WHITE)];
    guest::register_texture("snow_bricks", pixel_art(&rows, &palette));
    guest::register_block(BlockSpec::simple(
        "snow_fun:snow_bricks",
        "Snow Bricks",
        "snow_bricks",
    ));

    // An icy blue lantern that glows in the dark.
    guest::register_texture("ice_lantern", speckled([150, 210, 255, 255], 7));
    guest::register_block(BlockSpec {
        light: 12,
        ..BlockSpec::simple("snow_fun:ice_lantern", "Ice Lantern", "ice_lantern")
    });

    // A recipe for survival: a snow brick on top of a piece of coal makes
    // an ice lantern (on a crafting table or in the inventory's grid).
    guest::register_recipe(RecipeSpec::Shaped {
        pattern: vec!["S".into(), "C".into()],
        key: vec![
            ('S', "snow_fun:snow_bricks".into()),
            ('C', "core:coal".into()),
        ],
        result: "snow_fun:ice_lantern".into(),
        count: 1,
    });

    // Something to eat: a snow cone, drawn like the bricks.
    let cone = [
        "................",
        "......AAAA......",
        ".....ABBBBA.....",
        "....ABBRBBBA....",
        "....ABRRRBBA....",
        "....ABBRBBBA....",
        "....CCCCCCCC....",
        ".....CDCDCDC....",
        ".....CCDCDC.....",
        "......CDCDC.....",
        "......CCDC......",
        ".......CDC......",
        ".......CC.......",
        "........C.......",
        "................",
        "................",
    ];
    let cone_colours = [
        ('A', [190, 210, 230, 255]),
        ('B', SNOW_WHITE),
        ('R', [235, 80, 110, 255]),
        ('C', [214, 160, 90, 255]),
        ('D', [170, 115, 55, 255]),
    ];
    guest::register_texture("snow_cone", pixel_art(&cone, &cone_colours));
    guest::register_item(ItemSpec {
        food: 3,
        ..ItemSpec::simple("snow_fun:snow_cone", "Snow Cone", "snow_cone")
    });
    // Any arrangement: a snow brick and an apple make two snow cones.
    guest::register_recipe(RecipeSpec::Shapeless {
        ingredients: vec!["snow_fun:snow_bricks".into(), "core:apple".into()],
        result: "snow_fun:snow_cone".into(),
        count: 2,
    });

    // What a snowball looks like in the air: one small white box.
    guest::register_model(ModelSpec {
        name: "snow_fun:snowball".into(),
        boxes: vec![ModelBox {
            min: [-0.15; 3],
            max: [0.15; 3],
            color: SNOW_WHITE,
        }],
        spin: true,
    });

    // The thrower's hotbar icon.
    let icon = [
        "................",
        "................",
        "......####......",
        ".....#WWWW#.....",
        "....#WWWWWW#....",
        "...#WWWWWWWW#...",
        "...#WWWWWWLW#...",
        "...#WWWWWWLW#...",
        "...#WWWWWLLW#...",
        "...#WWWWLLLW#...",
        "....#WWLLLW#....",
        ".....#WWWW#.....",
        "......####......",
        "................",
        "................",
        "................",
    ];
    let icon_palette = [
        ('#', [70, 110, 160, 255]),
        ('W', SNOW_WHITE),
        ('L', [180, 210, 240, 255]),
    ];
    guest::register_tool(ToolSpec {
        name: "snow_fun:thrower".into(),
        display_name: "Snowball Thrower".into(),
        icon: pixel_art(&icon, &icon_palette),
        cooldown: 0.3,
    });
}

/// Build a snowman three blocks in front of the player.
fn build_snowman(player: PlayerId) {
    let Some(p) = guest::players().into_iter().find(|p| p.id == player) else {
        return;
    };
    let (Some(brick), Some(lantern)) = (
        guest::block_id("snow_fun:snow_bricks"),
        guest::block_id("snow_fun:ice_lantern"),
    ) else {
        return;
    };
    let x = (p.position[0] + p.look[0] * 3.0).floor() as i32;
    let z = (p.position[2] + p.look[2] * 3.0).floor() as i32;
    let Some(ground) = guest::ground_height(x, z) else {
        return;
    };
    // Two snowy blocks for the body and a glowing head.
    let body: [(i32, u16); 3] = [(1, brick), (2, brick), (3, lantern)];
    let mut placed = 0;
    for (dy, block) in body {
        let pos: BlockPos = [x, ground + dy, z];
        // Only build in empty air. `try_set_block` says whether it worked
        // (it doesn't if that part of the world hasn't loaded yet).
        if guest::get_block(pos) == 0 && guest::try_set_block(pos, block) {
            placed += 1;
        }
    }
    if placed == 0 {
        guest::chat(
            Some(player),
            "No room for a snowman here. Try somewhere else!",
        );
        return;
    }
    guest::effect(EffectKind::Confetti, center([x, ground + 3, z]), SNOW_WHITE);
    guest::chat(Some(player), "Ta-da! A snowman.");
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// The middle of a block.
fn center(pos: BlockPos) -> Vec3 {
    [
        pos[0] as f32 + 0.5,
        pos[1] as f32 + 0.5,
        pos[2] as f32 + 0.5,
    ]
}

ourcraft_mod_api::export_mod!(SnowFun);

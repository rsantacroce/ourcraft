//! # The Ourcraft mod API
//!
//! Mods are WebAssembly modules the server loads. They can't touch files or
//! the network: the only thing a mod can do is talk to the game through
//! this API. That makes it safe to run mods you download.
//!
//! ## How it works
//!
//! The game and a mod exchange **messages**, which are the Rust enums in
//! this file, encoded with `postcard`:
//!
//! - The game sends the mod an [`Event`] ("a player joined", "tick", …).
//! - While handling an event, the mod can make [`HostCall`]s ("set this
//!   block", "send a chat message", …) and gets a [`HostReply`] for each.
//! - The mod answers the event with an [`EventReply`] (for example
//!   [`EventReply::Deny`] to stop a block from being broken).
//!
//! Mod authors don't deal with any of that directly: see [`guest`] for the
//! friendly functions (`guest::set_block`, `guest::chat`, …) and the
//! [`export_mod!`] macro.
//!
//! ## Versioning
//!
//! [`API_VERSION`] changes whenever a message changes in an incompatible
//! way. The game refuses to load mods built for a different version, with a
//! clear message, instead of misbehaving. New *optional* features are added
//! as new enum variants at the end, which older mods simply never use.
//!
//! ## The low-level ABI (for the curious)
//!
//! A mod module must export:
//! - `ourcraft_api_version() -> u32`
//! - `ourcraft_alloc(len: u32) -> u32`: reserve `len` bytes for the host
//!   to write a message into
//! - `ourcraft_event(ptr: u32, len: u32) -> u64`: handle an encoded
//!   [`Event`]; returns the encoded [`EventReply`] as `(ptr << 32) | len`
//!
//! and may import a single function, `ourcraft.call(ptr, len) -> u64`,
//! which takes an encoded [`HostCall`] and returns an encoded [`HostReply`]
//! the same way.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

pub mod guest;

/// Used by [`export_mod!`]; not part of the API.
#[doc(hidden)]
pub use alloc::vec::Vec as __Vec;

/// Bumped on incompatible changes. See the crate docs.
pub const API_VERSION: u32 = 1;

pub type PlayerId = u64;
pub type EntityId = u64;
/// A block type's numeric id. Look ids up by name with
/// [`HostCall::BlockId`]; they can differ between worlds.
pub type BlockId = u16;
/// A block position in the world.
pub type BlockPos = [i32; 3];
/// A point or direction in the world.
pub type Vec3 = [f32; 3];
/// Red, green, blue, alpha.
pub type Rgba = [u8; 4];

/// Size of a block texture, in pixels.
pub const TEXTURE_SIZE: usize = 16;

/// How a block is drawn.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Render {
    Opaque,
    /// Has see-through pixels (alpha 0), like leaves or glass.
    Cutout,
    /// Partly see-through, like water.
    Translucent,
}

/// A new kind of block.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BlockSpec {
    /// Unique name, `"yourmod:thing"`.
    pub name: String,
    /// Shown to players.
    pub display_name: String,
    pub render: Render,
    pub solid: bool,
    pub breakable: bool,
    /// 0–15. Anything above 0 glows in the dark.
    pub light: u8,
    /// Texture names for the top, bottom and sides.
    pub texture_top: String,
    pub texture_bottom: String,
    pub texture_side: String,
}

impl BlockSpec {
    /// A plain solid block with the same texture on every side.
    pub fn simple(name: &str, display_name: &str, texture: &str) -> Self {
        Self {
            name: name.into(),
            display_name: display_name.into(),
            render: Render::Opaque,
            solid: true,
            breakable: true,
            light: 0,
            texture_top: texture.into(),
            texture_bottom: texture.into(),
            texture_side: texture.into(),
        }
    }
}

/// A 16×16 texture, `TEXTURE_SIZE * TEXTURE_SIZE * 4` RGBA bytes, rows top
/// to bottom.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextureSpec {
    pub name: String,
    pub rgba: Vec<u8>,
}

/// One coloured box of a model, in blocks relative to the model's origin.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct ModelBox {
    pub min: Vec3,
    pub max: Vec3,
    pub color: Rgba,
}

/// A simple shape made of boxes, used to draw entities (projectiles,
/// flags, …).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModelSpec {
    pub name: String,
    pub boxes: Vec<ModelBox>,
    /// Gently spin and bob, like a pickup.
    pub spin: bool,
}

/// Something a player can hold and use (a tool or a game item). Holding it
/// and clicking sends [`Event::UseTool`] to the mod.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolSpec {
    /// `"yourmod:thing"`.
    pub name: String,
    pub display_name: String,
    /// 16×16 RGBA icon for the hotbar.
    pub icon: Vec<u8>,
    /// Seconds between uses while the button is held.
    pub cooldown: f32,
}

/// What the game knows about a player.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: String,
    /// Feet position.
    pub position: Vec3,
    pub velocity: Vec3,
    /// Where they're looking.
    pub look: Vec3,
    pub on_ground: bool,
}

/// Rules for one player, set by game-mode mods.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerRules {
    /// Multiplies walking speed (1.0 = normal).
    pub speed: f32,
    pub can_fly: bool,
    /// Can break and place blocks.
    pub can_build: bool,
    /// Tint for the player's avatar shirt, if any.
    pub color: Option<Rgba>,
    /// Shown before the player's name tag, e.g. `"[Red]"`.
    pub tag: Option<String>,
}

impl Default for PlayerRules {
    fn default() -> Self {
        Self {
            speed: 1.0,
            can_fly: true,
            can_build: true,
            color: None,
            tag: None,
        }
    }
}

/// How an entity moves.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum EntityMotion {
    /// Stays where it's put.
    Fixed,
    /// Flies with its velocity, pulled down by `gravity` (blocks/s²). Hits
    /// blocks and (if `hits_players`) players, sending
    /// [`Event::ProjectileHit`] and disappearing.
    Projectile {
        gravity: f32,
        radius: f32,
        hits_players: bool,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EntitySpec {
    /// A model registered with [`HostCall::RegisterModel`].
    pub model: String,
    pub position: Vec3,
    pub velocity: Vec3,
    pub motion: EntityMotion,
    /// Removed automatically after this many seconds (0 = never).
    pub lifetime: f32,
    /// Who made it (e.g. who fired it), if anyone.
    pub owner: Option<PlayerId>,
}

/// What a ray or projectile hit.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Block { pos: BlockPos, normal: [i32; 3] },
    Player { id: PlayerId, at: Vec3 },
}

/// A burst of particles (purely visual).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum EffectKind {
    /// Paint splat on impact.
    Splat,
    /// Celebration (wins, captures, respawns).
    Confetti,
    /// A soft puff (popping out of the game when tagged out).
    Poof,
}

/// A line of text in a player's HUD panel (top right).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HudLine {
    pub text: String,
    pub color: Rgba,
}

/// Everything a mod can ask the game to do.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum HostCall {
    /// Write to the server log.
    Log(String),

    // ---- Content: only allowed while handling `Event::Init`. ----
    RegisterBlock(BlockSpec),
    RegisterTexture(TextureSpec),
    RegisterModel(ModelSpec),
    RegisterTool(ToolSpec),

    // ---- World ----
    /// Replies `HostReply::Block` (air if not loaded).
    GetBlock(BlockPos),
    SetBlock {
        pos: BlockPos,
        block: BlockId,
    },
    /// Look up a block id by name. Replies `HostReply::Block` or `Error`.
    BlockId(String),
    /// Highest solid block at a column (loads it if needed). Replies
    /// `HostReply::Height`.
    GroundHeight {
        x: i32,
        z: i32,
    },
    /// Replies `HostReply::Time` (0 = midnight, 0.5 = noon).
    GetTimeOfDay,
    SetTimeOfDay(f32),
    /// Replies `HostReply::Ray`.
    Raycast {
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        hit_players: bool,
        ignore_player: Option<PlayerId>,
    },

    // ---- Players ----
    /// Replies `HostReply::Players`.
    GetPlayers,
    /// To one player, or everyone if `to` is `None`.
    Chat {
        to: Option<PlayerId>,
        text: String,
    },
    Teleport {
        player: PlayerId,
        position: Vec3,
    },
    SetRules {
        player: PlayerId,
        rules: PlayerRules,
    },
    /// Show a health bar (hidden when `max` is 0).
    SetHealth {
        player: PlayerId,
        health: f32,
        max: f32,
    },
    /// Replace the HUD panel for one player or everyone.
    SetHud {
        player: Option<PlayerId>,
        lines: Vec<HudLine>,
    },
    /// A big message in the middle of the screen.
    ShowTitle {
        player: Option<PlayerId>,
        text: String,
        seconds: f32,
    },
    /// Fill the player's hotbar with these tools (by name). Empty list =
    /// give the normal block hotbar back.
    SetTools {
        player: PlayerId,
        tools: Vec<String>,
    },

    // ---- Entities and effects ----
    /// Replies `HostReply::Entity`.
    SpawnEntity(EntitySpec),
    MoveEntity {
        id: EntityId,
        position: Vec3,
    },
    DespawnEntity(EntityId),
    Effect {
        kind: EffectKind,
        position: Vec3,
        color: Rgba,
    },
    /// Play a built-in sound (see the client's sound list) at a position,
    /// or for everyone if `at` is `None`.
    Sound {
        name: String,
        at: Option<Vec3>,
    },
    /// Where new players appear (a fixed spot for each world). Replies
    /// `HostReply::Position`. Handy for placing things that must be in the
    /// same place every time the server starts.
    WorldSpawn,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum HostReply {
    Ok,
    Block(BlockId),
    Height(Option<i32>),
    Time(f32),
    Players(Vec<PlayerInfo>),
    Entity(EntityId),
    Ray(Option<Hit>),
    /// The call failed; the text says why.
    Error(String),
    Position(BlockPos),
}

/// Things that happen, sent to every mod.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Event {
    /// Register your content (blocks, textures, models, tools) now.
    /// `game_mode` is the world's game mode, if it has one.
    Init {
        api_version: u32,
        game_mode: Option<String>,
    },
    /// The world is ready; game logic can start.
    Start,
    /// Sent 20 times a second.
    Tick {
        tick: u64,
        dt: f32,
    },
    PlayerJoined {
        player: PlayerId,
        name: String,
    },
    PlayerLeft {
        player: PlayerId,
    },
    /// A player wants to change a block. Reply `Deny` to stop it.
    BlockChange {
        player: PlayerId,
        pos: BlockPos,
        old: BlockId,
        new: BlockId,
    },
    /// A chat message. Reply `Handled` if it was a command for this mod
    /// (then other mods and players don't see it).
    Chat {
        player: PlayerId,
        text: String,
    },
    /// A player used a tool from [`HostCall::SetTools`]. `secondary` is the
    /// right mouse button / place action.
    UseTool {
        player: PlayerId,
        tool: String,
        origin: Vec3,
        direction: Vec3,
        secondary: bool,
    },
    /// A projectile hit something (it's already gone).
    ProjectileHit {
        entity: EntityId,
        owner: Option<PlayerId>,
        hit: Hit,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventReply {
    Ok,
    /// Stop the action (for `BlockChange`).
    Deny,
    /// The mod dealt with it (for `Chat`).
    Handled,
}

/// Encode any message.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).unwrap_or_default()
}

/// Decode any message.
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}

/// Make a 16×16 texture from "pixel art": 16 strings of 16 characters, and
/// a palette saying which colour each character means. Unknown characters
/// are transparent.
///
/// ```
/// use ourcraft_mod_api::pixel_art;
/// let rows = ["RRRRRRRRRRRRRRRR"; 16];
/// let tex = pixel_art(&rows, &[('R', [220, 40, 40, 255])]);
/// assert_eq!(&tex[0..4], &[220, 40, 40, 255]);
/// ```
pub fn pixel_art(rows: &[&str; TEXTURE_SIZE], palette: &[(char, Rgba)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(TEXTURE_SIZE * TEXTURE_SIZE * 4);
    for row in rows {
        let mut chars = row.chars();
        for _ in 0..TEXTURE_SIZE {
            let c = chars.next().unwrap_or(' ');
            let color = palette
                .iter()
                .find(|(k, _)| *k == c)
                .map_or([0, 0, 0, 0], |(_, v)| *v);
            out.extend_from_slice(&color);
        }
    }
    out
}

/// A texture of one colour with a subtle speckle, so blocks don't look flat.
pub fn speckled(color: Rgba, seed: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(TEXTURE_SIZE * TEXTURE_SIZE * 4);
    for i in 0..(TEXTURE_SIZE * TEXTURE_SIZE) as u32 {
        let mut h = i.wrapping_mul(0x9E37_79B9) ^ seed.wrapping_mul(0x85EB_CA6B);
        h ^= h >> 15;
        let shade = (h % 21) as i32 - 10;
        let f = |v: u8| (v as i32 + shade).clamp(0, 255) as u8;
        out.extend_from_slice(&[f(color[0]), f(color[1]), f(color[2]), color[3]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn messages_round_trip() {
        let calls = vec![
            HostCall::Log("hi".into()),
            HostCall::RegisterBlock(BlockSpec::simple("m:a", "A", "a")),
            HostCall::SetBlock {
                pos: [1, -2, 3],
                block: 7,
            },
            HostCall::SpawnEntity(EntitySpec {
                model: "m:ball".into(),
                position: [0.0, 1.0, 2.0],
                velocity: [3.0, 0.0, 0.0],
                motion: EntityMotion::Projectile {
                    gravity: 9.8,
                    radius: 0.2,
                    hits_players: true,
                },
                lifetime: 3.0,
                owner: Some(4),
            }),
        ];
        for c in calls {
            assert_eq!(decode::<HostCall>(&encode(&c)), Some(c));
        }
        let ev = Event::UseTool {
            player: 1,
            tool: "m:blaster".into(),
            origin: [0.0; 3],
            direction: [0.0, 0.0, -1.0],
            secondary: false,
        };
        assert_eq!(decode::<Event>(&encode(&ev)), Some(ev));
        assert_eq!(
            decode::<EventReply>(&encode(&EventReply::Deny)),
            Some(EventReply::Deny)
        );
    }

    #[test]
    fn pixel_art_maps_palette_and_transparency() {
        let mut rows = ["................"; 16];
        rows[0] = "AB..............";
        let tex = pixel_art(&rows, &[('A', [1, 2, 3, 255]), ('B', [4, 5, 6, 255])]);
        assert_eq!(tex.len(), TEXTURE_SIZE * TEXTURE_SIZE * 4);
        assert_eq!(&tex[0..8], &[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(tex[11], 0, "unknown characters are transparent");
    }

    #[test]
    fn speckled_has_right_size() {
        assert_eq!(speckled([100, 100, 100, 255], 1).len(), 1024);
    }
}

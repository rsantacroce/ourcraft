//! The messages the client and server send each other.
//!
//! Both sides depend on this one file, so they always agree on the format.
//! Messages are Rust enums encoded with `postcard`. The transport (renet)
//! takes care of splitting large messages into packets, resending lost
//! ones on reliable channels, and keeping them in order.
//!
//! **Channels.** Two lanes per direction:
//! - [`Channel::Reliable`] (ordered, guaranteed): joining, world data, block
//!   changes, chat. Column data and block changes share this lane so a
//!   change can never arrive before the column it belongs to.
//! - [`Channel::Unreliable`]: frequent movement updates. If one is lost
//!   there's a newer one right behind it, so resending would only add lag.
//!
//! **Versioning.** [`PROTOCOL_VERSION`] is mixed into the connection's
//! protocol id, so mismatched clients and servers refuse to connect instead
//! of misreading each other's messages.

use serde::{Deserialize, Serialize};

/// Bump when any message changes shape.
pub const PROTOCOL_VERSION: u64 = 13;
/// Identifies Ourcraft traffic (and its version) on the wire.
pub const PROTOCOL_ID: u64 = 0x4f55_5243_0000_0000 | PROTOCOL_VERSION; // "OURC" + version
/// Default UDP port for servers.
pub const DEFAULT_PORT: u16 = 41_700;
/// UDP port where servers announce themselves on the local network.
pub const LAN_DISCOVERY_PORT: u16 = 41_701;

/// A server's "I'm here!" broadcast on the local network, sent every
/// second so the Join screen can list it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LanBeacon {
    /// Always `PROTOCOL_ID`, so other programs' broadcasts are ignored.
    pub protocol: u64,
    pub name: String,
    pub motd: String,
    /// Game port to connect to.
    pub port: u16,
    pub players: u16,
    pub max_players: u16,
    pub game_mode: Option<String>,
}

/// Max length of a player name, in characters.
pub const MAX_NAME_LEN: usize = 16;
/// Max length of a chat message, in characters.
pub const MAX_CHAT_LEN: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Channel {
    Reliable = 0,
    Unreliable = 1,
}

impl From<Channel> for u8 {
    fn from(c: Channel) -> u8 {
        c as u8
    }
}

/// Identifies a connected player (assigned by the server).
pub type PlayerId = u64;

/// A player's movement state, as sent over the network.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct NetPlayerState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub flying: bool,
    pub on_ground: bool,
}

/// One physics tick of player input (see `prediction`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct NetInput {
    /// The client's physics tick number, counting up from 1.
    pub tick: u32,
    /// `x` = strafe right, `y` = forward, each `-1..=1`.
    pub movement: [f32; 2],
    pub jump: bool,
    pub descend: bool,
    pub sprint: bool,
    pub auto_jump: bool,
    /// Whether the player wants to be flying.
    pub fly: bool,
    pub yaw: f32,
    pub pitch: f32,
    /// The speed multiplier the client assumed (the server uses its own).
    pub speed: f32,
    /// In a boat (the server checks).
    pub boat: bool,
}

impl Default for NetInput {
    fn default() -> Self {
        Self {
            tick: 0,
            movement: [0.0; 2],
            jump: false,
            descend: false,
            sprint: false,
            auto_jump: false,
            fly: false,
            yaw: 0.0,
            pitch: 0.0,
            speed: 1.0,
            boat: false,
        }
    }
}

/// Everything mods add, sent to each client when it joins so it can draw
/// the new blocks, models and tools.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ContentPack {
    /// Every block (built-in and from mods), in id order.
    pub blocks: Vec<crate::block::BlockDef>,
    /// Textures added by mods: (name, 16×16 RGBA).
    pub textures: Vec<(String, Vec<u8>)>,
    pub models: Vec<ourcraft_mod_api::ModelSpec>,
    pub tools: Vec<ourcraft_mod_api::ToolSpec>,
    /// Every item (blocks first, same numbers), in id order.
    pub items: Vec<crate::item::ItemDef>,
    pub recipes: Vec<crate::recipes::Recipe>,
    /// Every kind of animal (built-in and from mods).
    pub mobs: Vec<crate::mobs::MobDef>,
}

/// One stack of items on the wire.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetStack {
    pub item: u16,
    pub count: u16,
    pub wear: u16,
}

impl From<crate::inventory::ItemStack> for NetStack {
    fn from(s: crate::inventory::ItemStack) -> Self {
        Self {
            item: s.item.0,
            count: s.count,
            wear: s.wear,
        }
    }
}

impl From<NetStack> for crate::inventory::ItemStack {
    fn from(s: NetStack) -> Self {
        Self {
            item: crate::item::ItemId(s.item),
            count: s.count,
            wear: s.wear,
        }
    }
}

/// One inventory slot.
pub type NetSlot = Option<NetStack>;

/// A furnace's state as shown on its screen.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NetContainer {
    pub pos: [i32; 3],
    pub kind: crate::container::ContainerKind,
    pub slots: Vec<NetSlot>,
    /// 0..1: fire left, and how far the current item has smelted.
    pub burn: f32,
    pub smelt: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ClientMessage {
    /// First message after connecting.
    Hello {
        name: String,
        /// How many columns around the player the client wants.
        view_distance: u8,
        protocol_version: u64,
    },
    /// The client changed its view distance.
    ViewDistance(u8),
    /// Place (or, with air, break) a block. `held` is the item in hand
    /// (it decides what a broken block drops).
    SetBlock {
        pos: [i32; 3],
        block: u16,
        held: u16,
    },
    /// The latest few physics inputs (sent every tick; recent ones are
    /// repeated so a lost packet doesn't lose an input).
    Inputs(Vec<NetInput>),
    /// The player's inventory changed (so the server can save it).
    Inventory {
        slots: Vec<NetSlot>,
        selected: u8,
    },
    Chat {
        text: String,
    },
    /// Use a tool the game mode gave us, aiming in `direction`.
    UseTool {
        tool: String,
        direction: [f32; 3],
        secondary: bool,
    },
    /// Switched between creative and survival.
    GameMode {
        creative: bool,
    },
    /// Open the furnace (or other container) at `pos`.
    OpenContainer {
        pos: [i32; 3],
    },
    CloseContainer,
    /// Clicked a slot of the open container while holding `cursor` on the
    /// mouse. The server replies with `ContainerCursor`.
    ContainerClick {
        slot: u8,
        cursor: NetSlot,
        secondary: bool,
    },
    /// Throw items out in front of you (Q, or leftovers that didn't fit).
    DropItems(NetStack),
    /// Right-clicked an animal while holding `held` (shears, food, or
    /// nothing to pet it).
    UseEntity {
        id: u64,
        held: u16,
    },
    /// Used a block while holding `held`: open or close a door, sleep in
    /// a bed, eat cake, fill a composter, give a plant bone meal.
    UseBlock {
        pos: [i32; 3],
        held: u16,
    },
    /// Wrote on a sign (up to [`SIGN_LINES`] lines of [`SIGN_LINE_LEN`]).
    SetSign {
        pos: [i32; 3],
        lines: Vec<String>,
    },
    /// Ate one of this item (it was in hand).
    Eat {
        item: u16,
    },
    /// Hit an animal or monster (left click), holding `held`.
    HitEntity {
        id: u64,
        held: u16,
    },
    /// Got out of bed (moved while sleeping).
    LeaveBed,
    /// Got out of a boat (or off a pig).
    LeaveBoat,
    /// Used the held item in the air, aiming along `direction` (a bow).
    UseItem {
        held: u16,
        direction: [f32; 3],
    },
}

/// Lines on a sign, and characters per line.
pub const SIGN_LINES: usize = 4;
pub const SIGN_LINE_LEN: usize = 15;

/// Clean up sign text: at most 4 lines of 15 printable characters.
pub fn sanitize_sign(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .take(SIGN_LINES)
        .map(|l| {
            l.chars()
                .filter(|c| !c.is_control())
                .take(SIGN_LINE_LEN)
                .collect()
        })
        .collect()
}

/// Another player as seen by everyone else.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: String,
}

// `Welcome` is big, but it's only sent once per connection.
#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ServerMessage {
    /// Reply to `Hello`: everything the client needs to start playing.
    Welcome {
        your_id: PlayerId,
        server_name: String,
        motd: String,
        creative: bool,
        time_of_day: f32,
        /// Where to spawn (or where you logged off).
        state: NetPlayerState,
        /// Saved inventory, if the server knows this player.
        inventory: Option<(Vec<NetSlot>, u8)>,
        /// Blocks, textures, models and tools (including mods' additions).
        content: ContentPack,
        /// Players already online.
        players: Vec<PlayerInfo>,
    },
    /// A column of chunks, encoded like in save files.
    Column {
        x: i32,
        z: i32,
        data: Vec<u8>,
    },
    /// A block changed.
    BlockChanged {
        pos: [i32; 3],
        block: u16,
    },
    PlayerJoined(PlayerInfo),
    PlayerLeft(PlayerId),
    /// Where *you* are according to the server, after applying your input
    /// number `ack_tick`. Used for reconciliation.
    YourState {
        ack_tick: u32,
        state: NetPlayerState,
    },
    /// Where everyone else is (sent ~20 times a second).
    PlayerStates {
        tick: u32,
        players: Vec<(PlayerId, NetPlayerState)>,
    },
    TimeOfDay(f32),
    Chat {
        from: Option<String>,
        text: String,
    },
    /// You are being disconnected, with a reason to show.
    Kick(String),

    // ---- Game-mode features (driven by mods) ----
    /// Something appeared. Projectiles (`gravity` set) are moved by the
    /// client itself between updates, so they fly smoothly.
    EntitySpawned {
        id: u64,
        model: String,
        position: [f32; 3],
        velocity: [f32; 3],
        gravity: Option<f32>,
        /// Which way it faces (radians; 0 = towards +z).
        yaw: f32,
    },
    /// New positions (and facing) for entities (sent ~20 times a second).
    EntitiesMoved(Vec<(u64, [f32; 3], f32)>),
    EntityRemoved(u64),
    /// Your rules (speed, flying, building).
    Rules(ourcraft_mod_api::PlayerRules),
    /// How another player looks (shirt colour, name tag prefix).
    Appearance {
        player: PlayerId,
        color: Option<ourcraft_mod_api::Rgba>,
        tag: Option<String>,
    },
    /// Your health bar (hidden when `max` is 0).
    Health {
        health: f32,
        max: f32,
    },
    /// Your HUD panel (top right).
    Hud(Vec<ourcraft_mod_api::HudLine>),
    /// A big message in the middle of the screen.
    Title {
        text: String,
        seconds: f32,
    },
    /// Your tools (empty = normal block hotbar).
    Tools(Vec<String>),
    Effect {
        kind: ourcraft_mod_api::EffectKind,
        position: [f32; 3],
        color: ourcraft_mod_api::Rgba,
    },
    Sound {
        name: String,
        at: Option<[f32; 3]>,
    },

    // ---- Survival (milestone 11) ----
    /// You picked something up: add it to your inventory.
    GiveItems(NetStack),
    /// A container screen to show (also sent when its contents change).
    Container(NetContainer),
    /// What's on your mouse cursor after a `ContainerClick`.
    ContainerCursor(NetSlot),
    /// The container you had open is gone (someone broke it).
    ContainerClosed,
    /// Remove these from your inventory (you fed an animal, say).
    TakeItems(NetStack),
    /// What a sign says (sent when it changes, and with its column).
    SignText {
        pos: [i32; 3],
        lines: Vec<String>,
    },
    /// Your health and hunger (out of 20) and breath (out of 10), in
    /// survival. All zero hides them (creative, or a game mode).
    Vitals {
        health: f32,
        food: f32,
        air: f32,
    },
    /// Is it raining? (Snowing, where it's snowy.)
    Weather {
        raining: bool,
    },
    /// You're asleep in the bed at this block (lie down), or awake again.
    Sleeping {
        bed: Option<[i32; 3]>,
    },
    /// You're sitting in this boat (an entity), or not in one.
    Boating {
        boat: Option<u64>,
    },
    /// You're riding this animal (an entity), or not.
    Riding {
        mount: Option<u64>,
    },
    /// What's in the item frame at a block (0 = nothing).
    FrameItem {
        pos: [i32; 3],
        item: u16,
    },
    /// Where you wake up (your bed, or the world's start): the compass
    /// points there.
    Home {
        pos: [i32; 3],
    },
    /// What a player wears (item numbers, helmet first; 0 = nothing).
    Armour {
        player: u64,
        pieces: [u16; 4],
    },
}

/// Encode a message for sending.
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    postcard::to_allocvec(msg).expect("encoding a message can't fail")
}

/// Decode a received message. Returns `None` for garbage, which callers
/// should treat as a misbehaving peer (never trust bytes from the network).
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}

/// Clean up a player name: trim, limit length, keep printable characters.
pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .take(MAX_NAME_LEN)
        .collect();
    let cleaned = cleaned.trim().to_owned();
    if cleaned.is_empty() {
        "Player".into()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_messages_round_trip() {
        let msgs = vec![
            ClientMessage::Hello {
                name: "Robin".into(),
                view_distance: 8,
                protocol_version: PROTOCOL_VERSION,
            },
            ClientMessage::SetBlock {
                pos: [-5, 70, 123_456],
                block: 3,
                held: 40,
            },
            ClientMessage::Inputs(vec![NetInput {
                tick: 99,
                movement: [0.5, -1.0],
                jump: true,
                yaw: 1.0,
                ..Default::default()
            }]),
            ClientMessage::Inventory {
                slots: vec![
                    Some(NetStack {
                        item: 1,
                        count: 64,
                        wear: 0,
                    }),
                    None,
                ],
                selected: 1,
            },
            ClientMessage::Chat { text: "hi!".into() },
            ClientMessage::GameMode { creative: false },
            ClientMessage::OpenContainer { pos: [1, 2, 3] },
            ClientMessage::ContainerClick {
                slot: 2,
                cursor: None,
                secondary: true,
            },
        ];
        for m in msgs {
            let back: ClientMessage = decode(&encode(&m)).unwrap();
            assert_eq!(back, m);
        }
    }

    #[test]
    fn server_messages_round_trip() {
        let msgs = vec![
            ServerMessage::Welcome {
                your_id: 7,
                server_name: "Home".into(),
                motd: "Welcome!".into(),
                creative: true,
                time_of_day: 0.5,
                state: NetPlayerState::default(),
                inventory: Some((
                    vec![Some(NetStack {
                        item: 2,
                        count: 10,
                        wear: 3,
                    })],
                    0,
                )),
                content: {
                    let blocks = crate::block::BlockRegistry::with_builtin_blocks();
                    ContentPack {
                        items: crate::item::ItemRegistry::build(&blocks, &[])
                            .defs()
                            .to_vec(),
                        recipes: crate::recipes::builtin_recipes(),
                        blocks: blocks.defs().to_vec(),
                        textures: vec![("m:t".into(), vec![1, 2, 3, 4])],
                        ..Default::default()
                    }
                },
                players: vec![PlayerInfo {
                    id: 3,
                    name: "Sam".into(),
                }],
            },
            ServerMessage::Column {
                x: -1,
                z: 2,
                data: vec![1, 2, 3],
            },
            ServerMessage::GiveItems(NetStack {
                item: 5,
                count: 1,
                wear: 0,
            }),
            ServerMessage::Container(NetContainer {
                pos: [4, 5, 6],
                kind: crate::container::ContainerKind::Furnace,
                slots: vec![None, None, None],
                burn: 0.5,
                smelt: 0.25,
            }),
            ServerMessage::BlockChanged {
                pos: [0, 1, 2],
                block: 0,
            },
            ServerMessage::PlayerStates {
                tick: 5,
                players: vec![(3, NetPlayerState::default())],
            },
            ServerMessage::Kick("bye".into()),
        ];
        for m in msgs {
            let back: ServerMessage = decode(&encode(&m)).unwrap();
            assert_eq!(back, m);
        }
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(decode::<ClientMessage>(&[255, 255, 255, 255]).is_none());
        assert!(decode::<ServerMessage>(&[]).is_none());
    }

    #[test]
    fn movement_updates_are_small() {
        // Sent 60×/s per player with 4 inputs each, so keep it compact.
        let m = ClientMessage::Inputs(vec![NetInput::default(); 4]);
        assert!(encode(&m).len() < 120, "{} bytes", encode(&m).len());
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("  Robin  "), "Robin");
        assert_eq!(sanitize_name("<script>"), "script");
        assert_eq!(sanitize_name(""), "Player");
        assert_eq!(
            sanitize_name("a_very_long_name_indeed_yes").len(),
            MAX_NAME_LEN
        );
    }
}

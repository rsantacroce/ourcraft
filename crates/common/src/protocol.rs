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
pub const PROTOCOL_VERSION: u64 = 2;
/// Identifies Ourcraft traffic (and its version) on the wire.
pub const PROTOCOL_ID: u64 = 0x4f55_5243_0000_0000 | PROTOCOL_VERSION; // "OURC" + version
/// Default UDP port for servers.
pub const DEFAULT_PORT: u16 = 41_700;
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
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
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
}

/// One inventory slot: (block id, count).
pub type NetSlot = Option<(u16, u16)>;

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
    /// Place (or, with air, break) a block.
    SetBlock {
        pos: [i32; 3],
        block: u16,
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
}

/// Another player as seen by everyone else.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: String,
}

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
        /// Block names in id order, so the client can check it has the same
        /// blocks (mods add their own).
        block_names: Vec<String>,
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
            },
            ClientMessage::Inputs(vec![NetInput {
                tick: 99,
                movement: [0.5, -1.0],
                jump: true,
                yaw: 1.0,
                ..Default::default()
            }]),
            ClientMessage::Inventory {
                slots: vec![Some((1, 64)), None],
                selected: 1,
            },
            ClientMessage::Chat { text: "hi!".into() },
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
                inventory: Some((vec![Some((2, 10))], 0)),
                block_names: vec!["core:air".into(), "core:stone".into()],
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

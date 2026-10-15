//! Everything a mod author needs: friendly functions that talk to the game,
//! the [`Mod`] trait, and the [`export_mod!`](crate::export_mod) macro.
//!
//! ```ignore
//! use ourcraft_mod_api::guest::{self, Mod};
//! use ourcraft_mod_api::{Event, EventReply};
//!
//! #[derive(Default)]
//! struct Hello;
//!
//! impl Mod for Hello {
//!     fn on_event(&mut self, event: Event) -> EventReply {
//!         if let Event::PlayerJoined { name, .. } = event {
//!             guest::chat(None, &format!("Hello, {name}!"));
//!         }
//!         EventReply::Ok
//!     }
//! }
//!
//! ourcraft_mod_api::export_mod!(Hello);
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::*;

/// Implement this for your mod's state.
pub trait Mod {
    fn on_event(&mut self, event: Event) -> EventReply;
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "ourcraft")]
unsafe extern "C" {
    fn call(ptr: u32, len: u32) -> u64;
}

/// Send a call to the game and wait for its reply.
#[cfg(target_arch = "wasm32")]
pub fn host(c: &HostCall) -> HostReply {
    let bytes = encode(c);
    // SAFETY: the host reads `len` bytes at `ptr` from our memory, and
    // returns a buffer it got from `ourcraft_alloc` (see `take_buffer`).
    let packed = unsafe { call(bytes.as_ptr() as u32, bytes.len() as u32) };
    let reply = unsafe { take_buffer(packed) };
    decode(&reply).unwrap_or(HostReply::Error("unreadable reply".into()))
}

/// Outside WebAssembly (e.g. unit tests of a mod's logic) there's no game
/// to talk to.
#[cfg(not(target_arch = "wasm32"))]
pub fn host(_c: &HostCall) -> HostReply {
    HostReply::Error("not running inside Ourcraft".into())
}

/// Reserve a buffer for the host to write into. Ownership passes back to
/// us in [`take_buffer`].
pub fn alloc_buffer(len: u32) -> u32 {
    let buf: Box<[u8]> = alloc::vec![0u8; len as usize].into_boxed_slice();
    Box::into_raw(buf) as *mut u8 as usize as u32
}

/// Take back a buffer made by [`alloc_buffer`], given `(ptr << 32) | len`.
///
/// # Safety
/// `packed` must describe a buffer from `alloc_buffer` that hasn't been
/// taken yet.
pub unsafe fn take_buffer(packed: u64) -> Vec<u8> {
    let ptr = (packed >> 32) as usize as *mut u8;
    let len = (packed & 0xFFFF_FFFF) as usize;
    if len == 0 {
        return Vec::new();
    }
    // SAFETY: guaranteed by the caller.
    let slice = core::ptr::slice_from_raw_parts_mut(ptr, len);
    unsafe { Box::from_raw(slice) }.into_vec()
}

/// Generate the exports the game looks for, for a type implementing
/// [`Mod`] and [`Default`].
#[macro_export]
macro_rules! export_mod {
    ($ty:ty) => {
        static mut OURCRAFT_MOD: Option<$ty> = None;
        static mut OURCRAFT_REPLY: $crate::__Vec<u8> = $crate::__Vec::new();

        #[unsafe(no_mangle)]
        pub extern "C" fn ourcraft_api_version() -> u32 {
            $crate::API_VERSION
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn ourcraft_alloc(len: u32) -> u32 {
            $crate::guest::alloc_buffer(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn ourcraft_event(ptr: u32, len: u32) -> u64 {
            use $crate::guest::Mod;
            // SAFETY: WebAssembly mods are single-threaded, and the host
            // never calls in again while an event is running.
            unsafe {
                let bytes = $crate::guest::take_buffer(((ptr as u64) << 32) | len as u64);
                let reply = match $crate::decode::<$crate::Event>(&bytes) {
                    Some(event) => {
                        let m = (*core::ptr::addr_of_mut!(OURCRAFT_MOD))
                            .get_or_insert_with(<$ty as Default>::default);
                        m.on_event(event)
                    }
                    None => $crate::EventReply::Ok,
                };
                let out = &mut *core::ptr::addr_of_mut!(OURCRAFT_REPLY);
                *out = $crate::encode(&reply);
                ((out.as_ptr() as u64) << 32) | out.len() as u64
            }
        }
    };
}

// ---- Friendly helpers ----

pub fn log(text: &str) {
    host(&HostCall::Log(text.into()));
}

pub fn register_block(spec: BlockSpec) {
    host(&HostCall::RegisterBlock(spec));
}

pub fn register_texture(name: &str, rgba: Vec<u8>) {
    host(&HostCall::RegisterTexture(TextureSpec {
        name: name.into(),
        rgba,
    }));
}

pub fn register_model(spec: ModelSpec) {
    host(&HostCall::RegisterModel(spec));
}

pub fn register_tool(spec: ToolSpec) {
    host(&HostCall::RegisterTool(spec));
}

/// The id of a block by name (`"core:stone"`, `"yourmod:thing"`).
pub fn block_id(name: &str) -> Option<BlockId> {
    match host(&HostCall::BlockId(name.into())) {
        HostReply::Block(id) => Some(id),
        _ => None,
    }
}

pub fn get_block(pos: BlockPos) -> BlockId {
    match host(&HostCall::GetBlock(pos)) {
        HostReply::Block(id) => id,
        _ => 0,
    }
}

pub fn set_block(pos: BlockPos, block: BlockId) {
    host(&HostCall::SetBlock { pos, block });
}

/// Like [`set_block`], but tells you whether it worked. It fails if that
/// part of the world isn't loaded yet; just try again later.
pub fn try_set_block(pos: BlockPos, block: BlockId) -> bool {
    host(&HostCall::SetBlock { pos, block }) == HostReply::Ok
}

/// The y of the highest solid block at (x, z).
pub fn ground_height(x: i32, z: i32) -> Option<i32> {
    match host(&HostCall::GroundHeight { x, z }) {
        HostReply::Height(h) => h,
        _ => None,
    }
}

pub fn time_of_day() -> f32 {
    match host(&HostCall::GetTimeOfDay) {
        HostReply::Time(t) => t,
        _ => 0.0,
    }
}

pub fn set_time_of_day(t: f32) {
    host(&HostCall::SetTimeOfDay(t));
}

pub fn players() -> Vec<PlayerInfo> {
    match host(&HostCall::GetPlayers) {
        HostReply::Players(p) => p,
        _ => Vec::new(),
    }
}

/// Send a chat message to one player, or everyone with `None`.
pub fn chat(to: Option<PlayerId>, text: &str) {
    host(&HostCall::Chat {
        to,
        text: text.into(),
    });
}

pub fn teleport(player: PlayerId, position: Vec3) {
    host(&HostCall::Teleport { player, position });
}

pub fn set_rules(player: PlayerId, rules: PlayerRules) {
    host(&HostCall::SetRules { player, rules });
}

pub fn set_health(player: PlayerId, health: f32, max: f32) {
    host(&HostCall::SetHealth {
        player,
        health,
        max,
    });
}

pub fn set_hud(player: Option<PlayerId>, lines: Vec<HudLine>) {
    host(&HostCall::SetHud { player, lines });
}

pub fn show_title(player: Option<PlayerId>, text: &str, seconds: f32) {
    host(&HostCall::ShowTitle {
        player,
        text: text.into(),
        seconds,
    });
}

pub fn set_tools(player: PlayerId, tools: Vec<String>) {
    host(&HostCall::SetTools { player, tools });
}

pub fn spawn_entity(spec: EntitySpec) -> Option<EntityId> {
    match host(&HostCall::SpawnEntity(spec)) {
        HostReply::Entity(id) => Some(id),
        _ => None,
    }
}

pub fn move_entity(id: EntityId, position: Vec3) {
    host(&HostCall::MoveEntity { id, position });
}

pub fn despawn_entity(id: EntityId) {
    host(&HostCall::DespawnEntity(id));
}

pub fn effect(kind: EffectKind, position: Vec3, color: Rgba) {
    host(&HostCall::Effect {
        kind,
        position,
        color,
    });
}

pub fn sound(name: &str, at: Option<Vec3>) {
    host(&HostCall::Sound {
        name: name.into(),
        at,
    });
}

/// Cast a ray. Returns what it hit first, if anything.
pub fn raycast(
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
    hit_players: bool,
    ignore_player: Option<PlayerId>,
) -> Option<Hit> {
    match host(&HostCall::Raycast {
        origin,
        direction,
        max_distance,
        hit_players,
        ignore_player,
    }) {
        HostReply::Ray(hit) => hit,
        _ => None,
    }
}

/// A HUD line in white.
pub fn hud_line(text: impl Into<String>) -> HudLine {
    HudLine {
        text: text.into(),
        color: [255, 255, 255, 255],
    }
}

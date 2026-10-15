//! The Ourcraft server.
//!
//! The server owns the real world: it generates and saves terrain, decides
//! whether block edits are allowed, and tells every client what changed.
//! Clients only ever *ask* for changes. That's what "server-authoritative"
//! means, and it's what keeps everyone's world in sync (and makes cheating
//! harder).
//!
//! It runs two ways:
//! - **Dedicated:** the `ourcraft-server` binary, headless, listening on UDP.
//! - **Embedded:** single player starts it on a background thread inside
//!   the game and talks to it through an in-memory [`LocalLink`] instead of
//!   the network. Same code, same rules; and the same world can later be
//!   opened up to friends.

pub mod mods;
pub mod net;
pub mod players;
pub mod world;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;

/// Server ticks per second.
pub const TICK_RATE: f64 = 60.0;

/// How to run a server.
#[derive(Clone, Debug, Resource)]
pub struct ServerConfig {
    /// Folder holding the world (created if missing).
    pub world_dir: PathBuf,
    /// Name for a new world.
    pub world_name: String,
    /// Seed for a new world (random if `None`).
    pub seed: Option<u64>,
    /// Game mode for a new world.
    pub creative: bool,
    pub server_name: String,
    pub motd: String,
    /// The most columns around each player the server will send.
    pub max_view_distance: u8,
    /// Listen for network players on this address. `None` = single player only.
    pub bind: Option<SocketAddr>,
    pub max_players: usize,
    /// Game mode for a new world (a mod's name for it, e.g. `"team_shooter"`).
    pub game_mode: Option<String>,
    /// Extra mods (`*.wasm`) to load from this folder.
    pub mods_dir: Option<PathBuf>,
    /// Load the mods built into the game.
    pub bundled_mods: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            world_dir: PathBuf::from("worlds/world"),
            world_name: "world".into(),
            seed: None,
            creative: true,
            server_name: "Ourcraft Server".into(),
            motd: "Have fun and be kind!".into(),
            max_view_distance: 10,
            bind: None,
            max_players: 16,
            game_mode: None,
            mods_dir: None,
            bundled_mods: true,
        }
    }
}

/// Client side of an in-memory connection to an embedded server. Packets
/// are passed as byte vectors through channels between the two threads.
pub struct LocalLink {
    pub to_server: Sender<Vec<u8>>,
    // A `Receiver` can't be shared between threads, but Bevy resources must
    // be shareable, so it sits behind a (never contended) mutex.
    pub from_server: Mutex<Receiver<Vec<u8>>>,
}

/// Server side of a [`LocalLink`].
pub struct LocalServerLink {
    pub from_client: Mutex<Receiver<Vec<u8>>>,
    pub to_client: Sender<Vec<u8>>,
}

/// Create both ends of an in-memory connection.
pub fn local_link() -> (LocalLink, LocalServerLink) {
    let (to_server, from_client) = channel();
    let (to_client, from_server) = channel();
    (
        LocalLink {
            to_server,
            from_server: Mutex::new(from_server),
        },
        LocalServerLink {
            from_client: Mutex::new(from_client),
            to_client,
        },
    )
}

/// Set from outside to ask the server to save and stop.
#[derive(Resource, Clone, Default)]
pub struct StopFlag(pub Arc<AtomicBool>);

/// Set from outside to ask the server to save right away (e.g. when a
/// phone sends the game to the background). Cleared once saved.
#[derive(Resource, Clone, Default)]
pub struct SaveNowFlag(pub Arc<AtomicBool>);

/// All server systems.
pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<mods::ModQueue>()
            .add_plugins((world::WorldPlugin, net::NetPlugin, players::PlayersPlugin))
            .add_systems(Update, check_stop_flag);
    }
}

fn check_stop_flag(flag: Option<Res<StopFlag>>, mut exit: MessageWriter<AppExit>) {
    if flag.is_some_and(|f| f.0.load(Ordering::Relaxed)) {
        exit.write(AppExit::Success);
    }
}

/// Build a server app. `local` adds an in-memory client connection (single
/// player); `config.bind` adds UDP. `log` sets up logging (leave it off when
/// embedded: the game has already set it up for the whole process).
pub fn build_app(
    config: ServerConfig,
    local: Option<LocalServerLink>,
    stop: StopFlag,
    save_now: SaveNowFlag,
    log: bool,
) -> App {
    let mut app = App::new();
    app.add_plugins(
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / TICK_RATE,
        ))),
    );
    if log {
        app.add_plugins(bevy::log::LogPlugin::default());
    }
    app.insert_resource(config)
        .insert_resource(stop)
        .insert_resource(save_now)
        .insert_resource(net::PendingLocalLink(std::sync::Mutex::new(local)))
        .add_plugins(ServerPlugin);
    app
}

/// A server running on a background thread inside the game.
pub struct EmbeddedServer {
    stop: StopFlag,
    save: SaveNowFlag,
    thread: Option<JoinHandle<()>>,
}

impl EmbeddedServer {
    /// Start a server thread and return it with the client's end of the link.
    pub fn start(config: ServerConfig) -> (Self, LocalLink) {
        let (client_end, server_end) = local_link();
        let stop = StopFlag::default();
        let save = SaveNowFlag::default();
        let (thread_stop, thread_save) = (stop.clone(), save.clone());
        let thread = std::thread::Builder::new()
            .name("ourcraft-server".into())
            .spawn(move || {
                build_app(config, Some(server_end), thread_stop, thread_save, false).run();
            })
            .expect("can't start server thread");
        (
            Self {
                stop,
                save,
                thread: Some(thread),
            },
            client_end,
        )
    }

    /// Ask the server to save everything soon (without stopping).
    pub fn request_save(&self) {
        self.save.0.store(true, Ordering::Relaxed);
    }

    /// Ask the server to save and stop, and wait (briefly) for it.
    pub fn stop(&mut self) {
        self.stop.0.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            // Saving is quick; don't hang forever if something's wrong.
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !t.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if t.is_finished() {
                let _ = t.join();
            } else {
                warn!("Server thread didn't stop in time");
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}

impl Drop for EmbeddedServer {
    fn drop(&mut self) {
        self.stop();
    }
}

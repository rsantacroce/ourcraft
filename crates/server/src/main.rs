//! The dedicated Ourcraft server.
//!
//! ```text
//! ourcraft-server [--port 41700] [--world NAME] [--dir FOLDER] [--seed N]
//!                 [--survival] [--name "Server name"] [--motd "Message"]
//!                 [--view-distance 10] [--max-players 16]
//! ```
//! Worlds are stored in `./worlds/<NAME>` unless `--dir` says otherwise.
//! Stop it with Ctrl+C; it saves before exiting.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use ourcraft_common::protocol::DEFAULT_PORT;
use ourcraft_server::{SaveNowFlag, ServerConfig, StopFlag, build_app};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: ourcraft-server [--port {DEFAULT_PORT}] [--world NAME] [--dir FOLDER] \
             [--seed N] [--survival] [--name NAME] [--motd TEXT] [--view-distance N] \
             [--max-players N]"
        );
        return;
    }

    let world = value("--world").unwrap_or_else(|| "world".into());
    let port = value("--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let defaults = ServerConfig::default();
    let config = ServerConfig {
        world_dir: value("--dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("worlds").join(&world)),
        world_name: world,
        seed: value("--seed").and_then(|s| s.parse().ok()),
        creative: !args.iter().any(|a| a == "--survival"),
        server_name: value("--name").unwrap_or(defaults.server_name),
        motd: value("--motd").unwrap_or(defaults.motd),
        max_view_distance: value("--view-distance")
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.max_view_distance),
        bind: Some(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))),
        max_players: value("--max-players")
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.max_players),
    };

    // Ctrl+C asks the server to save and stop instead of dying instantly.
    let stop = StopFlag::default();
    let flag = stop.0.clone();
    if let Err(e) = ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed)) {
        eprintln!("Couldn't install Ctrl+C handler ({e}); the world still autosaves");
    }

    build_app(config, None, stop, SaveNowFlag::default(), true).run();
}

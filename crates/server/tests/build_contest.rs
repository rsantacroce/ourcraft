//! Build Contest, from bots: joining, starting a round, getting a plot and
//! the theme, and only building on your own plot.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::protocol::{
    self, Channel, ClientMessage, PROTOCOL_ID, PROTOCOL_VERSION, ServerMessage,
};
use ourcraft_server::net::connection_config;
use ourcraft_server::{SaveNowFlag, ServerConfig, StopFlag, build_app};
use renet::RenetClient;
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};

struct Bot {
    client: RenetClient,
    transport: NetcodeClientTransport,
    received: Vec<ServerMessage>,
}

impl Bot {
    fn connect(server: SocketAddr, id: u64) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let auth = ClientAuthentication::Unsecure {
            protocol_id: PROTOCOL_ID,
            client_id: id,
            server_addr: server,
            user_data: None,
        };
        Self {
            client: RenetClient::new(connection_config()),
            transport: NetcodeClientTransport::new(now, auth, socket).unwrap(),
            received: Vec::new(),
        }
    }

    fn send(&mut self, msg: &ClientMessage) {
        self.client
            .send_message(Channel::Reliable, protocol::encode(msg));
    }

    /// Exchange packets for a while, collecting messages.
    fn pump(&mut self, dt: Duration) {
        self.client.update(dt);
        let _ = self.transport.update(dt, &mut self.client);
        for ch in [Channel::Reliable, Channel::Unreliable] {
            while let Some(b) = self.client.receive_message(ch) {
                if let Some(m) = protocol::decode::<ServerMessage>(&b) {
                    self.received.push(m);
                }
            }
        }
        let _ = self.transport.send_packets(&mut self.client);
    }

    fn has(&self, f: impl Fn(&ServerMessage) -> bool) -> bool {
        self.received.iter().any(f)
    }
}

/// Run all bots until `done` holds or `secs` pass. Returns whether it held.
fn run_until(bots: &mut [&mut Bot], secs: f32, done: impl Fn(&[&mut Bot]) -> bool) -> bool {
    let start = Instant::now();
    let step = Duration::from_millis(10);
    while start.elapsed().as_secs_f32() < secs {
        for b in bots.iter_mut() {
            b.pump(step);
        }
        if done(bots) {
            return true;
        }
        std::thread::sleep(step);
    }
    false
}

fn titles(b: &Bot) -> Vec<String> {
    b.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Title { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn chats(b: &Bot) -> Vec<String> {
    b.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Chat { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Start a Build Contest server; returns its address, stop flag, thread
/// and world folder.
fn start_server(
    tag: &str,
) -> (
    SocketAddr,
    StopFlag,
    std::thread::JoinHandle<()>,
    std::path::PathBuf,
) {
    let dir = std::env::temp_dir().join(format!("ourcraft-contest-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(4242),
        game_mode: Some("build_contest".into()),
        bind: Some(addr),
        max_view_distance: 4,
        creative: true,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));
    (addr, stop, server, dir)
}

fn join(addr: SocketAddr, id: u64, name: &str) -> Bot {
    let mut bot = Bot::connect(addr, id);
    assert!(run_until(&mut [&mut bot], 5.0, |b| b[0]
        .client
        .is_connected()));
    bot.send(&ClientMessage::Hello {
        name: name.into(),
        view_distance: 4,
        protocol_version: PROTOCOL_VERSION,
    });
    bot
}

#[test]
fn a_round_gives_everyone_a_plot_and_a_theme() {
    let (addr, stop, server, dir) = start_server("theme");
    let mut ada = join(addr, 301, "Ada");
    // Welcomed to the game, with the vote badges known to the client.
    assert!(run_until(&mut [&mut ada], 10.0, |b| titles(b[0])
        .iter()
        .any(|t| t == "Build Contest!")));
    let content = ada
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("welcome");
    assert!(content.tools.iter().any(|t| t.name == "contest:vote_5"));
    assert!(run_until(&mut [&mut ada], 3.0, |b| b[0].has(
        |m| matches!(m, ServerMessage::Hud(l) if l.iter().any(|l| l.text == "Build Contest"))
    )));

    // Start a one-minute round: the plots are made, then the theme.
    ada.send(&ClientMessage::Chat {
        text: "/start 1".into(),
    });
    assert!(run_until(&mut [&mut ada], 5.0, |b| titles(b[0])
        .iter()
        .any(|t| t.starts_with("Getting the plots ready"))));
    assert!(
        run_until(&mut [&mut ada], 60.0, |b| titles(b[0])
            .iter()
            .any(|t| t.starts_with("Build: "))),
        "no theme; titles {:?}",
        titles(&ada)
    );
    let theme = titles(&ada)
        .into_iter()
        .find_map(|t| {
            t.strip_prefix("Build: ")
                .map(|t| t.trim_end_matches('!').to_owned())
        })
        .unwrap();
    assert!(
        chats(&ada)
            .iter()
            .any(|c| c.contains(&theme) && c.contains("1 minute to build")),
        "{:?}",
        chats(&ada)
    );
    // The HUD counts down.
    assert!(run_until(&mut [&mut ada], 3.0, |b| {
        b[0].has(
        |m| matches!(m, ServerMessage::Hud(l) if l.iter().any(|l| l.text.starts_with("Time left: 0:")))
    )
    }));
    // /theme says it again.
    ada.send(&ClientMessage::Chat {
        text: "/theme".into(),
    });
    assert!(run_until(&mut [&mut ada], 3.0, |b| chats(b[0])
        .iter()
        .any(|c| c == &format!("The theme is: {theme}"))));

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A whole round with two players: build, vote on each other, a winner.
/// Slow (a round has at least a minute of building):
/// `cargo test -p ourcraft_server --test build_contest -- --ignored`
#[test]
#[ignore]
fn two_players_vote_and_one_wins() {
    let (addr, stop, server, dir) = start_server("vote");
    let mut ada = join(addr, 401, "Ada");
    let mut bo = join(addr, 402, "Bo");
    assert!(run_until(&mut [&mut ada, &mut bo], 10.0, |b| b
        .iter()
        .all(|b| titles(b).iter().any(|t| t == "Build Contest!"))));
    ada.send(&ClientMessage::Chat {
        text: "/start 1".into(),
    });
    // Building, then voting: both get the vote badges.
    assert!(
        run_until(&mut [&mut ada, &mut bo], 100.0, |b| b
            .iter()
            .all(|b| b.has(
                |m| matches!(m, ServerMessage::Tools(t) if t.len() == 5 && t[4] == "contest:vote_5")
            ))),
        "no voting; titles {:?}",
        titles(&ada)
    );
    // Each plot in turn: whoever didn't build it votes WOW for Bo, Meh for Ada.
    let vote = |bot: &mut Bot, tool: &str| {
        bot.send(&ClientMessage::UseTool {
            tool: tool.into(),
            direction: [0.0, 0.0, -1.0],
            secondary: false,
        })
    };
    for _ in 0..2 {
        let showing = |b: &Bot| {
            titles(b)
                .last()
                .and_then(|t| t.strip_suffix("'s build").map(str::to_owned))
        };
        assert!(run_until(&mut [&mut ada, &mut bo], 30.0, |b| showing(b[0]).is_some()));
        let owner = showing(&ada).unwrap();
        if owner == "Bo" {
            vote(&mut ada, "contest:vote_5");
        } else {
            vote(&mut bo, "contest:vote_1");
        }
        let before = titles(&ada).len();
        // The next plot (or the results) comes along after the vote.
        assert!(run_until(&mut [&mut ada, &mut bo], 30.0, |b| titles(b[0])
            .len()
            > before));
    }
    assert!(
        run_until(&mut [&mut ada, &mut bo], 30.0, |b| titles(b[0])
            .iter()
            .any(|t| t == "Bo wins!")),
        "titles {:?}",
        titles(&ada)
    );
    assert!(
        chats(&ada)
            .iter()
            .any(|c| c.starts_with("Points: Bo 5, Ada 1"))
    );

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

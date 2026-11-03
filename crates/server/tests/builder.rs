//! The building tools (/pos1, /fill, /undo, /sphere) and /spawn, from a bot.

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

fn replies(b: &Bot) -> Vec<String> {
    b.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Chat { from: None, text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn changes_at(b: &Bot, pos: [i32; 3]) -> Vec<u16> {
    b.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::BlockChanged { pos: p, block } if *p == pos => Some(*block),
            _ => None,
        })
        .collect()
}

#[test]
fn building_tools_fill_undo_and_spawn() {
    let dir = std::env::temp_dir().join(format!("ourcraft-builder-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(21),
        bind: Some(addr),
        max_view_distance: 8,
        creative: true,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));
    let mut bot = Bot::connect(addr, 78);
    assert!(run_until(&mut [&mut bot], 5.0, |b| b[0]
        .client
        .is_connected()));
    bot.send(&ClientMessage::Hello {
        name: "Builder".into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });
    let columns = |b: &Bot| {
        b.received
            .iter()
            .filter(|m| matches!(m, ServerMessage::Column { .. }))
            .count()
    };
    assert!(run_until(&mut [&mut bot], 20.0, |b| columns(b[0]) >= 25));

    let say = |bot: &mut Bot, text: &str| {
        bot.send(&ClientMessage::Chat { text: text.into() });
    };

    // Both corners on the block we're looking at: a one-block box.
    say(&mut bot, "/pos1");
    say(&mut bot, "/pos2");
    let corner2 = |b: &Bot| replies(b).into_iter().find(|r| r.starts_with("Corner 2"));
    assert!(
        run_until(&mut [&mut bot], 5.0, |b| corner2(b[0]).is_some()),
        "{:?}",
        replies(&bot)
    );
    // "Corner 2 at x y z. The box is 1x1x1 (1 blocks)."
    let corner = corner2(&bot).unwrap();
    let nums: Vec<i32> = corner["Corner 2 at ".len()..]
        .split('.')
        .next()
        .unwrap()
        .split_whitespace()
        .filter_map(|w| w.parse().ok())
        .collect();
    let pos = [nums[0], nums[1], nums[2]];
    assert!(corner.contains("1x1x1"), "{corner}");

    say(&mut bot, "/fill glass");
    assert!(
        run_until(&mut [&mut bot], 5.0, |b| changes_at(b[0], pos).len() == 1),
        "no change at {pos:?}: {:?}",
        replies(&bot)
    );
    say(&mut bot, "/undo");
    assert!(run_until(&mut [&mut bot], 5.0, |b| changes_at(b[0], pos)
        .len()
        == 2));
    let seen = changes_at(&bot, pos);
    assert_ne!(seen[0], seen[1], "undo didn't put the old block back");

    // A big change re-sends whole columns rather than every block.
    let before = columns(&bot);
    say(&mut bot, "/sphere 12 glass");
    assert!(
        run_until(&mut [&mut bot], 10.0, |b| columns(b[0]) > before),
        "big edit wasn't re-sent: {:?}",
        replies(&bot)
    );
    assert!(replies(&bot).iter().any(|r| r.contains("blocks changed")));

    // Unknown blocks are explained, not guessed.
    say(&mut bot, "/fill nonsense");
    assert!(run_until(&mut [&mut bot], 5.0, |b| replies(b[0])
        .iter()
        .any(|r| r.contains("no block called"))));

    // Buried in the ground: lifted out after a moment. (A second player
    // watches where the first one is.)
    let mut watcher = Bot::connect(addr, 79);
    assert!(run_until(&mut [&mut watcher, &mut bot], 5.0, |b| b[0]
        .client
        .is_connected()));
    watcher.send(&ClientMessage::Hello {
        name: "Watcher".into(),
        view_distance: 2,
        protocol_version: PROTOCOL_VERSION,
    });
    let builder_id = bot
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { your_id, .. } => Some(*your_id),
            _ => None,
        })
        .unwrap();
    let buried_y = (pos[1] - 3) as f32;
    say(
        &mut bot,
        &format!(
            "/tp {} {buried_y} {}",
            pos[0] as f32 + 0.5,
            pos[2] as f32 + 0.5
        ),
    );
    let builder_y = |b: &Bot| {
        b.received.iter().rev().find_map(|m| match m {
            ServerMessage::PlayerStates { players, .. } => players
                .iter()
                .find(|(id, _)| *id == builder_id)
                .map(|(_, s)| s.position[1]),
            _ => None,
        })
    };
    assert!(
        run_until(&mut [&mut watcher, &mut bot], 5.0, |b| builder_y(b[0])
            .is_some_and(|y| y < buried_y + 0.5)),
        "never saw them buried: {:?}",
        builder_y(&watcher)
    );
    assert!(
        run_until(&mut [&mut watcher, &mut bot], 5.0, |b| builder_y(b[0])
            .is_some_and(|y| y >= pos[1] as f32)),
        "still buried at {:?}",
        builder_y(&watcher)
    );

    // /spawn always works.
    say(&mut bot, "/spawn");
    assert!(run_until(&mut [&mut bot], 5.0, |b| replies(b[0])
        .iter()
        .any(|r| r.contains("Back to"))));

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

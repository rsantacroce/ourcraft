//! Achievements: earned in survival by having things, announced, listed
//! with /goals, and saved with the world.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::protocol::{
    self, Channel, ClientMessage, NetStack, PROTOCOL_ID, PROTOCOL_VERSION, ServerMessage,
};
use ourcraft_common::save::WorldStorage;
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

fn titles(b: &Bot) -> Vec<String> {
    b.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Title { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn achievements_are_earned_listed_and_saved() {
    let dir = std::env::temp_dir().join(format!("ourcraft-goals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(5),
        bind: Some(addr),
        max_view_distance: 4,
        creative: false,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));
    let mut bot = Bot::connect(addr, 91);
    assert!(run_until(&mut [&mut bot], 5.0, |b| b[0]
        .client
        .is_connected()));
    bot.send(&ClientMessage::Hello {
        name: "Goalie".into(),
        view_distance: 2,
        protocol_version: PROTOCOL_VERSION,
    });
    assert!(run_until(&mut [&mut bot], 10.0, |b| b[0]
        .received
        .iter()
        .any(|m| matches!(m, ServerMessage::Welcome { .. }))));

    // Picking up a log (item 5, `core:log`) earns the first one.
    let mut slots = vec![None; 36];
    slots[0] = Some(NetStack {
        item: 5,
        count: 1,
        wear: 0,
    });
    bot.send(&ClientMessage::Inventory { slots, selected: 0 });
    assert!(
        run_until(&mut [&mut bot], 5.0, |b| titles(b[0])
            .iter()
            .any(|t| t == "Achievement! Get some wood")),
        "no title: {:?}",
        titles(&bot)
    );
    assert!(run_until(&mut [&mut bot], 3.0, |b| replies(b[0])
        .iter()
        .any(
            |r| r.starts_with("You did it: Get some wood!") && r.contains("Next: Make planks")
        )));

    // Only once.
    std::thread::sleep(Duration::from_millis(1200));
    run_until(&mut [&mut bot], 0.5, |_| false);
    assert_eq!(titles(&bot).len(), 1, "{:?}", titles(&bot));

    bot.send(&ClientMessage::Chat {
        text: "/goals".into(),
    });
    assert!(run_until(&mut [&mut bot], 3.0, |b| replies(b[0])
        .iter()
        .any(|r| r.starts_with("Goals: 1 of"))));

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let saved = WorldStorage::open(&dir)
        .unwrap()
        .load_achievements()
        .unwrap();
    assert_eq!(
        saved,
        vec![("Goalie".to_string(), vec!["wood".to_string()])]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

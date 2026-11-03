//! Gentle weapons, from a bot: a slingshot's pebble bops an animal (it
//! hops off with a baa) and a monster (bonk), from a few blocks away.

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

#[test]
fn slingshot_pebbles_bop_animals_and_monsters() {
    let dir = std::env::temp_dir().join(format!("ourcraft-sling-{}", std::process::id()));
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

    let mut kit = Bot::connect(addr, 77);
    assert!(run_until(&mut [&mut kit], 5.0, |b| b[0]
        .client
        .is_connected()));
    kit.send(&ClientMessage::Hello {
        name: "Kit".into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });
    let columns = |b: &Bot| {
        b.received
            .iter()
            .filter(|m| matches!(m, ServerMessage::Column { .. }))
            .count()
    };
    assert!(run_until(&mut [&mut kit], 20.0, |b| columns(b[0]) >= 25));
    let (content, feet) = kit
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { content, state, .. } => {
                Some((content.clone(), state.position))
            }
            _ => None,
        })
        .unwrap();
    let slingshot = content
        .items
        .iter()
        .position(|i| i.name == "core:slingshot")
        .unwrap() as u16;
    let eye = [
        feet[0],
        feet[1] + ourcraft_common::physics::EYE_HEIGHT,
        feet[2],
    ];

    // Summon something, then flick a pebble at it.
    let shoot_at = |kit: &mut Bot, what: &str| -> u64 {
        kit.received.clear();
        kit.send(&ClientMessage::Chat {
            text: format!("/summon {what}"),
        });
        let model = format!("@mob:core:{what}");
        let found = |b: &Bot| {
            b.received.iter().find_map(|m| match m {
                ServerMessage::EntitySpawned {
                    id,
                    model: m,
                    position,
                    ..
                } if *m == model => Some((*id, *position)),
                _ => None,
            })
        };
        assert!(run_until(&mut [&mut *kit], 5.0, |b| found(b[0]).is_some()));
        // Let it land, then aim at its middle.
        run_until(&mut [&mut *kit], 0.8, |_| false);
        let (id, mut at) = found(kit).unwrap();
        for m in &kit.received {
            if let ServerMessage::EntitiesMoved(list) = m {
                for (i, p, _) in list {
                    if *i == id {
                        at = *p;
                    }
                }
            }
        }
        let dir = [at[0] - eye[0], at[1] + 0.4 - eye[1], at[2] - eye[2]];
        kit.received.clear();
        kit.send(&ClientMessage::UseItem {
            held: slingshot,
            direction: dir,
        });
        id
    };

    shoot_at(&mut kit, "sheep");
    let sound = |b: &Bot, name: &str| {
        b.has(|m| matches!(m, ServerMessage::Sound { name: n, .. } if n == name))
    };
    assert!(run_until(&mut [&mut kit], 3.0, |b| b[0].has(
        |m| matches!(m, ServerMessage::EntitySpawned { model, .. } if model == "@pebble")
    )));
    assert!(
        run_until(&mut [&mut kit], 3.0, |b| sound(b[0], "baa")),
        "the sheep wasn't bopped"
    );

    // Monsters go away by day, so make it night first.
    kit.send(&ClientMessage::Chat {
        text: "/time night".into(),
    });
    std::thread::sleep(Duration::from_millis(600));
    shoot_at(&mut kit, "grumble");
    assert!(
        run_until(&mut [&mut kit], 3.0, |b| sound(b[0], "bonk")),
        "the grumble wasn't bopped"
    );
    // Two flicks at once: only one pebble (there's a short cooldown).
    std::thread::sleep(Duration::from_millis(600));
    kit.received.clear();
    for _ in 0..2 {
        kit.send(&ClientMessage::UseItem {
            held: slingshot,
            direction: [0.0, 0.0, 1.0],
        });
    }
    run_until(&mut [&mut kit], 0.4, |_| false);
    // (Earlier pebbles' news can arrive late: count only ones flying +z
    // from the eye.)
    let pebbles = kit
        .received
        .iter()
        .filter(|m| {
            matches!(m, ServerMessage::EntitySpawned { model, position, .. }
            if model == "@pebble" && (position[0] - eye[0]).abs() < 0.01 && position[2] > eye[2])
        })
        .count();
    assert_eq!(pebbles, 1);

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

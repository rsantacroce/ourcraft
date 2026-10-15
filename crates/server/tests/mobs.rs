//! End-to-end test of animals (milestone 12): a real server and a bot.
//! Animals appear around the player, walk about, and can't be used from
//! far away; up close, fed sheep have a lamb and shearing gives wool.

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
fn animals_appear_walk_and_react() {
    let dir = std::env::temp_dir().join(format!("ourcraft-mobs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(12),
        bind: Some(addr),
        max_view_distance: 4,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));

    let mut ana = Bot::connect(addr, 21);
    assert!(run_until(&mut [&mut ana], 5.0, |b| b[0]
        .client
        .is_connected()));
    ana.send(&ClientMessage::Hello {
        name: "Ana".into(),
        view_distance: 4,
        protocol_version: PROTOCOL_VERSION,
    });
    let mob_ids = |b: &Bot| -> Vec<(u64, String, [f32; 3])> {
        b.received
            .iter()
            .filter_map(|m| match m {
                ServerMessage::EntitySpawned {
                    id,
                    model,
                    position,
                    ..
                } if model.starts_with("@mob:") => Some((*id, model.clone(), *position)),
                _ => None,
            })
            .collect()
    };
    assert!(
        run_until(&mut [&mut ana], 40.0, |b| mob_ids(b[0]).len() >= 4),
        "no animals appeared"
    );
    let content = ana
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { content, .. } => Some(content.clone()),
            _ => None,
        })
        .unwrap();
    assert!(content.mobs.iter().any(|m| m.name == "core:sheep"));
    let item = |name: &str| content.items.iter().position(|i| i.name == name).unwrap() as u16;

    // They walk about.
    let ids: Vec<u64> = mob_ids(&ana).iter().map(|m| m.0).collect();
    assert!(
        run_until(&mut [&mut ana], 15.0, |b| b[0].has(|m| matches!(
            m,
            ServerMessage::EntitiesMoved(list) if list.iter().any(|(id, _, _)| ids.contains(id))
        ))),
        "animals never moved"
    );

    // Animals appear 20+ blocks away, out of reach: the server refuses to
    // let the bot feed one from there. (Feeding and shearing up close are
    // checked by playing.)
    let (far_id, _, _) = mob_ids(&ana)[0].clone();
    ana.received.clear();
    ana.send(&ClientMessage::UseEntity {
        id: far_id,
        held: item("core:apple"),
    });
    run_until(&mut [&mut ana], 1.0, |_| false);
    assert!(
        !ana.has(|m| matches!(m, ServerMessage::TakeItems(_))),
        "fed an animal from far away"
    );

    // ---- Up close: /summon two sheep right in front of us.
    ana.received.clear();
    for _ in 0..2 {
        ana.send(&ClientMessage::Chat {
            text: "/summon sheep".into(),
        });
    }
    let sheep = |b: &Bot| -> Vec<u64> {
        b.received
            .iter()
            .filter_map(|m| match m {
                ServerMessage::EntitySpawned { id, model, .. } if model == "@mob:core:sheep" => {
                    Some(*id)
                }
                _ => None,
            })
            .collect()
    };
    assert!(run_until(&mut [&mut ana], 5.0, |b| sheep(b[0]).len() >= 2));
    let pair = sheep(&ana);
    // Wait for the server to start looking after them.
    run_until(&mut [&mut ana], 2.5, |_| false);

    // Feeding both: the apples are taken, and soon there's a lamb.
    for id in &pair {
        ana.send(&ClientMessage::UseEntity {
            id: *id,
            held: item("core:apple"),
        });
    }
    assert!(
        run_until(&mut [&mut ana], 3.0, |b| {
            b[0].received
                .iter()
                .filter(
                    |m| matches!(m, ServerMessage::TakeItems(s) if s.item == item("core:apple")),
                )
                .count()
                == 2
        }),
        "the sheep didn't eat"
    );
    assert!(ana.has(|m| matches!(
        m,
        ServerMessage::Effect {
            kind: ourcraft_mod_api::EffectKind::Hearts,
            ..
        }
    )));
    assert!(
        run_until(&mut [&mut ana], 15.0, |b| {
            b[0].has(
            |m| matches!(m, ServerMessage::EntitySpawned { model, .. } if model == "@mob:core:sheep:baby")
        )
        }),
        "no lamb"
    );

    // Shearing a third sheep: it changes look and wool pops out.
    ana.received.clear();
    ana.send(&ClientMessage::Chat {
        text: "/summon sheep".into(),
    });
    assert!(run_until(&mut [&mut ana], 5.0, |b| !sheep(b[0]).is_empty()));
    let woolly = sheep(&ana)[0];
    run_until(&mut [&mut ana], 2.5, |_| false);
    ana.send(&ClientMessage::UseEntity {
        id: woolly,
        held: item("core:shears"),
    });
    let wool_model = format!("@item:{}", item("core:wool"));
    assert!(
        run_until(&mut [&mut ana], 3.0, |b| {
            b[0].has(|m| matches!(m, ServerMessage::EntitySpawned { model, .. } if model == "@mob:core:sheep:sheared"))
                && b[0].has(|m| matches!(m, ServerMessage::EntitySpawned { model, .. } if *model == wool_model))
        }),
        "shearing didn't work"
    );

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

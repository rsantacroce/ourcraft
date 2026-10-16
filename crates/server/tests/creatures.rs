//! End-to-end test of more creatures (milestone 23): a saddled horse
//! carries you faster than walking, a cat fed a fish becomes a friend,
//! and a full bee hive gives honey.

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
fn horses_cats_and_honey() {
    let dir = std::env::temp_dir().join(format!("ourcraft-creatures-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(15),
        bind: Some(addr),
        max_view_distance: 3,
        creative: false,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));

    let mut fay = Bot::connect(addr, 95);
    assert!(run_until(&mut [&mut fay], 5.0, |b| b[0]
        .client
        .is_connected()));
    fay.send(&ClientMessage::Hello {
        name: "Zoe".into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });
    assert!(run_until(&mut [&mut fay], 20.0, |b| {
        b[0].received
            .iter()
            .filter(|m| matches!(m, ServerMessage::Column { .. }))
            .count()
            >= 49
    }));
    let (feet, content) = fay
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { state, content, .. } => {
                Some((state.position, content.clone()))
            }
            _ => None,
        })
        .unwrap();
    let registry =
        ourcraft_common::block::BlockRegistry::from_defs(content.blocks.clone()).unwrap();
    let block = |name: &str| registry.by_name(name).unwrap().0;
    let item = |name: &str| content.items.iter().position(|i| i.name == name).unwrap() as u16;
    let feet = [
        feet[0].floor() as i32,
        feet[1].floor() as i32,
        feet[2].floor() as i32,
    ];
    let at = |dx: i32, dy: i32, dz: i32| [feet[0] + dx, feet[1] + dy, feet[2] + dz];
    let set = |bot: &mut Bot, pos: [i32; 3], block: u16, held: u16| {
        bot.send(&ClientMessage::SetBlock { pos, block, held });
    };
    let changed_to = |b: &Bot, pos: [i32; 3], to: u16| {
        b.has(|m| matches!(m, ServerMessage::BlockChanged { pos: p, block } if *p == pos && *block == to))
    };

    fay.send(&ClientMessage::GameMode { creative: true });
    let summon = |bot: &mut Bot, what: &str| {
        bot.send(&ClientMessage::Chat {
            text: format!("/summon {what}"),
        });
    };
    let spawned = |b: &Bot, model: &str| {
        b.received.iter().find_map(|m| match m {
            ServerMessage::EntitySpawned { id, model: m, .. } if m == model => Some(*id),
            _ => None,
        })
    };

    // ---- A horse: saddle it, then ride (twice as fast as walking).
    fay.received.clear();
    summon(&mut fay, "horse");
    assert!(run_until(&mut [&mut fay], 5.0, |b| spawned(
        b[0],
        "@mob:core:horse"
    )
    .is_some()));
    let horse = spawned(&fay, "@mob:core:horse").unwrap();
    run_until(&mut [&mut fay], 2.5, |_| false);
    fay.send(&ClientMessage::UseEntity {
        id: horse,
        held: item("core:saddle"),
    });
    run_until(&mut [&mut fay], 0.3, |_| false);
    fay.send(&ClientMessage::UseEntity { id: horse, held: 0 });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(|m| matches!(
            m,
            ServerMessage::Riding { mount: Some(h), speed } if *h == horse && *speed > 1.5
        ))),
        "couldn't ride the horse"
    );
    fay.send(&ClientMessage::LeaveBoat);
    run_until(&mut [&mut fay], 0.5, |_| false);

    // ---- A cat, fed a fish: hearts (a friend now).
    fay.received.clear();
    summon(&mut fay, "cat");
    assert!(run_until(&mut [&mut fay], 5.0, |b| spawned(
        b[0],
        "@mob:core:cat"
    )
    .is_some()));
    let cat = spawned(&fay, "@mob:core:cat").unwrap();
    run_until(&mut [&mut fay], 2.5, |_| false);
    fay.received.clear();
    fay.send(&ClientMessage::UseEntity {
        id: cat,
        held: item("core:fish"),
    });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(|m| matches!(
            m,
            ServerMessage::Effect {
                kind: ourcraft_mod_api::EffectKind::Hearts,
                ..
            }
        ))),
        "the cat didn't make friends"
    );

    // ---- A bee hive: bone meal fills it, a bowl takes the honey.
    for dy in 1..3 {
        set(&mut fay, at(2, dy, 0), 0, 0);
    }
    let hive = block("core:beehive");
    set(&mut fay, at(2, 1, 0), hive, 0);
    run_until(&mut [&mut fay], 0.5, |_| false);
    fay.send(&ClientMessage::UseBlock {
        pos: at(2, 1, 0),
        held: item("core:bone_meal"),
    });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| changed_to(
            b[0],
            at(2, 1, 0),
            block("core:beehive_full")
        )),
        "the hive didn't fill"
    );
    fay.send(&ClientMessage::GameMode { creative: false });
    run_until(&mut [&mut fay], 0.3, |_| false);
    fay.received.clear();
    fay.send(&ClientMessage::UseBlock {
        pos: at(2, 1, 0),
        held: item("core:bowl"),
    });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::GiveItems(s) if s.item == item("core:honey_bowl"))
        )),
        "no honey"
    );
    // The new creatures are all known.
    for name in [
        "core:fish",
        "core:bee",
        "core:frog",
        "core:rabbit",
        "core:fox",
        "core:dog",
        "core:donkey",
        "core:puff",
        "core:rumble",
    ] {
        assert!(content.mobs.iter().any(|m| m.name == name), "{name}");
    }

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

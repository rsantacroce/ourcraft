//! End-to-end test of building blocks (milestone 14): doors open as a
//! pair, signs remember their words, chests keep things, beds skip the
//! night, and breaking one half of a door takes the other.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::protocol::{
    self, Channel, ClientMessage, NetStack, PROTOCOL_ID, PROTOCOL_VERSION, ServerMessage,
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
fn doors_signs_chests_and_beds() {
    let dir = std::env::temp_dir().join(format!("ourcraft-building-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(14),
        bind: Some(addr),
        max_view_distance: 3,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));

    let mut lee = Bot::connect(addr, 31);
    assert!(run_until(&mut [&mut lee], 5.0, |b| b[0]
        .client
        .is_connected()));
    lee.send(&ClientMessage::Hello {
        name: "Lee".into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });
    assert!(run_until(&mut [&mut lee], 20.0, |b| {
        b[0].received
            .iter()
            .filter(|m| matches!(m, ServerMessage::Column { .. }))
            .count()
            >= 49
    }));
    let (feet, content) = lee
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
    let id = |name: &str| registry.by_name(name).unwrap();
    let feet = [
        feet[0].floor() as i32,
        feet[1].floor() as i32,
        feet[2].floor() as i32,
    ];
    let at = |dx: i32, dy: i32, dz: i32| [feet[0] + dx, feet[1] + dy, feet[2] + dz];
    let place = |bot: &mut Bot, pos: [i32; 3], block: ourcraft_common::block::BlockId| {
        bot.send(&ClientMessage::SetBlock {
            pos,
            block: block.0,
            held: 0,
        });
    };
    let pause = |bot: &mut Bot| {
        run_until(&mut [bot], 0.4, |_| false);
    };
    // Clear a little space above the ground first (trees!).
    for dx in 1..=3 {
        for dy in 0..=2 {
            for dz in -1..=1 {
                place(
                    &mut lee,
                    at(dx, dy, dz),
                    ourcraft_common::block::BlockId::AIR,
                );
            }
        }
    }
    pause(&mut lee);

    // ---- A door: two halves; using it opens both.
    let door = registry
        .placement(id("core:door"), [0, 1, 0], [1.0, 0.0, 0.0])
        .unwrap();
    let (offset, top) = door.extra.unwrap();
    place(&mut lee, at(2, 0, 0), door.block);
    place(
        &mut lee,
        [at(2, 0, 0)[0], at(2, 0, 0)[1] + offset[1], at(2, 0, 0)[2]],
        top,
    );
    pause(&mut lee);
    lee.received.clear();
    lee.send(&ClientMessage::UseBlock {
        pos: at(2, 0, 0),
        held: 0,
    });
    let open_lower = registry.toggled(door.block).unwrap()[0].1;
    assert!(
        run_until(&mut [&mut lee], 3.0, |b| {
            b[0].received
                .iter()
                .filter(|m| matches!(m, ServerMessage::BlockChanged { .. }))
                .count()
                == 2
                && b[0].has(|m| matches!(m, ServerMessage::BlockChanged { block, .. } if *block == open_lower.0))
        }),
        "the door didn't open"
    );

    // ---- A sign: what you write is told to everyone.
    let sign = registry
        .placement(id("core:sign"), [0, 1, 0], [1.0, 0.0, 0.0])
        .unwrap()
        .block;
    place(&mut lee, at(1, 0, 1), sign);
    pause(&mut lee);
    lee.send(&ClientMessage::SetSign {
        pos: at(1, 0, 1),
        lines: vec![
            "Welcome to".into(),
            "Lee's house!".into(),
            "\u{7}bell".into(),
        ],
    });
    assert!(
        run_until(&mut [&mut lee], 3.0, |b| {
            b[0].has(|m| matches!(
            m,
            ServerMessage::SignText { lines, .. } if lines[1] == "Lee's house!" && lines[2] == "bell"
        ))
        }),
        "sign text"
    );

    // ---- A chest: 27 slots, anything goes in.
    place(&mut lee, at(1, 0, -1), id("core:chest"));
    pause(&mut lee);
    lee.send(&ClientMessage::OpenContainer { pos: at(1, 0, -1) });
    assert!(run_until(&mut [&mut lee], 3.0, |b| {
        b[0].has(|m| matches!(
        m,
        ServerMessage::Container(c) if c.kind == ourcraft_common::container::ContainerKind::Chest && c.slots.len() == 27
    ))
    }));
    lee.received.clear();
    lee.send(&ClientMessage::ContainerClick {
        slot: 13,
        cursor: Some(NetStack {
            item: id("core:dirt").0,
            count: 5,
            wear: 0,
        }),
        secondary: false,
    });
    assert!(run_until(&mut [&mut lee], 3.0, |b| {
        b[0].has(|m| matches!(m, ServerMessage::ContainerCursor(None)))
            && b[0].has(|m| matches!(m, ServerMessage::Container(c) if c.slots[13].is_some_and(|s| s.count == 5)))
    }));
    lee.send(&ClientMessage::CloseContainer);

    // ---- A bed: by day it only remembers you; at night everyone (just
    // Lee) asleep means morning.
    let bed = registry
        .placement(id("core:bed"), [0, 1, 0], [0.0, 0.0, 1.0])
        .unwrap();
    let (head_offset, head) = bed.extra.unwrap();
    let foot = at(3, 0, -1);
    place(&mut lee, foot, bed.block);
    place(
        &mut lee,
        [foot[0] + head_offset[0], foot[1], foot[2] + head_offset[2]],
        head,
    );
    pause(&mut lee);
    lee.send(&ClientMessage::Chat {
        text: "/time night".into(),
    });
    pause(&mut lee);
    lee.received.clear();
    lee.send(&ClientMessage::UseBlock { pos: foot, held: 0 });
    assert!(
        run_until(&mut [&mut lee], 3.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::TimeOfDay(t) if (0.25..0.3).contains(t))
        )),
        "the night didn't end"
    );
    // Lee lay down in the bed, and got up in the morning.
    assert!(lee.has(|m| matches!(m, ServerMessage::Sleeping { bed: Some(b) } if *b == foot)));
    assert!(lee.has(|m| matches!(m, ServerMessage::Sleeping { bed: None })));

    // ---- Breaking the door's bottom takes its top too.
    lee.received.clear();
    place(&mut lee, at(2, 0, 0), ourcraft_common::block::BlockId::AIR);
    let top_pos = at(2, 1, 0);
    assert!(
        run_until(&mut [&mut lee], 3.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::BlockChanged { pos, block: 0 } if *pos == top_pos)
        )),
        "the door's top half stayed"
    );

    // ---- Signs and beds are saved.
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let storage = ourcraft_common::save::WorldStorage::open(&dir).unwrap();
    assert_eq!(storage.load_signs().unwrap().len(), 1);
    assert_eq!(storage.load_beds().unwrap()[0].0, "Lee");
    let _ = std::fs::remove_dir_all(&dir);
}

//! End-to-end test of trees and stone (milestone 19): bone meal grows a
//! sapling into a tree, boats go down, carry you and come back up, and
//! water on still lava makes obsidian.

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
fn saplings_boats_and_obsidian() {
    let dir = std::env::temp_dir().join(format!("ourcraft-trees-{}", std::process::id()));
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

    let mut fay = Bot::connect(addr, 61);
    assert!(run_until(&mut [&mut fay], 5.0, |b| b[0]
        .client
        .is_connected()));
    fay.send(&ClientMessage::Hello {
        name: "Tam".into(),
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

    // Creative, so we can build freely.
    fay.send(&ClientMessage::GameMode { creative: true });
    let dirt = block("core:dirt");
    // Clear a tall, wide space with dirt underneath.
    for dy in 0..12 {
        for dx in -1..=5 {
            for dz in -3..=3 {
                set(&mut fay, at(dx + 3, dy, dz), 0, 0);
            }
        }
    }
    set(&mut fay, at(5, -1, 0), dirt, 0);
    run_until(&mut [&mut fay], 1.0, |_| false);

    // ---- A sapling and some bone meal: a tree.
    let sapling = block("core:birch_sapling");
    let log = block("core:birch_log");
    set(&mut fay, at(5, 0, 0), sapling, 0);
    run_until(&mut [&mut fay], 0.5, |_| false);
    fay.received.clear();
    for _ in 0..30 {
        fay.send(&ClientMessage::UseBlock {
            pos: at(5, 0, 0),
            held: item("core:bone_meal"),
        });
        if run_until(&mut [&mut fay], 0.15, |b| {
            changed_to(b[0], at(5, 0, 0), log)
        }) {
            break;
        }
    }
    assert!(
        changed_to(&fay, at(5, 0, 0), log),
        "the sapling didn't grow"
    );
    assert!(changed_to(&fay, at(5, 3, 0), log), "no trunk");

    // ---- A boat: down on the ground, in, out, and picked up.
    fay.received.clear();
    fay.send(&ClientMessage::UseBlock {
        pos: at(2, -1, 0),
        held: item("core:birch_boat"),
    });
    let boat = |b: &Bot| {
        b.received.iter().find_map(|m| match m {
            ServerMessage::EntitySpawned { id, model, .. } if model == "@boat:birch" => Some(*id),
            _ => None,
        })
    };
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| boat(b[0]).is_some()),
        "no boat"
    );
    let id = boat(&fay).unwrap();
    fay.send(&ClientMessage::UseEntity { id, held: 0 });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::Boating { boat: Some(b) } if *b == id)
        )),
        "couldn't get in"
    );
    fay.send(&ClientMessage::LeaveBoat);
    assert!(run_until(&mut [&mut fay], 2.0, |b| b[0]
        .has(|m| matches!(m, ServerMessage::Boating { boat: None }))));
    fay.send(&ClientMessage::HitEntity { id, held: 0 });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0]
            .has(|m| matches!(m, ServerMessage::EntityRemoved(e) if *e == id))),
        "the boat wasn't picked up"
    );

    // ---- Water next to still lava: obsidian.
    let lava = block("core:lava");
    let obsidian = block("core:obsidian");
    set(&mut fay, at(1, -1, -2), dirt, 0);
    set(&mut fay, at(2, -1, -2), dirt, 0);
    set(&mut fay, at(1, 0, -2), lava, 0);
    run_until(&mut [&mut fay], 0.5, |_| false);
    fay.received.clear();
    set(&mut fay, at(2, 0, -2), block("core:water"), 0);
    assert!(
        run_until(&mut [&mut fay], 5.0, |b| changed_to(
            b[0],
            at(1, 0, -2),
            obsidian
        )),
        "no obsidian"
    );

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

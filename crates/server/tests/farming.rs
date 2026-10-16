//! End-to-end test of farming (milestone 17): a hoe makes farmland,
//! seeds only go on farmland, bone meal makes them grow, ripe wheat gives
//! wheat and seeds, cake is eaten a slice at a time, and a full composter
//! gives bone meal.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::block::BlockId;
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
fn hoe_seeds_bone_meal_harvest_cake_and_compost() {
    let dir = std::env::temp_dir().join(format!("ourcraft-farming-{}", std::process::id()));
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

    let mut fay = Bot::connect(addr, 51);
    assert!(run_until(&mut [&mut fay], 5.0, |b| b[0]
        .client
        .is_connected()));
    fay.send(&ClientMessage::Hello {
        name: "Fay".into(),
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
    let give = |bot: &mut Bot, stacks: &[(u16, u16)]| {
        bot.send(&ClientMessage::Inventory {
            slots: stacks
                .iter()
                .map(|(item, count)| {
                    Some(NetStack {
                        item: *item,
                        count: *count,
                        wear: 0,
                    })
                })
                .collect(),
            selected: 0,
        });
    };

    // A patch of dirt in front of us (in creative, to build it).
    let dirt = block("core:dirt");
    fay.send(&ClientMessage::GameMode { creative: true });
    for dx in 2..=3 {
        set(&mut fay, at(dx, -1, 0), dirt, 0);
        set(&mut fay, at(dx, 0, 0), 0, 0);
        set(&mut fay, at(dx, 1, 0), 0, 0);
    }
    run_until(&mut [&mut fay], 0.5, |_| false);
    fay.send(&ClientMessage::GameMode { creative: false });
    run_until(&mut [&mut fay], 0.3, |_| false);

    // ---- Seeds don't grow on dirt; a hoe makes farmland; then they do.
    let wheat = block("core:wheat_crop");
    fay.received.clear();
    set(&mut fay, at(2, 0, 0), wheat, item("core:wheat_seeds"));
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| changed_to(b[0], at(2, 0, 0), 0)),
        "planted on bare dirt"
    );
    let farmland = block("core:farmland");
    set(&mut fay, at(3, -1, 0), farmland, item("core:stone_shovel"));
    set(&mut fay, at(2, -1, 0), farmland, item("core:wood_hoe"));
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| changed_to(
            b[0],
            at(2, -1, 0),
            farmland
        ) && changed_to(
            b[0],
            at(3, -1, 0),
            dirt
        )),
        "the hoe didn't till (or the shovel did)"
    );
    fay.received.clear();
    set(&mut fay, at(2, 0, 0), wheat, item("core:wheat_seeds"));
    assert!(run_until(&mut [&mut fay], 2.0, |b| changed_to(
        b[0],
        at(2, 0, 0),
        wheat
    )));

    // ---- Bone meal makes it grow (and is used up).
    let ripe = registry.advanced(BlockId(wheat), u8::MAX).unwrap().0;
    give(&mut fay, &[(item("core:bone_meal"), 10)]);
    run_until(&mut [&mut fay], 0.3, |_| false);
    for _ in 0..8 {
        fay.send(&ClientMessage::UseBlock {
            pos: at(2, 0, 0),
            held: item("core:bone_meal"),
        });
        run_until(&mut [&mut fay], 0.15, |_| false);
    }
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| changed_to(
            b[0],
            at(2, 0, 0),
            ripe
        )),
        "bone meal didn't ripen the wheat"
    );
    assert!(
        fay.has(|m| matches!(m, ServerMessage::TakeItems(s) if s.item == item("core:bone_meal")))
    );

    // ---- Ripe wheat gives wheat and seeds.
    fay.received.clear();
    set(&mut fay, at(2, 0, 0), 0, 0);
    let dropped = |b: &Bot, name: &str| {
        let model = format!("@item:{}", item(name));
        b.has(|m| matches!(m, ServerMessage::EntitySpawned { model: m, .. } if *m == model))
    };
    assert!(
        run_until(&mut [&mut fay], 3.0, |b| dropped(b[0], "core:wheat")
            && dropped(b[0], "core:wheat_seeds")),
        "no harvest"
    );

    // ---- A cake, eaten a slice at a time (once we're a bit hungry).
    let cake = block("core:cake");
    fay.send(&ClientMessage::GameMode { creative: true });
    set(&mut fay, at(3, 0, 0), cake, 0);
    run_until(&mut [&mut fay], 0.5, |_| false);
    fay.received.clear();
    fay.send(&ClientMessage::UseBlock {
        pos: at(3, 0, 0),
        held: 0,
    });
    let bitten = registry.advanced(BlockId(cake), 1).unwrap().0;
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| changed_to(
            b[0],
            at(3, 0, 0),
            bitten
        )),
        "couldn't eat the cake"
    );

    // ---- A composter: fill it up, get bone meal.
    let composter = block("core:composter");
    set(&mut fay, at(3, 0, 0), 0, 0);
    run_until(&mut [&mut fay], 0.3, |_| false);
    set(&mut fay, at(3, 0, 0), composter, 0);
    run_until(&mut [&mut fay], 0.5, |_| false);
    fay.received.clear();
    for _ in 0..12 {
        fay.send(&ClientMessage::UseBlock {
            pos: at(3, 0, 0),
            held: item("core:pumpkin"),
        });
        run_until(&mut [&mut fay], 0.1, |_| false);
    }
    assert!(
        run_until(&mut [&mut fay], 3.0, |b| dropped(b[0], "core:bone_meal")),
        "the composter gave no bone meal"
    );

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

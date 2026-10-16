//! End-to-end test of the living world (milestone 15): water flows, sand
//! falls, survival players have health and hunger, golden apples can be
//! eaten, monsters come out at night, and /peaceful sends them away.

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
fn flowing_falling_eating_and_monsters() {
    let dir = std::env::temp_dir().join(format!("ourcraft-living-{}", std::process::id()));
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

    let mut max = Bot::connect(addr, 41);
    assert!(run_until(&mut [&mut max], 5.0, |b| b[0]
        .client
        .is_connected()));
    max.send(&ClientMessage::Hello {
        name: "Max".into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });
    assert!(run_until(&mut [&mut max], 20.0, |b| {
        b[0].received
            .iter()
            .filter(|m| matches!(m, ServerMessage::Column { .. }))
            .count()
            >= 49
    }));
    let (feet, content) = max
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
    let item = |name: &str| content.items.iter().position(|i| i.name == name).unwrap() as u16;
    let feet = [
        feet[0].floor() as i32,
        feet[1].floor() as i32,
        feet[2].floor() as i32,
    ];
    let at = |dx: i32, dy: i32, dz: i32| [feet[0] + dx, feet[1] + dy, feet[2] + dz];

    // ---- Survival players hear about their health and hunger.
    assert!(
        run_until(&mut [&mut max], 3.0, |b| {
            b[0].has(
            |m| matches!(m, ServerMessage::Vitals { health, food, .. } if *health == 20.0 && *food == 20.0)
        )
        }),
        "no vitals"
    );

    // ---- Pour water on a pillar: it flows down and outwards.
    let set = |bot: &mut Bot, pos: [i32; 3], block: u16| {
        bot.send(&ClientMessage::SetBlock {
            pos,
            block,
            held: 0,
        });
    };
    max.send(&ClientMessage::GameMode { creative: true });
    for dy in 0..3 {
        set(&mut max, at(3, dy, 0), 0);
        set(&mut max, at(3, dy, 1), 0);
        set(&mut max, at(2, dy, 0), 0);
        set(&mut max, at(4, dy, 0), 0);
    }
    run_until(&mut [&mut max], 0.5, |_| false);
    max.received.clear();
    set(
        &mut max,
        at(3, 2, 0),
        registry.by_name("core:water").unwrap().0,
    );
    let flowing = |b: &u16| {
        registry
            .fluid(ourcraft_common::block::BlockId(*b))
            .is_some_and(|f| f.level > 0 || f.falling)
    };
    assert!(
        run_until(&mut [&mut max], 5.0, |b| {
            b[0].received
                .iter()
                .filter(
                    |m| matches!(m, ServerMessage::BlockChanged { block, .. } if flowing(block)),
                )
                .count()
                >= 3
        }),
        "the water didn't flow"
    );

    // ---- The source can't be broken by hand, but a bucket scoops it up.
    let water = registry.by_name("core:water").unwrap().0;
    max.received.clear();
    set(&mut max, at(3, 2, 0), 0);
    assert!(
        run_until(&mut [&mut max], 2.0, |b| b[0].has(|m| matches!(
            m,
            ServerMessage::BlockChanged { pos, block } if *pos == at(3, 2, 0) && *block == water
        ))),
        "broke water by hand"
    );
    max.received.clear();
    max.send(&ClientMessage::SetBlock {
        pos: at(3, 2, 0),
        block: 0,
        held: item("core:bucket"),
    });
    assert!(
        run_until(&mut [&mut max], 2.0, |b| b[0].has(|m| matches!(
            m,
            ServerMessage::BlockChanged { pos, block } if *pos == at(3, 2, 0) && *block == 0
        ))),
        "the bucket didn't scoop up the water"
    );

    // ---- Sand placed in the air falls.
    set(&mut max, at(-3, 0, -3), 0);
    set(&mut max, at(-3, 1, -3), 0);
    set(&mut max, at(-3, 2, -3), 0);
    run_until(&mut [&mut max], 0.3, |_| false);
    max.received.clear();
    let sand = registry.by_name("core:sand").unwrap().0;
    set(&mut max, at(-3, 2, -3), sand);
    let lower = at(-3, 1, -3);
    assert!(
        run_until(&mut [&mut max], 3.0, |b| {
            b[0].has(
            |m| matches!(m, ServerMessage::BlockChanged { pos, block } if *pos == lower && *block == sand)
        )
        }),
        "the sand didn't fall"
    );
    max.send(&ClientMessage::GameMode { creative: false });

    // ---- A golden apple can always be eaten (it heals).
    max.send(&ClientMessage::Inventory {
        slots: vec![Some(NetStack {
            item: item("core:golden_apple"),
            count: 2,
            wear: 0,
        })],
        selected: 0,
    });
    run_until(&mut [&mut max], 0.3, |_| false);
    max.received.clear();
    max.send(&ClientMessage::Eat {
        item: item("core:golden_apple"),
    });
    assert!(run_until(&mut [&mut max], 3.0, |b| b[0].has(
        |m| matches!(m, ServerMessage::TakeItems(s) if s.item == item("core:golden_apple"))
    )));
    // An apple isn't (we're not hungry), and neither is something we don't have.
    max.received.clear();
    max.send(&ClientMessage::Eat {
        item: item("core:apple"),
    });
    run_until(&mut [&mut max], 0.5, |_| false);
    assert!(!max.has(|m| matches!(m, ServerMessage::TakeItems(_))));

    // ---- The weather is announced, and /weather changes it.
    assert!(run_until(&mut [&mut max], 8.0, |b| b[0].has(|m| matches!(
        m,
        ServerMessage::Weather { raining: false }
    ))));
    max.send(&ClientMessage::Chat {
        text: "/weather rain".into(),
    });
    assert!(
        run_until(&mut [&mut max], 3.0, |b| b[0]
            .has(|m| matches!(m, ServerMessage::Weather { raining: true }))),
        "no rain"
    );

    // ---- Night brings monsters; /peaceful sends them away.
    max.send(&ClientMessage::Chat {
        text: "/time night".into(),
    });
    let monster = |m: &ServerMessage| {
        matches!(m, ServerMessage::EntitySpawned { model, .. }
            if model.starts_with("@mob:core:grumble") || model.starts_with("@mob:core:bouncer"))
    };
    assert!(
        run_until(&mut [&mut max], 15.0, |b| b[0].has(monster)),
        "no monsters at night"
    );
    let ids: Vec<u64> = max
        .received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::EntitySpawned { id, .. } if monster(m) => Some(*id),
            _ => None,
        })
        .collect();
    max.send(&ClientMessage::Chat {
        text: "/peaceful".into(),
    });
    assert!(
        run_until(&mut [&mut max], 20.0, |b| {
            ids.iter()
                .all(|id| b[0].has(|m| matches!(m, ServerMessage::EntityRemoved(r) if r == id)))
        }),
        "monsters stayed in peaceful"
    );

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let storage = ourcraft_common::save::WorldStorage::open(&dir).unwrap();
    assert!(
        storage
            .load_rules()
            .unwrap()
            .iter()
            .any(|(k, v)| k == "peaceful" && *v)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

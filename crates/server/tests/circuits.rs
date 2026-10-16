//! End-to-end test of circuits and transport (milestone 22): a lever
//! powers wire that lights a lamp and pushes a piston; rails join up; a
//! cart on a powered rail rolls with its rider.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::block::BlockId;
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
fn levers_wire_lamps_pistons_rails_and_carts() {
    let dir = std::env::temp_dir().join(format!("ourcraft-circuits-{}", std::process::id()));
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

    let mut fay = Bot::connect(addr, 91);
    assert!(run_until(&mut [&mut fay], 5.0, |b| b[0]
        .client
        .is_connected()));
    fay.send(&ClientMessage::Hello {
        name: "Ada".into(),
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
    // A stone floor two up (so the ground's shape doesn't matter).
    let stone = block("core:stone");
    for dx in 1..=8 {
        for dz in -1..=3 {
            for dy in 2..5 {
                set(&mut fay, at(dx, dy, dz), 0, 0);
            }
            set(&mut fay, at(dx, 1, dz), stone, 0);
        }
    }
    run_until(&mut [&mut fay], 1.0, |_| false);
    let y = 2;
    // Lever, three wire, a lamp at the end; a piston beside the last wire
    // pushing east into a block.
    let lever = block("core:lever");
    let wire = block("core:spark_wire");
    let lamp = block("core:spark_lamp");
    set(&mut fay, at(1, y, 0), lever, 0);
    for dx in 2..=4 {
        set(&mut fay, at(dx, y, 0), wire, 0);
    }
    set(&mut fay, at(5, y, 0), lamp, 0);
    let piston = registry
        .placement(BlockId(block("core:piston")), [0, 1, 0], [1.0, 0.0, 0.0])
        .unwrap()
        .block;
    set(&mut fay, at(4, y, 1), piston.0, 0);
    // The piston faces east; it pushes from (5, 1) — put a block there.
    set(&mut fay, at(5, y, 1), block("core:cobblestone"), 0);
    // A dispenser beside the middle wire, facing south, with arrows in.
    let dispenser = registry
        .placement(BlockId(block("core:dispenser")), [0, 1, 0], [0.0, 0.0, 1.0])
        .unwrap()
        .block;
    set(&mut fay, at(3, y, 1), dispenser.0, 0);
    run_until(&mut [&mut fay], 0.5, |_| false);
    fay.send(&ClientMessage::OpenContainer { pos: at(3, y, 1) });
    run_until(&mut [&mut fay], 0.3, |_| false);
    fay.send(&ClientMessage::ContainerClick {
        slot: 0,
        cursor: Some(ourcraft_common::protocol::NetStack {
            item: item("core:arrow"),
            count: 2,
            wear: 0,
        }),
        secondary: false,
    });
    run_until(&mut [&mut fay], 0.3, |_| false);
    fay.send(&ClientMessage::CloseContainer);
    run_until(&mut [&mut fay], 1.0, |_| false);
    let lit = registry.powered(BlockId(lamp), true).unwrap().0;

    // ---- Flip the lever: the lamp lights and the piston pushes.
    fay.received.clear();
    fay.send(&ClientMessage::UseBlock {
        pos: at(1, y, 0),
        held: 0,
    });
    assert!(
        run_until(&mut [&mut fay], 3.0, |b| changed_to(b[0], at(5, y, 0), lit)),
        "the lamp didn't light"
    );
    assert!(
        run_until(&mut [&mut fay], 3.0, |b| changed_to(
            b[0],
            at(6, y, 1),
            block("core:cobblestone")
        )),
        "the piston didn't push"
    );
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::EntitySpawned { model, .. } if model == "@arrow")
        )),
        "the dispenser didn't shoot"
    );
    // ---- And off again.
    fay.received.clear();
    fay.send(&ClientMessage::UseBlock {
        pos: at(1, y, 0),
        held: 0,
    });
    assert!(
        run_until(&mut [&mut fay], 3.0, |b| changed_to(
            b[0],
            at(5, y, 0),
            lamp
        )),
        "the lamp didn't go out"
    );

    // ---- Rails join round a corner.
    let rail = block("core:rail");
    set(&mut fay, at(2, y, 3), rail, 0);
    set(&mut fay, at(3, y, 3), rail, 0);
    set(&mut fay, at(3, y, 2), rail, 0);
    run_until(&mut [&mut fay], 1.0, |_| false);
    // (3, 3) has rails to the west and north: a corner.
    let corner = registry
        .rail_joining(
            "core:rail",
            ourcraft_common::shapes::Dir4::West,
            ourcraft_common::shapes::Dir4::North,
        )
        .unwrap()
        .0;
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| changed_to(
            b[0],
            at(3, y, 3),
            corner
        )),
        "the rails didn't join"
    );

    // ---- A cart on a powered rail (powered by a spark block) rolls off
    // with us in it.
    for dx in 1..=8 {
        set(&mut fay, at(dx, y, -1), 0, 0);
    }
    run_until(&mut [&mut fay], 0.5, |_| false);
    // The spark block under the powered rail goes first (rails need
    // something under them).
    set(&mut fay, at(2, y - 1, -1), 0, 0);
    run_until(&mut [&mut fay], 0.3, |_| false);
    set(&mut fay, at(2, y - 1, -1), block("core:spark_block"), 0);
    run_until(&mut [&mut fay], 0.3, |_| false);
    let powered = registry
        .placement(
            BlockId(block("core:powered_rail")),
            [0, 1, 0],
            [1.0, 0.0, 0.0],
        )
        .unwrap()
        .block;
    set(&mut fay, at(2, y, -1), powered.0, 0);
    let east = registry
        .placement(BlockId(rail), [0, 1, 0], [1.0, 0.0, 0.0])
        .unwrap()
        .block;
    for dx in 3..=6 {
        set(&mut fay, at(dx, y, -1), east.0, 0);
    }
    // Then a step up: a block, with the last rails on top.
    set(&mut fay, at(7, y, -1), stone, 0);
    set(&mut fay, at(8, y, -1), stone, 0);
    run_until(&mut [&mut fay], 0.3, |_| false);
    for dx in 7..=8 {
        set(&mut fay, at(dx, y + 1, -1), 0, 0);
    }
    run_until(&mut [&mut fay], 0.3, |_| false);
    for dx in 7..=8 {
        set(&mut fay, at(dx, y + 1, -1), east.0, 0);
    }
    run_until(&mut [&mut fay], 1.0, |_| false);
    fay.received.clear();
    fay.send(&ClientMessage::UseBlock {
        pos: at(2, y, -1),
        held: item("core:minecart"),
    });
    let cart = |b: &Bot| {
        b.received.iter().find_map(|m| match m {
            ServerMessage::EntitySpawned { id, model, .. } if model == "@cart" => Some(*id),
            _ => None,
        })
    };
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| cart(b[0]).is_some()),
        "no cart"
    );
    let id = cart(&fay).unwrap();
    fay.send(&ClientMessage::UseEntity { id, held: 0 });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::InCart { cart: Some(c) } if *c == id)
        )),
        "couldn't get in"
    );
    let start_x = feet[0] as f32 + 2.5;
    assert!(
        run_until(&mut [&mut fay], 4.0, |b| b[0].has(|m| matches!(
            m,
            ServerMessage::EntitiesMoved(list)
                if list.iter().any(|(e, p, _)| *e == id && p[0] > start_x + 2.0)
        ))),
        "the cart didn't roll"
    );
    assert!(
        run_until(&mut [&mut fay], 4.0, |b| b[0].has(|m| matches!(
            m,
            ServerMessage::EntitiesMoved(list)
                if list.iter().any(|(e, p, _)| *e == id && p[1] > (feet[1] + y + 1) as f32)
        ))),
        "the cart didn't climb the step"
    );

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

//! End-to-end test of our own magic (milestone 24): wishing a pickaxe
//! sparkly on a wish table, and drinking a potion.

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
fn wishes_and_potions() {
    let dir = std::env::temp_dir().join(format!("ourcraft-magic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(24),
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

    let mut fay = Bot::connect(addr, 82);
    assert!(run_until(&mut [&mut fay], 5.0, |b| b[0]
        .client
        .is_connected()));
    fay.send(&ClientMessage::Hello {
        name: "Wren".into(),
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

    fay.send(&ClientMessage::GameMode { creative: true });
    for dy in 1..3 {
        for dx in 1..=3 {
            set(&mut fay, at(dx, dy, 0), 0, 0);
        }
    }
    set(&mut fay, at(2, 1, 0), block("core:wish_table"), 0);
    run_until(&mut [&mut fay], 0.5, |_| false);
    fay.send(&ClientMessage::GameMode { creative: false });
    let stack = |name: &str| {
        Some(NetStack {
            item: item(name),
            count: 1,
            wear: 0,
        })
    };
    fay.send(&ClientMessage::Inventory {
        slots: vec![
            stack("core:iron_pickaxe"),
            stack("core:wish_star"),
            stack("core:hop_potion"),
        ],
        selected: 0,
    });
    run_until(&mut [&mut fay], 0.3, |_| false);

    // ---- A wish: the pickaxe and the star go, a sparkly pickaxe comes.
    fay.received.clear();
    fay.send(&ClientMessage::UseBlock {
        pos: at(2, 1, 0),
        held: item("core:iron_pickaxe"),
    });
    let shiny = item("core:sparkly_iron_pickaxe");
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::GiveItems(s) if s.item == shiny)
        )),
        "no sparkly pickaxe"
    );
    assert!(
        fay.has(|m| matches!(m, ServerMessage::TakeItems(s) if s.item == item("core:wish_star")))
    );
    assert!(
        fay.has(
            |m| matches!(m, ServerMessage::TakeItems(s) if s.item == item("core:iron_pickaxe"))
        )
    );

    // ---- A hop potion: the effect starts and the bottle comes back.
    fay.received.clear();
    fay.send(&ClientMessage::Eat {
        item: item("core:hop_potion"),
    });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| {
            b[0].has(
            |m| matches!(m, ServerMessage::Effects { seconds } if seconds[0] > 80.0 && seconds[2] == 0.0)
        )
        }),
        "the potion did nothing"
    );
    assert!(fay.has(|m| matches!(m, ServerMessage::GiveItems(s) if s.item == item("core:bottle"))));
    assert!(
        fay.has(|m| matches!(m, ServerMessage::TakeItems(s) if s.item == item("core:hop_potion")))
    );

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

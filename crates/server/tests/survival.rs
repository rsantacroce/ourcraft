//! End-to-end test of survival (milestone 11): a real server and a bot
//! player who breaks a block, picks up what it drops, and smelts iron in a
//! furnace.

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

fn stack(item: u16, count: u16) -> Option<NetStack> {
    Some(NetStack {
        item,
        count,
        wear: 0,
    })
}

#[test]
fn drops_pickups_and_furnaces() {
    let dir = std::env::temp_dir().join(format!("ourcraft-survival-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(11),
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

    let mut kim = Bot::connect(addr, 9);
    assert!(run_until(&mut [&mut kim], 5.0, |b| b[0]
        .client
        .is_connected()));
    kim.send(&ClientMessage::Hello {
        name: "Kim".into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });
    // All the columns in view (7×7 at view distance 3).
    assert!(run_until(&mut [&mut kim], 20.0, |b| {
        b[0].received
            .iter()
            .filter(|m| matches!(m, ServerMessage::Column { .. }))
            .count()
            >= 49
    }));
    let (feet, content, creative) = kim
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome {
                state,
                content,
                creative,
                ..
            } => Some((state.position, content.clone(), *creative)),
            _ => None,
        })
        .expect("welcome");
    assert!(!creative, "a survival world starts players in survival");
    let item = |name: &str| content.items.iter().position(|i| i.name == name).unwrap() as u16;
    let feet = [
        feet[0].floor() as i32,
        feet[1].floor() as i32,
        feet[2].floor() as i32,
    ];

    // ---- Break the block we stand on: it drops as an item we pick up.
    kim.send(&ClientMessage::GameMode { creative: false });
    let ground = [feet[0], feet[1] - 1, feet[2]];
    kim.send(&ClientMessage::SetBlock {
        pos: ground,
        block: 0,
        held: 0,
    });
    assert!(
        run_until(&mut [&mut kim], 5.0, |b| {
            b[0].has(
            |m| matches!(m, ServerMessage::EntitySpawned { model, .. } if model.starts_with("@item:"))
        )
        }),
        "no item dropped"
    );
    assert!(
        run_until(&mut [&mut kim], 5.0, |b| b[0]
            .has(|m| matches!(m, ServerMessage::GiveItems(_)))),
        "item not picked up"
    );
    let given = kim
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::GiveItems(s) => Some(*s),
            _ => None,
        })
        .unwrap();
    let name = &content.items[given.item as usize].name;
    assert!(
        ["core:dirt", "core:sand", "core:gravel"].contains(&name.as_str()),
        "got {name}"
    );

    // ---- A furnace above our head smelts iron ore with coal.
    let furnace_pos = [feet[0], feet[1] + 2, feet[2]];
    kim.send(&ClientMessage::SetBlock {
        pos: furnace_pos,
        block: item("core:furnace"),
        held: item("core:furnace"),
    });
    // (The placer isn't sent its own change back; give the server a moment.)
    run_until(&mut [&mut kim], 0.3, |_| false);
    kim.send(&ClientMessage::OpenContainer { pos: furnace_pos });
    assert!(
        run_until(&mut [&mut kim], 5.0, |b| b[0]
            .has(|m| matches!(m, ServerMessage::Container(_)))),
        "furnace didn't open"
    );
    // Coal can't go in the top slot: it comes straight back.
    kim.received.clear();
    kim.send(&ClientMessage::ContainerClick {
        slot: 0,
        cursor: stack(item("core:coal"), 1),
        secondary: false,
    });
    assert!(run_until(&mut [&mut kim], 5.0, |b| b[0].has(
        |m| matches!(m, ServerMessage::ContainerCursor(Some(s)) if s.item == item("core:coal"))
    )));
    // Ore on top, coal below.
    kim.send(&ClientMessage::ContainerClick {
        slot: 0,
        cursor: stack(item("core:iron_ore"), 2),
        secondary: false,
    });
    kim.send(&ClientMessage::ContainerClick {
        slot: 1,
        cursor: stack(item("core:coal"), 1),
        secondary: false,
    });
    let ingot = item("core:iron_ingot");
    assert!(
        run_until(&mut [&mut kim], 15.0, |b| {
            b[0].has(|m| matches!(
            m,
            ServerMessage::Container(c) if c.slots[2].is_some_and(|s| s.item == ingot && s.count == 2)
        ))
        }),
        "no ingots after smelting"
    );
    // Take them out.
    kim.received.clear();
    kim.send(&ClientMessage::ContainerClick {
        slot: 2,
        cursor: None,
        secondary: false,
    });
    assert!(run_until(&mut [&mut kim], 5.0, |b| b[0].has(
        |m| matches!(m, ServerMessage::ContainerCursor(Some(s)) if s.item == ingot && s.count == 2)
    )));

    // ---- Furnaces are saved with the world.
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let storage = ourcraft_common::save::WorldStorage::open(&dir).unwrap();
    let saved = storage.load_containers().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].pos, furnace_pos);
    let _ = std::fs::remove_dir_all(&dir);
}

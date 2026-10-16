//! End-to-end test of tools and adventure (milestone 20): fishing, an
//! arrow at a target, cooking on a campfire, armour others can see, and
//! riding a saddled pig.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bevy::math::Vec3;
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
fn fishing_arrows_campfires_armour_and_riding() {
    let dir = std::env::temp_dir().join(format!("ourcraft-adventure-{}", std::process::id()));
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

    let mut fay = Bot::connect(addr, 71);
    assert!(run_until(&mut [&mut fay], 5.0, |b| b[0]
        .client
        .is_connected()));
    fay.send(&ClientMessage::Hello {
        name: "Ivy".into(),
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
    let _changed_to = |b: &Bot, pos: [i32; 3], to: u16| {
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

    let my_id = 71;
    // Build in creative: a pool, a target, a campfire.
    fay.send(&ClientMessage::GameMode { creative: true });
    for dy in 1..6 {
        for dx in 1..=6 {
            for dz in -2..=2 {
                set(&mut fay, at(dx, dy, dz), 0, 0);
            }
        }
    }
    // (One block up, in case the ground isn't flat.)
    set(&mut fay, at(2, 1, 0), block("core:water"), 0);
    set(&mut fay, at(5, 1, 0), block("core:target"), 0);
    set(&mut fay, at(4, 1, 2), block("core:campfire"), 0);
    run_until(&mut [&mut fay], 1.0, |_| false);
    fay.send(&ClientMessage::GameMode { creative: false });
    run_until(&mut [&mut fay], 0.3, |_| false);
    give(
        &mut fay,
        &[
            (item("core:fishing_rod"), 1),
            (item("core:fish"), 3),
            (item("core:bow"), 1),
            (item("core:arrow"), 8),
        ],
    );
    run_until(&mut [&mut fay], 0.3, |_| false);
    let dropped = |b: &Bot, name: &str| {
        let model = format!("@item:{}", item(name));
        b.has(|m| matches!(m, ServerMessage::EntitySpawned { model: m, .. } if *m == model))
    };

    // ---- Fishing: a bobber, then (soon) a catch.
    fay.received.clear();
    fay.send(&ClientMessage::UseBlock {
        pos: at(2, 1, 0),
        held: item("core:fishing_rod"),
    });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::EntitySpawned { model, .. } if model == "@bobber")
        )),
        "no bobber: {:?}",
        fay.received
            .iter()
            .filter(|m| !matches!(
                m,
                ServerMessage::Column { .. }
                    | ServerMessage::PlayerStates { .. }
                    | ServerMessage::YourState { .. }
            ))
            .take(30)
            .collect::<Vec<_>>()
    );
    assert!(
        run_until(&mut [&mut fay], 15.0, |b| b[0]
            .has(|m| matches!(m, ServerMessage::GiveItems(_)))),
        "nothing bit"
    );

    // ---- An arrow at the target: confetti, and the arrow comes back.
    fay.received.clear();
    let eye = Vec3::new(
        feet[0] as f32 + 0.5,
        feet[1] as f32 + 1.62,
        feet[2] as f32 + 0.5,
    );
    let aim = Vec3::new(
        feet[0] as f32 + 5.5,
        feet[1] as f32 + 1.5,
        feet[2] as f32 + 0.5,
    ) - eye;
    fay.send(&ClientMessage::UseItem {
        held: item("core:bow"),
        direction: aim.normalize().to_array(),
    });
    assert!(
        run_until(&mut [&mut fay], 3.0, |b| b[0].has(|m| matches!(
            m,
            ServerMessage::Effect {
                kind: ourcraft_mod_api::EffectKind::Confetti,
                ..
            }
        ))),
        "the arrow missed the target"
    );
    assert!(run_until(&mut [&mut fay], 2.0, |b| dropped(
        b[0],
        "core:arrow"
    )));

    // ---- A fish on the campfire becomes cooked fish.
    fay.received.clear();
    fay.send(&ClientMessage::UseBlock {
        pos: at(4, 1, 2),
        held: item("core:fish"),
    });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::TakeItems(s) if s.item == item("core:fish"))
        )),
        "the campfire didn't take the fish"
    );
    assert!(
        run_until(&mut [&mut fay], 10.0, |b| dropped(b[0], "core:cooked_fish")),
        "nothing cooked"
    );

    // ---- Armour: everyone hears what we wear.
    fay.received.clear();
    let mut slots: Vec<Option<NetStack>> = vec![None; 36];
    slots.push(Some(NetStack {
        item: item("core:iron_helmet"),
        count: 1,
        wear: 0,
    }));
    // Boots in the leggings slot don't count.
    slots.push(None);
    slots.push(Some(NetStack {
        item: item("core:iron_boots"),
        count: 1,
        wear: 0,
    }));
    fay.send(&ClientMessage::Inventory { slots, selected: 0 });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(|m| matches!(
            m,
            ServerMessage::Armour { player, pieces }
                if *player == my_id && pieces[0] == item("core:iron_helmet") && pieces[2] == 0
        ))),
        "armour wasn't announced"
    );

    // ---- A saddled pig can be ridden.
    fay.send(&ClientMessage::GameMode { creative: true });
    fay.received.clear();
    fay.send(&ClientMessage::Chat {
        text: "/summon pig".into(),
    });
    let pig = |b: &Bot| {
        b.received.iter().find_map(|m| match m {
            ServerMessage::EntitySpawned { id, model, .. } if model == "@mob:core:pig" => Some(*id),
            _ => None,
        })
    };
    assert!(run_until(&mut [&mut fay], 5.0, |b| pig(b[0]).is_some()));
    let id = pig(&fay).unwrap();
    run_until(&mut [&mut fay], 2.5, |_| false);
    fay.send(&ClientMessage::UseEntity {
        id,
        held: item("core:saddle"),
    });
    run_until(&mut [&mut fay], 0.3, |_| false);
    fay.send(&ClientMessage::UseEntity { id, held: 0 });
    assert!(
        run_until(&mut [&mut fay], 2.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::Riding { mount: Some(m) } if *m == id)
        )),
        "couldn't ride the pig"
    );
    fay.send(&ClientMessage::LeaveBoat);
    assert!(run_until(&mut [&mut fay], 2.0, |b| b[0]
        .has(|m| matches!(m, ServerMessage::Riding { mount: None }))));

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

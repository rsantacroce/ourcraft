//! End-to-end test of the modding guide's tutorial mod, Snow Fun: a real
//! server with the bundled mods and a headless "bot" player over UDP.
//!
//! Needs `mods/dist/snow_fun.wasm` (run `mods/build.sh`); skipped otherwise.

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
fn snow_fun_commands_and_snowballs() {
    let wasm = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods/dist/snow_fun.wasm");
    if !std::path::Path::new(wasm).exists() {
        eprintln!("skipping: {wasm} not built");
        return;
    }
    let dir = std::env::temp_dir().join(format!("ourcraft-snow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(77),
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

    let mut sam = Bot::connect(addr, 7);
    assert!(run_until(&mut [&mut sam], 5.0, |b| b[0]
        .client
        .is_connected()));
    sam.send(&ClientMessage::Hello {
        name: "Sam".into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });

    // Step 1: a hello in chat.
    assert!(
        run_until(&mut [&mut sam], 10.0, |b| b[0].has(
            |m| matches!(m, ServerMessage::Chat { text, .. } if text.contains("hi Sam"))
        )),
        "no greeting"
    );

    // Wait for some terrain, like a real player would have.
    // All the columns in view (7×7 at view distance 3).
    assert!(run_until(&mut [&mut sam], 20.0, |b| {
        b[0].received
            .iter()
            .filter(|m| matches!(m, ServerMessage::Column { .. }))
            .count()
            >= 49
    }));

    // Step 2: the blocks, model and tool were registered.
    let content = sam
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("welcome");
    let id_of = |name: &str| {
        content
            .blocks
            .iter()
            .position(|b| b.name == name)
            .unwrap_or_else(|| panic!("{name} not registered")) as u16
    };
    let brick = id_of("snow_fun:snow_bricks");
    let lantern = id_of("snow_fun:ice_lantern");
    assert!(content.tools.iter().any(|t| t.name == "snow_fun:thrower"));
    assert!(content.models.iter().any(|m| m.name == "snow_fun:snowball"));
    assert!(
        content.recipes.iter().any(|r| matches!(
            r,
            ourcraft_common::recipes::Recipe::Shaped { result, .. } if result == "snow_fun:ice_lantern"
        )),
        "the mod's recipe reached the client"
    );
    // The snow cone: an item (not a block) with its own picture, and food.
    let cone = content
        .items
        .iter()
        .find(|i| i.name == "snow_fun:snow_cone")
        .expect("the mod's item reached the client");
    assert_eq!((cone.food, cone.block()), (3, None));
    assert!(
        content
            .textures
            .iter()
            .any(|(n, _)| n == "snow_fun:snow_cone")
    );

    // Step 3: /snowman builds bricks and a lantern head, and stays out of chat.
    sam.send(&ClientMessage::Chat {
        text: "/snowman".into(),
    });
    assert!(
        run_until(&mut [&mut sam], 5.0, |b| {
            b[0].has(
                |m| matches!(m, ServerMessage::BlockChanged { block, .. } if *block == lantern),
            )
        }),
        "no snowman; chats: {:?}, block changes: {:?}",
        sam.received
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Chat { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        sam.received
            .iter()
            .filter_map(|m| match m {
                ServerMessage::BlockChanged { pos, block } => Some((*pos, *block)),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
    assert!(!sam.has(|m| matches!(m, ServerMessage::Chat { text, .. } if text == "/snowman")));

    // Step 4: /snowballs gives the thrower; throwing straight down leaves a
    // snow brick.
    sam.send(&ClientMessage::Chat {
        text: "/snowballs".into(),
    });
    assert!(run_until(&mut [&mut sam], 5.0, |b| b[0].has(
        |m| matches!(m, ServerMessage::Tools(t) if t == &["snow_fun:thrower"])
    )));
    sam.received.clear();
    sam.send(&ClientMessage::UseTool {
        tool: "snow_fun:thrower".into(),
        direction: [0.0, -1.0, 0.0],
        secondary: false,
    });
    assert!(
        run_until(&mut [&mut sam], 5.0, |b| {
            b[0].has(|m| matches!(m, ServerMessage::EntitySpawned { model, .. } if model == "snow_fun:snowball"))
                && b[0].has(|m| matches!(m, ServerMessage::BlockChanged { block, .. } if *block == brick))
        }),
        "snowball didn't land"
    );

    // And /snowballs again gives the blocks back.
    sam.send(&ClientMessage::Chat {
        text: "/snowballs".into(),
    });
    assert!(run_until(&mut [&mut sam], 5.0, |b| b[0]
        .has(|m| matches!(m, ServerMessage::Tools(t) if t.is_empty()))));

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

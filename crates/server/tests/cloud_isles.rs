//! End-to-end test of the Cloud Isles: building a sunstone frame, lighting
//! it with a feather, standing in it to travel, not seeing players who are
//! in the other dimension, coming back to the Cloud Isles after a restart,
//! and floating home after falling off.

use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::block::BlockRegistry;
use ourcraft_common::dimension::Dimension;
use ourcraft_common::protocol::{
    self, Channel, ClientMessage, ContentPack, NetInput, NetStack, PROTOCOL_ID, PROTOCOL_VERSION,
    ServerMessage,
};
use ourcraft_server::net::connection_config;
use ourcraft_server::{SaveNowFlag, ServerConfig, StopFlag, build_app};
use renet::RenetClient;
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};

struct Bot {
    client: RenetClient,
    transport: NetcodeClientTransport,
    received: Vec<ServerMessage>,
    /// Send physics inputs (standing still) as a game would, so gravity
    /// works on the server.
    walking: bool,
    tick: u32,
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
            walking: false,
            tick: 0,
        }
    }

    fn send(&mut self, msg: &ClientMessage) {
        self.client
            .send_message(Channel::Reliable, protocol::encode(msg));
    }

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
        if self.walking && self.client.is_connected() {
            self.tick += 1;
            let input = NetInput {
                tick: self.tick,
                ..Default::default()
            };
            self.client.send_message(
                Channel::Unreliable,
                protocol::encode(&ClientMessage::Inputs(vec![input])),
            );
        }
        let _ = self.transport.send_packets(&mut self.client);
    }

    fn has(&self, f: impl Fn(&ServerMessage) -> bool) -> bool {
        self.received.iter().any(f)
    }

    fn chat(&mut self, text: &str) {
        self.send(&ClientMessage::Chat { text: text.into() });
    }

    fn moved_to(&self, dim: Dimension) -> Option<[f32; 3]> {
        self.received.iter().find_map(|m| match m {
            ServerMessage::ChangedDimension { dimension, state } if *dimension == dim => {
                Some(state.position)
            }
            _ => None,
        })
    }
}

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

fn start_server(dir: &Path) -> (SocketAddr, StopFlag, JoinHandle<()>) {
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.to_path_buf(),
        seed: Some(31),
        bind: Some(addr),
        max_view_distance: 3,
        creative: true,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));
    (addr, stop, server)
}

fn join(addr: SocketAddr, id: u64, name: &str) -> Bot {
    let mut bot = Bot::connect(addr, id);
    assert!(run_until(&mut [&mut bot], 5.0, |b| b[0]
        .client
        .is_connected()));
    bot.send(&ClientMessage::Hello {
        name: name.into(),
        view_distance: 2,
        protocol_version: PROTOCOL_VERSION,
    });
    assert!(
        run_until(&mut [&mut bot], 20.0, |b| {
            b[0].received
                .iter()
                .filter(|m| matches!(m, ServerMessage::Column { .. }))
                .count()
                >= 25
        }),
        "{name} got no land"
    );
    bot
}

fn welcome(bot: &Bot) -> (ContentPack, Dimension, [f32; 3]) {
    bot.received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome {
                content,
                dimension,
                state,
                ..
            } => Some((content.clone(), *dimension, state.position)),
            _ => None,
        })
        .expect("no welcome")
}

#[test]
fn to_the_cloud_isles_and_back() {
    let dir = std::env::temp_dir().join(format!("ourcraft-sky-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (addr, stop, server) = start_server(&dir);

    let mut ada = join(addr, 91, "Ada");
    let (content, dim, feet) = welcome(&ada);
    assert_eq!(dim, Dimension::Overworld);
    let registry = BlockRegistry::from_defs(content.blocks.clone()).unwrap();
    let block = |name: &str| registry.by_name(name).unwrap().0;
    let item = |name: &str| content.items.iter().position(|i| i.name == name).unwrap() as u16;
    let (sunstone, portal) = (block("core:sunstone"), block("core:cloud_portal"));
    let f = [
        feet[0].floor() as i32,
        feet[1].floor() as i32,
        feet[2].floor() as i32,
    ];
    let at = |dx: i32, dy: i32, dz: i32| [f[0] + dx, f[1] + dy, f[2] + dz];

    // ---- Build a frame (4 wide, 5 tall) two blocks away, on clear ground.
    for dx in -1..=4 {
        for dy in 0..=5 {
            for dz in 1..=3 {
                ada.send(&ClientMessage::SetBlock {
                    pos: at(dx, dy, dz),
                    block: 0,
                    held: 0,
                });
            }
        }
    }
    for dx in 0..4 {
        for dy in 0..5 {
            if dx == 0 || dx == 3 || dy == 0 || dy == 4 {
                ada.send(&ClientMessage::SetBlock {
                    pos: at(dx, dy, 2),
                    block: sunstone,
                    held: 0,
                });
            }
        }
    }
    run_until(&mut [&mut ada], 1.0, |_| false);

    // ---- Light it with a feather (in survival, it's used up).
    ada.send(&ClientMessage::GameMode { creative: false });
    let feather = item("core:feather");
    ada.send(&ClientMessage::Inventory {
        slots: vec![Some(NetStack {
            item: feather,
            count: 2,
            wear: 0,
        })],
        selected: 0,
    });
    run_until(&mut [&mut ada], 0.3, |_| false);
    ada.received.clear();
    ada.send(&ClientMessage::UseBlock {
        pos: at(1, 0, 2),
        held: feather,
    });
    let lit = |b: &Bot| {
        b.received
            .iter()
            .filter(|m| matches!(m, ServerMessage::BlockChanged { block, .. } if *block == portal))
            .count()
    };
    assert!(
        run_until(&mut [&mut ada], 3.0, |b| lit(b[0]) >= 6),
        "the portal didn't light: {} portal blocks",
        lit(&ada)
    );
    assert!(ada.has(|m| matches!(m, ServerMessage::TakeItems(s) if s.item == feather)));

    // ---- A friend joins, in the overworld.
    let mut ben = join(addr, 92, "Ben");
    let (_, _, bf) = welcome(&ben);

    // ---- Step into the portal and wait.
    ada.send(&ClientMessage::GameMode { creative: true });
    run_until(&mut [&mut ada, &mut ben], 0.3, |_| false);
    ada.received.clear();
    ada.chat(&format!(
        "/tp {} {} {}",
        f[0] as f32 + 1.5,
        f[1] + 1,
        f[2] as f32 + 2.5
    ));
    assert!(
        run_until(&mut [&mut ada, &mut ben], 20.0, |b| b[0]
            .moved_to(Dimension::CloudIsles)
            .is_some()),
        "the portal didn't take Ada anywhere"
    );
    let there = ada.moved_to(Dimension::CloudIsles).unwrap();
    // At the Cloud Isles' own heights, close to where she left.
    assert!((0.0..256.0).contains(&there[1]), "arrived at {there:?}");
    assert!((there[0] - (f[0] as f32 + 1.5)).abs() < 16.0);

    // She's sent the Cloud Isles' land, with a portal back in it.
    ada.received
        .retain(|m| !matches!(m, ServerMessage::ChangedDimension { .. }));
    assert!(
        run_until(&mut [&mut ada, &mut ben], 15.0, |b| {
            b[0].received.iter().any(|m| match m {
                ServerMessage::Column { data, .. } => {
                    ourcraft_common::save::decode_column(data, None).is_ok_and(|c| {
                        c.iter()
                            .any(|ch| ch.palette().iter().any(|b| b.0 == portal))
                    })
                }
                _ => false,
            })
        }),
        "no portal on the other side"
    );

    // ---- Ada and Ben can't see each other now.
    ada.received.clear();
    ben.received.clear();
    run_until(&mut [&mut ada, &mut ben], 1.5, |_| false);
    let sees = |b: &Bot, id: u64| {
        b.has(|m| matches!(m, ServerMessage::PlayerStates { players, .. } if players.iter().any(|(p, _)| *p == id)))
    };
    assert!(!sees(&ben, 91), "Ben can see Ada in another dimension");
    assert!(!sees(&ada, 92), "Ada can see Ben in another dimension");
    // Nor any of the overworld's blocks changing.
    let stone = block("core:stone");
    ben.send(&ClientMessage::SetBlock {
        pos: [bf[0] as i32 + 1, bf[1] as i32, bf[2] as i32],
        block: stone,
        held: 0,
    });
    run_until(&mut [&mut ada, &mut ben], 0.5, |_| false);
    assert!(
        ben.has(|m| matches!(m, ServerMessage::BlockChanged { block: b, .. } if *b == stone)),
        "Ben's block wasn't placed"
    );
    assert!(
        !ada.has(|m| matches!(m, ServerMessage::BlockChanged { block: b, .. } if *b == stone)),
        "Ada heard about a block in the overworld"
    );
    drop(ben);

    // ---- The server stops and starts again: Ada is still up there.
    let sky_feet = there;
    drop(ada);
    run_until(&mut [], 0.5, |_| false);
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    assert!(
        dir.join("dimensions/cloud_isles/region")
            .read_dir()
            .unwrap()
            .count()
            > 0,
        "the Cloud Isles weren't saved"
    );
    let (addr, stop, server) = start_server(&dir);
    let mut ada = join(addr, 93, "Ada");
    let (_, dim, back) = welcome(&ada);
    assert_eq!(dim, Dimension::CloudIsles);
    assert!(
        (back[1] - sky_feet[1]).abs() < 2.0,
        "{back:?} vs {sky_feet:?}"
    );

    // ---- Back through the portal: out of it, and in again. She comes out
    // of the frame she built, not a new one.
    let portal_trip = |ada: &mut Bot, cell: [f32; 3], to: Dimension| {
        ada.received.clear();
        ada.chat(&format!("/tp {} {} {}", cell[0], cell[1], cell[2] + 2.0));
        run_until(&mut [&mut *ada], 0.5, |_| false);
        ada.chat(&format!("/tp {} {} {}", cell[0], cell[1], cell[2]));
        assert!(
            run_until(&mut [&mut *ada], 20.0, |b| b[0].moved_to(to).is_some()),
            "the portal didn't take Ada to {}",
            to.display_name()
        );
        ada.moved_to(to).unwrap()
    };
    let home = portal_trip(&mut ada, back, Dimension::Overworld);
    let frame = [f[0] as f32 + 1.5, f[1] as f32 + 1.0, f[2] as f32 + 2.5];
    let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|i| (a[i] - b[i]).abs() <= 1.5);
    assert!(near(home, frame), "came home at {home:?}, not at {frame:?}");
    let up = portal_trip(&mut ada, home, Dimension::CloudIsles);
    assert!(near(up, back), "went up to {up:?}, not to {back:?}");

    // ---- Falling off the islands: float gently home.
    let back = up;
    ada.walking = true;
    ada.chat(&format!("/tp {} 1 {}", back[0] + 40.0, back[2]));
    assert!(
        run_until(&mut [&mut ada], 30.0, |b| b[0]
            .moved_to(Dimension::Overworld)
            .is_some()),
        "Ada didn't come home after falling"
    );
    let home = ada.moved_to(Dimension::Overworld).unwrap();
    assert!(home[1] > 1.0 && home[1] < 256.0, "landed at {home:?}");
    assert!(ada.has(|m| matches!(m, ServerMessage::Chat { text, .. } if text.contains("floated"))));

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

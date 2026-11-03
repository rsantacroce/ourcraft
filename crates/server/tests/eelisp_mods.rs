//! Mods written in EELisp: a `.el` file in the mods folder is a mod. Runs
//! the Glow Garden example (`mods/eelisp_runner/examples/glow_garden.el`)
//! on a real server with two bots. Skipped unless the runner is built
//! (`mods/eelisp_runner/build.sh`).

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

fn titles(b: &Bot) -> Vec<String> {
    b.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Title { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn chats(b: &Bot) -> Vec<String> {
    b.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Chat { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn join(addr: SocketAddr, id: u64, name: &str) -> Bot {
    let mut bot = Bot::connect(addr, id);
    assert!(run_until(&mut [&mut bot], 5.0, |b| b[0]
        .client
        .is_connected()));
    bot.send(&ClientMessage::Hello {
        name: name.into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });
    let columns = |b: &Bot| {
        b.received
            .iter()
            .filter(|m| matches!(m, ServerMessage::Column { .. }))
            .count()
    };
    assert!(run_until(&mut [&mut bot], 20.0, |b| columns(b[0]) >= 25));
    bot
}

fn block_id(bot: &Bot, name: &str) -> Option<u16> {
    bot.received.iter().find_map(|m| match m {
        ServerMessage::Welcome { content, .. } => content
            .blocks
            .iter()
            .position(|b| b.name == name)
            .map(|i| i as u16),
        _ => None,
    })
}

/// Where blocks changed to `block`.
fn changed_to(bot: &Bot, block: u16) -> Vec<[i32; 3]> {
    bot.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::BlockChanged { pos, block: b } if *b == block => Some(*pos),
            _ => None,
        })
        .collect()
}

#[test]
fn an_eelisp_file_in_the_mods_folder_is_a_mod() {
    if !std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mods/runtimes/eelisp.wasm"
    ))
    .exists()
    {
        eprintln!("skipping: the EELisp runner isn't built");
        return;
    }
    let dir = std::env::temp_dir().join(format!("ourcraft-eelisp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mods = dir.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    std::fs::write(
        mods.join("glow_garden.el"),
        include_str!("../../../mods/eelisp_runner/examples/glow_garden.el"),
    )
    .unwrap();
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.join("world"),
        seed: Some(99),
        bind: Some(addr),
        max_view_distance: 3,
        creative: true,
        mods_dir: Some(mods),
        bundled_mods: false,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));

    // The script's blocks are in the game, and it says hello.
    let mut ada = join(addr, 501, "Ada");
    let glow = block_id(&ada, "garden:glow").expect("the script's block is registered");
    assert!(block_id(&ada, "garden:leafglass").is_some());
    assert!(
        run_until(&mut [&mut ada], 3.0, |b| chats(b[0])
            .iter()
            .any(|c| c.starts_with("Welcome to the Glow Garden, Ada!"))),
        "{:?}",
        chats(&ada)
    );

    // /garden builds a garden with a glow block in each corner.
    ada.send(&ClientMessage::Chat {
        text: "/garden".into(),
    });
    assert!(
        run_until(&mut [&mut ada], 5.0, |b| changed_to(b[0], glow).len() == 4
            && titles(b[0]).iter().any(|t| t == "A garden grows!")),
        "chats {:?}",
        chats(&ada)
    );
    // The command was the mod's, so nobody sees it in chat.
    assert!(
        !ada.has(|m| matches!(m, ServerMessage::Chat { from: Some(_), text } if text == "/garden"))
    );
    let corner = changed_to(&ada, glow)[0];

    // Bo isn't the gardener: the glow block stays.
    let mut bo = join(addr, 502, "Bo");
    bo.received.clear();
    bo.send(&ClientMessage::SetBlock {
        pos: corner,
        block: 0,
        held: 0,
    });
    assert!(
        run_until(&mut [&mut ada, &mut bo], 3.0, |b| chats(b[1])
            .iter()
            .any(|c| c == "Only the gardener can pick glow blocks!")),
        "{:?}",
        chats(&bo)
    );
    run_until(&mut [&mut ada, &mut bo], 0.5, |_| false);
    assert!(changed_to(&bo, 0).is_empty(), "the glow block was broken");

    // Ada is: it breaks.
    ada.send(&ClientMessage::SetBlock {
        pos: corner,
        block: 0,
        held: 0,
    });
    assert!(run_until(&mut [&mut ada, &mut bo], 3.0, |b| changed_to(
        b[1], 0
    )
    .contains(&corner)));

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

//! Boats left in the world are still there after the server restarts.

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

fn start(dir: &std::path::Path) -> (SocketAddr, StopFlag, std::thread::JoinHandle<()>) {
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.to_path_buf(),
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
    (addr, stop, server)
}

fn join(addr: SocketAddr, id: u64) -> Bot {
    let mut bot = Bot::connect(addr, id);
    assert!(run_until(&mut [&mut bot], 5.0, |b| b[0]
        .client
        .is_connected()));
    bot.send(&ClientMessage::Hello {
        name: "Ash".into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    });
    assert!(run_until(&mut [&mut bot], 20.0, |b| b[0]
        .has(|m| matches!(m, ServerMessage::Welcome { .. }))));
    bot
}

#[test]
fn boats_are_saved() {
    let dir = std::env::temp_dir().join(format!("ourcraft-vehicles-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (addr, stop, server) = start(&dir);
    let mut ash = join(addr, 71);
    run_until(&mut [&mut ash], 2.0, |_| false);
    let (feet, content) = ash
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { state, content, .. } => {
                Some((state.position, content.clone()))
            }
            _ => None,
        })
        .unwrap();
    let item = |name: &str| content.items.iter().position(|i| i.name == name).unwrap() as u16;
    let ground = [
        feet[0].floor() as i32 + 2,
        feet[1].floor() as i32 - 1,
        feet[2].floor() as i32,
    ];
    ash.send(&ClientMessage::UseBlock {
        pos: ground,
        held: item("core:birch_boat"),
    });
    let boat = |b: &Bot| {
        b.has(|m| matches!(m, ServerMessage::EntitySpawned { model, .. } if model == "@boat:birch"))
    };
    assert!(run_until(&mut [&mut ash], 2.0, |b| boat(b[0])), "no boat");
    // Long enough for the boat to be noted down for saving.
    run_until(&mut [&mut ash], 1.0, |_| false);
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    assert!(dir.join("vehicles.dat").exists(), "nothing saved");

    let (addr, stop, server) = start(&dir);
    let mut ash = join(addr, 72);
    assert!(
        run_until(&mut [&mut ash], 5.0, |b| boat(b[0])),
        "the boat didn't come back"
    );
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

//! Teleporting far away and back: the columns at home are sent again.

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
fn teleporting_back_resends_home() {
    let dir = std::env::temp_dir().join(format!("ourcraft-tp-{}", std::process::id()));
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
        max_view_distance: 8,
        creative: true,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));
    let mut bot = Bot::connect(addr, 77);
    assert!(run_until(&mut [&mut bot], 5.0, |b| b[0]
        .client
        .is_connected()));
    bot.send(&ClientMessage::Hello {
        name: "Zoe".into(),
        view_distance: 2,
        protocol_version: PROTOCOL_VERSION,
    });
    let cols = |b: &Bot, near: (i32, i32)| {
        b.received
            .iter()
            .filter(|m| {
                matches!(m, ServerMessage::Column { x, z, .. }
                if (x - near.0).abs() <= 1 && (z - near.1).abs() <= 1)
            })
            .count()
    };
    assert!(run_until(&mut [&mut bot], 20.0, |b| cols(b[0], (0, 0)) >= 9));
    let feet = bot
        .received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { state, .. } => Some(state.position),
            _ => None,
        })
        .unwrap();
    let home = (
        (feet[0] / 16.0).floor() as i32,
        (feet[2] / 16.0).floor() as i32,
    );
    let tp = |bot: &mut Bot, x: f32| {
        bot.send(&ClientMessage::Chat {
            text: format!("/tp {x} 90 {}", feet[2]),
        })
    };
    tp(&mut bot, feet[0] + 300.0);
    let far = (home.0 + 18, home.1);
    assert!(
        run_until(&mut [&mut bot], 20.0, |b| cols(b[0], far) >= 9),
        "nothing far away"
    );
    run_until(&mut [&mut bot], 1.0, |_| false);
    bot.received.clear();
    tp(&mut bot, feet[0]);
    assert!(
        run_until(&mut [&mut bot], 20.0, |b| cols(b[0], home) >= 9),
        "home wasn't sent again: {} columns",
        cols(&bot, home)
    );
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

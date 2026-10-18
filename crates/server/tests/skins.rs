//! Skins: a player's skin is passed on to everyone else, newcomers are
//! told the skins of players already there, and broken skins are dropped.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::protocol::{
    self, Channel, ClientMessage, PROTOCOL_ID, PROTOCOL_VERSION, ServerMessage,
};
use ourcraft_common::skin::{NetSkin, SKIN_BYTES};
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

/// The skins a bot has been told about, by player.
fn skins_of(b: &Bot, player: u64) -> Vec<NetSkin> {
    b.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::Skin { player: p, skin } if *p == player => Some(skin.clone()),
            _ => None,
        })
        .collect()
}

fn welcomed(b: &Bot) -> bool {
    b.received
        .iter()
        .any(|m| matches!(m, ServerMessage::Welcome { .. }))
}

#[test]
fn skins_are_passed_on() {
    let dir = std::env::temp_dir().join(format!("ourcraft-skins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(16),
        bind: Some(addr),
        max_view_distance: 2,
        creative: true,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));
    let join = |id: u64, name: &str| {
        let mut bot = Bot::connect(addr, id);
        assert!(run_until(&mut [&mut bot], 5.0, |b| b[0]
            .client
            .is_connected()));
        bot.send(&ClientMessage::Hello {
            name: name.into(),
            view_distance: 2,
            protocol_version: PROTOCOL_VERSION,
        });
        assert!(run_until(&mut [&mut bot], 10.0, |b| welcomed(b[0])));
        bot
    };
    let mut ana = join(201, "Ana");
    let mut ben = join(202, "Ben");

    // Ana wears her own picture; Ben sees it, Ana isn't sent it back.
    let picture: Vec<u8> = (0..SKIN_BYTES).map(|i| (i % 251) as u8).collect();
    ana.send(&ClientMessage::Skin(NetSkin::Custom(picture.clone())));
    assert!(
        run_until(&mut [&mut ana, &mut ben], 10.0, |b| !skins_of(b[1], 201)
            .is_empty()),
        "Ben never saw Ana's skin"
    );
    assert_eq!(skins_of(&ben, 201), [NetSkin::Custom(picture.clone())]);
    assert!(skins_of(&ana, 201).is_empty());

    // A broken skin (too few pixels) and a silly name go nowhere.
    run_until(&mut [&mut ana, &mut ben], 0.7, |_| false);
    ben.send(&ClientMessage::Skin(NetSkin::Custom(vec![1; 100])));
    run_until(&mut [&mut ana, &mut ben], 0.7, |_| false);
    ben.send(&ClientMessage::Skin(NetSkin::Builtin("Not A Skin!".into())));
    run_until(&mut [&mut ana, &mut ben], 1.0, |_| false);
    assert!(skins_of(&ana, 202).is_empty());

    // Cal joins later and is told how Ana looks.
    let mut cal = join(203, "Cal");
    assert!(
        run_until(&mut [&mut ana, &mut ben, &mut cal], 10.0, |b| !skins_of(
            b[2], 201
        )
        .is_empty()),
        "Cal never saw Ana's skin"
    );
    assert_eq!(skins_of(&cal, 201), [NetSkin::Custom(picture)]);

    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

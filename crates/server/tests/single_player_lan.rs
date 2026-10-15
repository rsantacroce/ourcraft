//! Single player with "friends on my network can join" switched on: the
//! in-memory client and UDP clients must both work at the same time.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::protocol::{
    self, Channel, ClientMessage, PROTOCOL_ID, PROTOCOL_VERSION, ServerMessage,
};
use ourcraft_server::net::connection_config;
use ourcraft_server::{EmbeddedServer, ServerConfig};
use renet::RenetClient;
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};

fn hello(name: &str) -> Vec<u8> {
    protocol::encode(&ClientMessage::Hello {
        name: name.into(),
        view_distance: 2,
        protocol_version: PROTOCOL_VERSION,
    })
}

fn welcomed(client: &mut RenetClient) -> bool {
    while let Some(b) = client.receive_message(Channel::Reliable) {
        if matches!(protocol::decode(&b), Some(ServerMessage::Welcome { .. })) {
            return true;
        }
    }
    false
}

#[test]
fn local_and_network_players_both_join() {
    let dir = std::env::temp_dir().join(format!("ourcraft-splan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let (mut server, link) = EmbeddedServer::start(ServerConfig {
        world_dir: dir.clone(),
        seed: Some(1),
        bind: Some(addr),
        bundled_mods: false,
        ..Default::default()
    });

    // The single player, over the in-memory link.
    let mut local = RenetClient::new(connection_config());
    local.set_connected();
    local.send_message(Channel::Reliable, hello("Host"));

    // A friend, over UDP.
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let auth = ClientAuthentication::Unsecure {
        protocol_id: PROTOCOL_ID,
        client_id: 77,
        server_addr: addr,
        user_data: None,
    };
    let mut friend = RenetClient::new(connection_config());
    let mut transport =
        NetcodeClientTransport::new(now, auth, UdpSocket::bind("127.0.0.1:0").unwrap()).unwrap();
    let mut friend_hello_sent = false;

    let (mut local_ok, mut friend_ok) = (false, false);
    let start = Instant::now();
    let dt = Duration::from_millis(10);
    while start.elapsed() < Duration::from_secs(10) && !(local_ok && friend_ok) {
        local.update(dt);
        for p in local.get_packets_to_send() {
            link.to_server.send(p).unwrap();
        }
        while let Ok(p) = link.from_server.lock().unwrap().try_recv() {
            local.process_packet(&p);
        }
        local_ok |= welcomed(&mut local);

        friend.update(dt);
        let _ = transport.update(dt, &mut friend);
        if friend.is_connected() && !friend_hello_sent {
            friend.send_message(Channel::Reliable, hello("Friend"));
            friend_hello_sent = true;
        }
        friend_ok |= welcomed(&mut friend);
        let _ = transport.send_packets(&mut friend);
        std::thread::sleep(dt);
    }
    assert!(local_ok, "the single player was never welcomed");
    assert!(friend_ok, "the network friend was never welcomed");
    server.stop();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = Ordering::Relaxed;
}

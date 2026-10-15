//! Moving bytes: UDP and in-memory transports, connection events, and
//! turning raw messages into typed [`FromClient`] messages for the rest of
//! the server.

use std::collections::HashSet;
use std::net::UdpSocket;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use ourcraft_common::protocol::{self, Channel, ClientMessage, PROTOCOL_ID, ServerMessage};
use renet::{ChannelConfig, ClientId, ConnectionConfig, RenetServer, SendType, ServerEvent};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig as NetcodeConfig};

use crate::{LocalServerLink, ServerConfig};

/// Client id used for the in-memory (single player) connection. Network
/// clients pick random ids, so a clash is astronomically unlikely.
pub const LOCAL_CLIENT_ID: ClientId = 1;

/// Channel setup, shared by client and server (they must match exactly).
pub fn connection_config() -> ConnectionConfig {
    let channels = |reliable_mem| {
        vec![
            ChannelConfig {
                channel_id: Channel::Reliable.into(),
                // Reliable channels disconnect if they overflow, so leave
                // plenty of room for world data.
                max_memory_usage_bytes: reliable_mem,
                send_type: SendType::ReliableOrdered {
                    resend_time: Duration::from_millis(200),
                },
            },
            ChannelConfig {
                channel_id: Channel::Unreliable.into(),
                max_memory_usage_bytes: 2 * 1024 * 1024,
                send_type: SendType::Unreliable,
            },
        ]
    };
    ConnectionConfig {
        available_bytes_per_tick: 120_000,
        server_channels_config: channels(64 * 1024 * 1024),
        client_channels_config: channels(4 * 1024 * 1024),
    }
}

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FromClient>()
            .add_message::<ClientConnected>()
            .add_message::<ClientDisconnected>()
            .add_systems(Startup, setup_network)
            .add_systems(PreUpdate, receive.in_set(ReceiveSet))
            .add_systems(PostUpdate, send.in_set(SendSet));
    }
}

/// Systems that bring in network messages (PreUpdate).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReceiveSet;

/// Systems that flush outgoing packets (PostUpdate).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SendSet;

/// A decoded message from a client.
#[derive(Message, Debug, Clone)]
pub struct FromClient {
    pub client: ClientId,
    pub msg: ClientMessage,
}

#[derive(Message, Debug, Clone, Copy)]
pub struct ClientConnected(pub ClientId);

#[derive(Message, Debug, Clone)]
pub struct ClientDisconnected {
    pub client: ClientId,
    pub reason: String,
}

/// The in-memory link handed over by `build_app`, taken at startup.
#[derive(Resource)]
pub struct PendingLocalLink(pub Mutex<Option<LocalServerLink>>);

/// The server's connections.
#[derive(Resource)]
pub struct Network {
    pub server: RenetServer,
    netcode: Option<NetcodeServerTransport>,
    local: Option<LocalServerLink>,
    /// Clients that sent garbage; disconnected at the end of the tick.
    misbehaving: HashSet<ClientId>,
}

impl Network {
    pub fn send(&mut self, client: ClientId, channel: Channel, msg: &ServerMessage) {
        if self.server.is_connected(client) {
            self.server
                .send_message(client, channel, protocol::encode(msg));
        }
    }

    pub fn broadcast(&mut self, channel: Channel, msg: &ServerMessage) {
        self.server
            .broadcast_message(channel, protocol::encode(msg));
    }

    pub fn broadcast_except(&mut self, except: ClientId, channel: Channel, msg: &ServerMessage) {
        self.server
            .broadcast_message_except(except, channel, protocol::encode(msg));
    }

    /// Whether `bytes` more can be queued for a client without overflowing.
    pub fn has_room(&self, client: ClientId, bytes: usize) -> bool {
        self.server
            .can_send_message(client, Channel::Reliable, bytes)
    }

    pub fn kick(&mut self, client: ClientId, reason: &str) {
        self.send(
            client,
            Channel::Reliable,
            &ServerMessage::Kick(reason.into()),
        );
        // Disconnect after the kick message has had a chance to go out.
        self.misbehaving.insert(client);
    }

    /// Round-trip time to a client, in seconds.
    pub fn rtt(&self, client: ClientId) -> f64 {
        self.server.rtt(client)
    }

    pub fn is_listening(&self) -> bool {
        self.netcode.is_some()
    }
}

fn now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

fn setup_network(
    mut commands: Commands,
    config: Res<ServerConfig>,
    pending: Res<PendingLocalLink>,
) {
    let mut server = RenetServer::new(connection_config());
    let local = pending.0.lock().unwrap().take();
    if local.is_some() {
        // The in-memory client is connected from the start.
        server.add_connection(LOCAL_CLIENT_ID);
    }
    let netcode = config.bind.and_then(|addr| {
        let socket = match UdpSocket::bind(addr) {
            Ok(s) => s,
            Err(e) => {
                error!("Can't listen on {addr}: {e}");
                return None;
            }
        };
        let netcode_config = NetcodeConfig {
            current_time: now(),
            max_clients: config.max_players,
            protocol_id: PROTOCOL_ID,
            public_addresses: vec![socket.local_addr().unwrap_or(addr)],
            // No login system: anyone who knows the address can join. Fine
            // for a LAN or a server you share with friends.
            authentication: ServerAuthentication::Unsecure,
        };
        match NetcodeServerTransport::new(netcode_config, socket) {
            Ok(t) => {
                info!("Listening for players on UDP {addr}");
                Some(t)
            }
            Err(e) => {
                error!("Can't start network transport: {e}");
                None
            }
        }
    });
    commands.insert_resource(Network {
        server,
        netcode,
        local,
        misbehaving: HashSet::new(),
    });
}

fn receive(
    time: Res<Time<Real>>,
    mut net: ResMut<Network>,
    mut connected: MessageWriter<ClientConnected>,
    mut disconnected: MessageWriter<ClientDisconnected>,
    mut messages: MessageWriter<FromClient>,
) {
    let net = &mut *net;
    let dt = time.delta();
    net.server.update(dt);
    if let Some(transport) = net.netcode.as_mut()
        && let Err(e) = transport.update(dt, &mut net.server)
    {
        warn!("Network error: {e}");
    }
    if let Some(local) = &net.local {
        let from_client = local.from_client.lock().unwrap();
        while let Ok(packet) = from_client.try_recv() {
            let _ = net.server.process_packet_from(&packet, LOCAL_CLIENT_ID);
        }
    }

    while let Some(event) = net.server.get_event() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                connected.write(ClientConnected(client_id));
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                net.misbehaving.remove(&client_id);
                disconnected.write(ClientDisconnected {
                    client: client_id,
                    reason: reason.to_string(),
                });
            }
        }
    }

    for client in net.server.clients_id() {
        for channel in [Channel::Reliable, Channel::Unreliable] {
            while let Some(bytes) = net.server.receive_message(client, channel) {
                match protocol::decode::<ClientMessage>(&bytes) {
                    Some(msg) => {
                        messages.write(FromClient { client, msg });
                    }
                    None => {
                        warn!("Client {client} sent an unreadable message; disconnecting");
                        net.misbehaving.insert(client);
                    }
                }
            }
        }
    }
}

fn send(mut net: ResMut<Network>) {
    let net = &mut *net;
    // Disconnect clients flagged this tick (after their last messages).
    for client in std::mem::take(&mut net.misbehaving) {
        net.server.disconnect(client);
    }
    if let Some(transport) = net.netcode.as_mut() {
        transport.send_packets(&mut net.server);
    }
    if let Some(local) = &net.local
        && let Ok(packets) = net.server.get_packets_to_send(LOCAL_CLIENT_ID)
    {
        for p in packets {
            // The client may have gone away; nothing to do then.
            let _ = local.to_client.send(p);
        }
    }
}

/// Disconnect everyone politely (used when shutting down).
pub fn disconnect_all(net: &mut Network) {
    net.broadcast(
        Channel::Reliable,
        &ServerMessage::Kick("The server is shutting down".into()),
    );
    if let Some(t) = net.netcode.as_mut() {
        t.send_packets(&mut net.server);
        t.disconnect_all(&mut net.server);
    }
}

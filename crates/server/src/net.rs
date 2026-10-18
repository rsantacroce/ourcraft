//! Moving bytes: UDP and in-memory transports, connection events, and
//! turning raw messages into typed [`FromClient`] messages for the rest of
//! the server.

use std::collections::{HashMap, HashSet};
use std::net::UdpSocket;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use ourcraft_common::dimension::Dimension;
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
            .add_systems(PostUpdate, send.in_set(SendSet))
            .add_systems(Update, announce_on_lan);
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
    /// Which dimension each client is in (they only hear about that one;
    /// see `dimensions::localize`). Missing means the overworld.
    dims: HashMap<ClientId, Dimension>,
    /// When each client last changed dimension (seconds), and the clock.
    moved_at: HashMap<ClientId, f64>,
    clock: f64,
}

/// For this long after changing dimension, what a client says about places
/// is ignored: it was probably sent before it heard about the move.
const STALE_AFTER_MOVE_SECS: f64 = 1.0;

impl Network {
    pub fn send(&mut self, client: ClientId, channel: Channel, msg: &ServerMessage) {
        if !self.server.is_connected(client) {
            return;
        }
        if let Some(msg) = crate::dimensions::localize(msg, self.dimension(client)) {
            self.server
                .send_message(client, channel, protocol::encode(&*msg));
        }
    }

    /// Send to everyone (as each of them should hear it).
    pub fn broadcast(&mut self, channel: Channel, msg: &ServerMessage) {
        self.send_to_all(None, channel, msg);
    }

    pub fn broadcast_except(&mut self, except: ClientId, channel: Channel, msg: &ServerMessage) {
        self.send_to_all(Some(except), channel, msg);
    }

    fn send_to_all(&mut self, except: Option<ClientId>, channel: Channel, msg: &ServerMessage) {
        // Encoded once per dimension, at most.
        let mut encoded: HashMap<Dimension, Option<Vec<u8>>> = HashMap::new();
        for client in self.server.clients_id() {
            if Some(client) == except {
                continue;
            }
            let dim = self.dimension(client);
            let bytes = encoded.entry(dim).or_insert_with(|| {
                crate::dimensions::localize(msg, dim).map(|m| protocol::encode(&*m))
            });
            if let Some(bytes) = bytes {
                self.server.send_message(client, channel, bytes.clone());
            }
        }
    }

    /// The dimension a client is in.
    pub fn dimension(&self, client: ClientId) -> Dimension {
        self.dims.get(&client).copied().unwrap_or_default()
    }

    /// From now on, `client` hears about `dim` (and its messages about
    /// places are about there), having just moved there.
    pub fn set_dimension(&mut self, client: ClientId, dim: Dimension) {
        self.dims.insert(client, dim);
        self.moved_at.insert(client, self.clock);
    }

    /// `client` joins the game in `dim`.
    pub fn join_dimension(&mut self, client: ClientId, dim: Dimension) {
        self.dims.insert(client, dim);
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
        dims: HashMap::new(),
        moved_at: HashMap::new(),
        clock: 0.0,
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
    net.clock = time.elapsed_secs_f64();
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
                net.dims.remove(&client_id);
                net.moved_at.remove(&client_id);
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
                        let dim = net.dimension(client);
                        let just_moved = net
                            .moved_at
                            .get(&client)
                            .is_some_and(|t| net.clock - t < STALE_AFTER_MOVE_SECS);
                        if just_moved && crate::dimensions::has_place(&msg) {
                            continue;
                        }
                        let msg = crate::dimensions::globalize(msg, dim);
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
    // The in-memory (single player) client first: the UDP transport sends
    // to *every* connection it finds, and would otherwise swallow the local
    // player's packets (it can't reach that client over UDP).
    if let Some(local) = &net.local
        && let Ok(packets) = net.server.get_packets_to_send(LOCAL_CLIENT_ID)
    {
        for p in packets {
            // The client may have gone away; nothing to do then.
            let _ = local.to_client.send(p);
        }
    }
    if let Some(transport) = net.netcode.as_mut() {
        transport.send_packets(&mut net.server);
    }
}

/// Tell computers on the local network that this server exists (once a
/// second, as a UDP broadcast), so their Join screens can list it.
fn announce_on_lan(
    time: Res<Time<Real>>,
    config: Res<ServerConfig>,
    net: Res<Network>,
    open: Option<Res<crate::world::OpenWorld>>,
    mut socket: Local<Option<UdpSocket>>,
    mut timer: Local<f32>,
) {
    let Some(bind) = config.bind else { return };
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 1.0;
    if socket.is_none() {
        *socket = UdpSocket::bind("0.0.0.0:0")
            .ok()
            .filter(|s| s.set_broadcast(true).is_ok());
    }
    let Some(s) = socket.as_ref() else { return };
    let beacon = ourcraft_common::protocol::LanBeacon {
        protocol: PROTOCOL_ID,
        name: config.server_name.clone(),
        motd: config.motd.clone(),
        port: bind.port(),
        players: net.server.connected_clients() as u16,
        max_players: config.max_players as u16,
        game_mode: open.and_then(|o| o.meta.game_mode.clone()),
    };
    let bytes = protocol::encode(&beacon);
    let target = (
        std::net::Ipv4Addr::BROADCAST,
        ourcraft_common::protocol::LAN_DISCOVERY_PORT,
    );
    // Failing to broadcast (no network, not allowed) just means no listing.
    let _ = s.send_to(&bytes, target);
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

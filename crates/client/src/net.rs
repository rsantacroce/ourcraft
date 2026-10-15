//! The client's connection to a server.
//!
//! Single player starts an [`EmbeddedServer`] on a background thread and
//! talks to it over an in-memory link; multiplayer connects over UDP. Either
//! way the rest of the client sees the same thing: [`FromServer`] messages
//! arriving, and [`Connection::send`] for replies.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::window::AppLifecycle;
use ourcraft_common::protocol::{
    self, Channel, ClientMessage, DEFAULT_PORT, PROTOCOL_ID, PROTOCOL_VERSION, PlayerId,
    ServerMessage, sanitize_name,
};
use ourcraft_server::net::connection_config;
use ourcraft_server::{EmbeddedServer, LocalLink, ServerConfig};
use renet::RenetClient;
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};

use crate::paths::{folder_name, saves_dir};
use crate::settings::GraphicsSettings;

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FromServer>()
            .add_systems(Startup, connect_from_launch_options)
            .add_systems(
                PreUpdate,
                receive
                    .in_set(NetReceiveSet)
                    .run_if(resource_exists::<Connection>),
            )
            .add_systems(
                Update,
                (send_view_distance, save_on_suspend).run_if(resource_exists::<Connection>),
            )
            .add_systems(
                PostUpdate,
                send.in_set(NetSendSet)
                    .run_if(resource_exists::<Connection>),
            );
    }
}

/// Where incoming server messages are turned into [`FromServer`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetReceiveSet;

/// Where outgoing packets are flushed.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetSendSet;

/// A message from the server, for whichever system cares about it.
#[derive(Message, Debug, Clone)]
pub struct FromServer(pub ServerMessage);

/// What to do on startup (from the command line until the menu in M10):
/// - `ourcraft` → single player, world "My World"
/// - `ourcraft --world NAME [--seed N] [--lan]` → single player, that world
///   (`--lan` also lets others on your network join)
/// - `ourcraft --connect HOST[:PORT] [--name NAME]` → join a server
#[derive(Resource, Debug, Clone)]
pub enum LaunchOptions {
    SinglePlayer {
        world: String,
        seed: Option<u64>,
        lan: bool,
        name: String,
    },
    Join {
        address: String,
        name: String,
    },
    /// Don't connect yet (the menu will).
    None,
}

impl LaunchOptions {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let name = sanitize_name(&value("--name").unwrap_or_else(|| "Player".into()));
        if let Some(address) = value("--connect") {
            return Self::Join { address, name };
        }
        Self::SinglePlayer {
            world: value("--world").unwrap_or_else(|| "My World".into()),
            seed: value("--seed").and_then(|s| s.parse().ok()),
            lan: args.iter().any(|a| a == "--lan"),
            name,
        }
    }
}

enum Transport {
    Udp(Box<NetcodeClientTransport>),
    Local(LocalLink),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Connecting,
    Connected,
    Disconnected(String),
}

/// The connection to the server. Dropping it disconnects (and stops the
/// embedded server, which saves the world).
#[derive(Resource)]
pub struct Connection {
    client: RenetClient,
    transport: Transport,
    // Declared after `transport` so the link closes before the server stops.
    embedded: Option<EmbeddedServer>,
    pub name: String,
    /// Our player id, once welcomed.
    pub my_id: Option<PlayerId>,
    pub status: Status,
    /// Shown while connecting.
    pub target: String,
    hello_sent: bool,
}

impl Connection {
    /// Start single player: run a server in the background and connect to it.
    pub fn single_player(world: &str, seed: Option<u64>, lan: bool, name: &str) -> Self {
        let config = ServerConfig {
            world_dir: saves_dir().join(folder_name(world)),
            world_name: world.to_owned(),
            seed,
            server_name: format!("{name}'s world"),
            bind: lan.then(|| SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT))),
            ..default()
        };
        info!("Starting single player world '{world}'");
        let (server, link) = EmbeddedServer::start(config);
        let mut client = RenetClient::new(connection_config());
        // In-memory links are connected from the start.
        client.set_connected();
        Self {
            client,
            transport: Transport::Local(link),
            embedded: Some(server),
            name: name.to_owned(),
            my_id: None,
            status: Status::Connecting,
            target: world.to_owned(),
            hello_sent: false,
        }
    }

    /// Connect to a server over the network. `address` is `host` or
    /// `host:port`.
    pub fn join(address: &str, name: &str) -> Result<Self, String> {
        let with_port = if address.contains(':') {
            address.to_owned()
        } else {
            format!("{address}:{DEFAULT_PORT}")
        };
        let server_addr = with_port
            .to_socket_addrs()
            .map_err(|e| format!("Can't find {address}: {e}"))?
            .find(|a| a.is_ipv4())
            .ok_or_else(|| format!("Can't find {address}"))?;
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        // A random id; the server tells players apart by it.
        let client_id = ourcraft_common::worldgen::splitmix64(now.as_nanos() as u64) | 2;
        let auth = ClientAuthentication::Unsecure {
            protocol_id: PROTOCOL_ID,
            client_id,
            server_addr,
            user_data: None,
        };
        let transport =
            NetcodeClientTransport::new(now, auth, socket).map_err(|e| e.to_string())?;
        info!("Connecting to {server_addr}");
        Ok(Self {
            client: RenetClient::new(connection_config()),
            transport: Transport::Udp(Box::new(transport)),
            embedded: None,
            name: name.to_owned(),
            my_id: None,
            status: Status::Connecting,
            target: address.to_owned(),
            hello_sent: false,
        })
    }

    pub fn send(&mut self, channel: Channel, msg: &ClientMessage) {
        if self.client.is_connected() {
            self.client.send_message(channel, protocol::encode(msg));
        }
    }

    pub fn is_single_player(&self) -> bool {
        self.embedded.is_some()
    }

    /// Round-trip time in milliseconds.
    pub fn rtt_ms(&self) -> f64 {
        self.client.rtt() * 1000.0
    }

    pub fn is_welcomed(&self) -> bool {
        self.my_id.is_some()
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if let Transport::Udp(t) = &mut self.transport {
            // Tell the server we're leaving rather than just vanishing.
            t.disconnect();
        }
        if let Some(server) = &mut self.embedded {
            server.stop();
        }
    }
}

fn connect_from_launch_options(mut commands: Commands, options: Option<Res<LaunchOptions>>) {
    let options = options
        .map(|o| o.clone())
        .unwrap_or_else(LaunchOptions::from_args);
    match options {
        LaunchOptions::SinglePlayer {
            world,
            seed,
            lan,
            name,
        } => {
            commands.insert_resource(Connection::single_player(&world, seed, lan, &name));
        }
        LaunchOptions::Join { address, name } => match Connection::join(&address, &name) {
            Ok(c) => commands.insert_resource(c),
            Err(e) => error!("{e}"),
        },
        LaunchOptions::None => {}
    }
}

fn receive(
    time: Res<Time<Real>>,
    settings: Res<GraphicsSettings>,
    mut conn: ResMut<Connection>,
    mut messages: MessageWriter<FromServer>,
) {
    let conn = &mut *conn;
    let dt = time.delta();
    conn.client.update(dt);
    match &mut conn.transport {
        Transport::Udp(t) => {
            if let Err(e) = t.update(dt, &mut conn.client) {
                conn.status = Status::Disconnected(e.to_string());
            }
        }
        Transport::Local(link) => {
            let rx = link.from_server.lock().unwrap();
            while let Ok(packet) = rx.try_recv() {
                conn.client.process_packet(&packet);
            }
        }
    }

    if conn.client.is_connected() && !conn.hello_sent {
        conn.hello_sent = true;
        let hello = ClientMessage::Hello {
            name: conn.name.clone(),
            view_distance: settings.render_distance as u8,
            protocol_version: PROTOCOL_VERSION,
        };
        conn.send(Channel::Reliable, &hello);
    }

    for channel in [Channel::Reliable, Channel::Unreliable] {
        while let Some(bytes) = conn.client.receive_message(channel) {
            let Some(msg) = protocol::decode::<ServerMessage>(&bytes) else {
                warn!("Unreadable message from the server");
                continue;
            };
            match &msg {
                ServerMessage::Welcome {
                    your_id,
                    server_name,
                    motd,
                    ..
                } => {
                    info!("Joined {server_name}: {motd}");
                    conn.my_id = Some(*your_id);
                    conn.status = Status::Connected;
                }
                ServerMessage::Kick(reason) => {
                    conn.status = Status::Disconnected(reason.clone());
                }
                _ => {}
            }
            messages.write(FromServer(msg));
        }
    }

    if conn.client.is_disconnected() && !matches!(conn.status, Status::Disconnected(_)) {
        let reason = conn
            .client
            .disconnect_reason()
            .map_or_else(|| "Connection lost".to_owned(), |r| r.to_string());
        warn!("Disconnected: {reason}");
        conn.status = Status::Disconnected(reason);
    }
}

fn send(mut conn: ResMut<Connection>) {
    let conn = &mut *conn;
    match &mut conn.transport {
        Transport::Udp(t) => {
            if let Err(e) = t.send_packets(&mut conn.client) {
                warn!("Network send failed: {e}");
            }
        }
        Transport::Local(link) => {
            for p in conn.client.get_packets_to_send() {
                let _ = link.to_server.send(p);
            }
        }
    }
}

fn send_view_distance(settings: Res<GraphicsSettings>, mut conn: ResMut<Connection>) {
    if settings.is_changed() && !settings.is_added() {
        conn.send(
            Channel::Reliable,
            &ClientMessage::ViewDistance(settings.render_distance as u8),
        );
    }
}

/// Phones and tablets can kill a backgrounded app without warning, so save
/// the moment we're told we're going to the background.
fn save_on_suspend(mut lifecycle: MessageReader<AppLifecycle>, conn: Res<Connection>) {
    if lifecycle
        .read()
        .any(|e| matches!(e, AppLifecycle::WillSuspend))
        && let Some(server) = &conn.embedded
    {
        server.request_save();
    }
}

/// How long to keep showing remote players' old positions behind real time,
/// so there's always a newer position to blend towards (see
/// `remote_players`).
pub const INTERPOLATION_DELAY: Duration = Duration::from_millis(100);

//! The client's connection to a server.
//!
//! Single player starts an [`EmbeddedServer`] on a background thread and
//! talks to it over an in-memory link; multiplayer connects over UDP. Either
//! way the rest of the client sees the same thing: [`FromServer`] messages
//! arriving, and [`Connection::send`] for replies.

use std::collections::VecDeque;
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
            .add_message::<SessionEnded>()
            .add_systems(Startup, connect_from_launch_options)
            .add_systems(
                PreUpdate,
                receive
                    .in_set(NetReceiveSet)
                    .run_if(resource_exists::<Connection>),
            )
            .add_systems(
                Update,
                (send_view_distance, save_on_suspend, cycle_lag_presets)
                    .run_if(resource_exists::<Connection>),
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

/// A single-player world to start (or create).
#[derive(Debug, Clone)]
pub struct SinglePlayer {
    /// Folder name under the saves folder.
    pub folder: String,
    pub world_name: String,
    /// For a new world (random if `None`).
    pub seed: Option<u64>,
    /// Let others on the local network join.
    pub lan: bool,
    /// Game mode for a new world.
    pub mode: Option<String>,
    /// Creative (fly, endless blocks) or survival, for a new world.
    pub creative: bool,
}

/// Sent when we leave a game (quit to the title screen), so every part of
/// the client can clear what it knew about that world.
#[derive(Message, Debug, Clone, Copy)]
pub struct SessionEnded;

/// What to do on startup (from the command line until the menu in M10):
/// - `ourcraft` → single player, world "My World"
/// - `ourcraft --world NAME [--seed N] [--lan] [--mode team_shooter]` →
///   single player, that world (`--lan` also lets others on your network
///   join; `--mode` picks a game mode for a new world)
/// - `ourcraft --connect HOST[:PORT] [--name NAME]` → join a server
#[derive(Resource, Debug, Clone)]
pub enum LaunchOptions {
    SinglePlayer {
        world: String,
        seed: Option<u64>,
        lan: bool,
        name: String,
        /// Game mode for a new world (e.g. "team_shooter").
        mode: Option<String>,
        creative: bool,
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
        // No world chosen on the command line: show the main menu.
        if value("--world").is_none() {
            return Self::None;
        }
        Self::SinglePlayer {
            world: value("--world").unwrap_or_else(|| "My World".into()),
            seed: value("--seed").and_then(|s| s.parse().ok()),
            lan: args.iter().any(|a| a == "--lan"),
            name,
            mode: value("--mode"),
            creative: !args.iter().any(|a| a == "--survival"),
        }
    }
}

enum Transport {
    Udp(Box<NetcodeClientTransport>),
    Local(LocalLink),
}

/// Pretend the network is worse than it is, to test prediction and
/// smoothing at home. Applies to everything we send and receive.
///
/// Set with `--lag 150 --jitter 20 --loss 5` or cycle presets with F7.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LagSettings {
    /// Extra round-trip time in milliseconds (half each way).
    pub latency_ms: u32,
    /// Random extra delay of up to this many milliseconds per message.
    pub jitter_ms: u32,
    /// Percentage of unreliable messages dropped. (Reliable ones are never
    /// dropped: renet would resend them anyway.)
    pub loss_percent: u32,
}

impl LagSettings {
    pub const PRESETS: [LagSettings; 4] = [
        LagSettings {
            latency_ms: 0,
            jitter_ms: 0,
            loss_percent: 0,
        },
        LagSettings {
            latency_ms: 100,
            jitter_ms: 10,
            loss_percent: 0,
        },
        LagSettings {
            latency_ms: 150,
            jitter_ms: 30,
            loss_percent: 5,
        },
        LagSettings {
            latency_ms: 300,
            jitter_ms: 50,
            loss_percent: 10,
        },
    ];

    pub fn is_on(&self) -> bool {
        *self != Self::default()
    }

    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
        };
        Self {
            latency_ms: value("--lag"),
            jitter_ms: value("--jitter"),
            loss_percent: value("--loss"),
        }
    }
}

/// A message held back by the lag simulation until `release` (seconds).
struct Delayed<T> {
    release: f64,
    channel: Channel,
    item: T,
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
    pub lag: LagSettings,
    outgoing: VecDeque<Delayed<Vec<u8>>>,
    incoming: VecDeque<Delayed<ServerMessage>>,
    /// Latest release time per direction for reliable messages, which must
    /// stay in order.
    last_reliable_out: f64,
    last_reliable_in: f64,
    /// Seconds since startup, updated every frame.
    clock: f64,
    rng: u64,
}

impl Connection {
    /// Start single player: run a server in the background and connect to it.
    pub fn single_player(opts: &SinglePlayer, name: &str) -> Self {
        let world = opts.world_name.as_str();
        let config = ServerConfig {
            game_mode: opts.mode.clone(),
            creative: opts.creative,
            // Extra mods can be dropped in here (next to the worlds folder).
            mods_dir: saves_dir().parent().map(|p| p.join("mods")),
            world_dir: saves_dir().join(&opts.folder),
            world_name: world.to_owned(),
            seed: opts.seed,
            server_name: format!("{name}'s world"),
            bind: opts
                .lan
                .then(|| SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT))),
            ..default()
        };
        info!("Starting single player world '{world}'");
        if opts.lan {
            // Open to the network: play over UDP on this computer, exactly
            // like friends who join, so both kinds of player are treated
            // the same.
            let server = EmbeddedServer::start_networked(config.clone());
            match Self::join(&format!("127.0.0.1:{DEFAULT_PORT}"), name) {
                Ok(mut conn) => {
                    conn.embedded = Some(server);
                    conn.target = world.to_owned();
                    return conn;
                }
                Err(e) => error!("Couldn't connect to our own server: {e}"),
            }
        }
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
            lag: LagSettings::default(),
            outgoing: VecDeque::new(),
            incoming: VecDeque::new(),
            last_reliable_out: 0.0,
            last_reliable_in: 0.0,
            clock: 0.0,
            rng: 0x9E37_79B9_7F4A_7C15,
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
            lag: LagSettings::default(),
            outgoing: VecDeque::new(),
            incoming: VecDeque::new(),
            last_reliable_out: 0.0,
            last_reliable_in: 0.0,
            clock: 0.0,
            rng: client_id,
        })
    }

    pub fn send(&mut self, channel: Channel, msg: &ClientMessage) {
        if !self.client.is_connected() {
            return;
        }
        let bytes = protocol::encode(msg);
        if !self.lag.is_on() {
            self.client.send_message(channel, bytes);
            return;
        }
        if let Some(release) = self.lag_release(channel, true) {
            self.outgoing.push_back(Delayed {
                release,
                channel,
                item: bytes,
            });
        }
    }

    /// A random number in `0..n` (cheap xorshift; only for lag simulation).
    fn random(&mut self, n: u32) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % n.max(1) as u64) as u32
    }

    /// When a message should be released, or `None` to drop it.
    fn lag_release(&mut self, channel: Channel, outgoing: bool) -> Option<f64> {
        if channel == Channel::Unreliable && self.random(100) < self.lag.loss_percent {
            return None;
        }
        let jitter = self.random(self.lag.jitter_ms + 1);
        let delay = (self.lag.latency_ms / 2 + jitter) as f64 / 1000.0;
        let mut release = self.clock + delay;
        if channel == Channel::Reliable {
            let last = if outgoing {
                &mut self.last_reliable_out
            } else {
                &mut self.last_reliable_in
            };
            release = release.max(*last);
            *last = release;
        }
        Some(release)
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

fn connect_from_launch_options(
    mut commands: Commands,
    options: Option<Res<LaunchOptions>>,
    settings: Res<crate::settings::ClientSettings>,
) {
    let mut options = options
        .map(|o| o.clone())
        .unwrap_or_else(LaunchOptions::from_args);
    // Without --name, use the name from Settings.
    if !std::env::args().any(|a| a == "--name") {
        match &mut options {
            LaunchOptions::SinglePlayer { name, .. } | LaunchOptions::Join { name, .. } => {
                *name = settings.name.clone();
            }
            LaunchOptions::None => {}
        }
    }
    let lag = LagSettings::from_args();
    if lag.is_on() {
        info!("Simulating lag: {lag:?}");
    }

    match options {
        LaunchOptions::SinglePlayer {
            world,
            seed,
            lan,
            name,
            mode,
            creative,
        } => {
            let opts = SinglePlayer {
                folder: folder_name(&world),
                world_name: world,
                seed,
                lan,
                mode,
                creative,
            };
            let mut conn = Connection::single_player(&opts, &name);
            conn.lag = lag;
            commands.insert_resource(conn);
        }
        LaunchOptions::Join { address, name } => match Connection::join(&address, &name) {
            Ok(mut conn) => {
                conn.lag = lag;
                commands.insert_resource(conn);
            }
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
    conn.clock = time.elapsed_secs_f64();
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
            let release = if conn.lag.is_on() {
                conn.lag_release(channel, false)
            } else {
                Some(conn.clock)
            };
            if let Some(release) = release {
                conn.incoming.push_back(Delayed {
                    release,
                    channel,
                    item: msg,
                });
            }
        }
    }

    // Deliver messages whose (simulated) travel time is up.
    let now = conn.clock;
    let mut ready = Vec::new();
    conn.incoming.retain_mut(|d| {
        if d.release <= now {
            ready.push((
                d.release,
                std::mem::replace(&mut d.item, ServerMessage::PlayerLeft(0)),
            ));
            false
        } else {
            true
        }
    });
    ready.sort_by(|a, b| a.0.total_cmp(&b.0));
    {
        for (_, msg) in ready {
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

fn send(time: Res<Time<Real>>, mut conn: ResMut<Connection>) {
    let conn = &mut *conn;
    // Hand over messages the lag simulation was holding back.
    let now = time.elapsed_secs_f64();
    while let Some(d) = conn.outgoing.front() {
        if d.release > now {
            break;
        }
        let d = conn.outgoing.pop_front().unwrap();
        if conn.client.is_connected() {
            conn.client.send_message(d.channel, d.item);
        }
    }
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

/// F7 cycles through lag simulation presets.
fn cycle_lag_presets(keys: Res<ButtonInput<KeyCode>>, mut conn: ResMut<Connection>) {
    if keys.just_pressed(KeyCode::F7) {
        let presets = LagSettings::PRESETS;
        let i = presets
            .iter()
            .position(|p| *p == conn.lag)
            .map_or(0, |i| i + 1);
        conn.lag = presets[i % presets.len()];
        info!("Lag simulation: {:?}", conn.lag);
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

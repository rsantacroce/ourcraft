//! End-to-end test of the Splat Squad game mode: a real server (with the
//! bundled mods) and two headless "bot" players talking to it over UDP.
//!
//! Needs `mods/dist/team_shooter.wasm` (run `mods/build.sh`); skipped
//! otherwise.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ourcraft_common::protocol::{
    self, Channel, ClientMessage, PROTOCOL_ID, PROTOCOL_VERSION, ServerMessage,
};
use ourcraft_mod_api::EffectKind;
use ourcraft_server::net::connection_config;
use ourcraft_server::{SaveNowFlag, ServerConfig, StopFlag, build_app};
use renet::RenetClient;
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};

struct Bot {
    client: RenetClient,
    transport: NetcodeClientTransport,
    received: Vec<ServerMessage>,
    id: u64,
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
            id,
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

#[test]
fn splat_squad_full_flow() {
    let wasm = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mods/dist/team_shooter.wasm"
    );
    if !std::path::Path::new(wasm).exists() {
        eprintln!("skipping: {wasm} not built");
        return;
    }
    let dir = std::env::temp_dir().join(format!("ourcraft-splat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: dir.clone(),
        seed: Some(4242),
        game_mode: Some("team_shooter".into()),
        bind: Some(addr),
        max_view_distance: 3,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));

    let mut alex = Bot::connect(addr, 101);
    let hello = |name: &str| ClientMessage::Hello {
        name: name.into(),
        view_distance: 3,
        protocol_version: PROTOCOL_VERSION,
    };

    // --- Alex joins: Red team, class badges, rules, a welcome title.
    assert!(run_until(&mut [&mut alex], 5.0, |b| b[0]
        .client
        .is_connected()));
    alex.send(&hello("Alex"));
    let joined = run_until(&mut [&mut alex], 10.0, |b| {
        b[0].has(|m| matches!(m, ServerMessage::Tools(t) if t.iter().any(|t| t.contains("badge"))))
    });
    assert!(
        joined,
        "no class badges; got {:?}",
        alex.received.iter().map(std::mem::discriminant).count()
    );
    let welcome = alex.received.iter().find_map(|m| match m {
        ServerMessage::Welcome { content, .. } => Some(content.clone()),
        _ => None,
    });
    let content = welcome.expect("welcome");
    assert!(content.tools.iter().any(|t| t.name == "splat:blaster"));
    assert!(content.models.iter().any(|m| m.name == "splat:flag_red"));
    assert!(alex.has(|m| matches!(m, ServerMessage::Rules(r) if !r.can_build && !r.can_fly && r.tag.as_deref() == Some("[Red]"))));
    // Both flags appear.
    assert!(run_until(&mut [&mut alex], 3.0, |b| {
        ["splat:flag_red", "splat:flag_blue"].iter().all(|f| {
            b[0].has(|m| matches!(m, ServerMessage::EntitySpawned { model, .. } if model == f))
        })
    }));

    // --- Alex picks Dasher: gets the blaster and a full paint meter.
    alex.send(&ClientMessage::UseTool {
        tool: "splat:badge_dasher".into(),
        direction: [0.0, 0.0, -1.0],
        secondary: false,
    });
    assert!(
        run_until(&mut [&mut alex], 5.0, |b| {
            b[0].has(|m| matches!(m, ServerMessage::Tools(t) if t == &["splat:blaster".to_string()]))
                && b[0].has(|m| matches!(m, ServerMessage::Health { health, max } if *health == 80.0 && *max == 80.0))
        }),
        "no blaster/health after picking a class"
    );
    // The HUD shows the game.
    assert!(run_until(&mut [&mut alex], 3.0, |b| {
        b[0].has(
            |m| matches!(m, ServerMessage::Hud(l) if l.iter().any(|l| l.text == "SPLAT SQUAD")),
        )
    }));

    // --- Sam joins: Blue team (teams are balanced).
    let mut sam = Bot::connect(addr, 202);
    assert!(run_until(&mut [&mut alex, &mut sam], 5.0, |b| b[1]
        .client
        .is_connected()));
    sam.send(&hello("Sam"));
    assert!(
        run_until(&mut [&mut alex, &mut sam], 8.0, |b| {
            b[0].has(|m| matches!(m, ServerMessage::Appearance { player, tag, .. } if *player == 202 && tag.as_deref() == Some("[Blue]")))
        }),
        "Sam wasn't put on Blue"
    );

    // --- Alex fires at the ground: a projectile flies, hits, and splats.
    alex.received.clear();
    // Wait out the badge cooldown.
    run_until(&mut [&mut alex, &mut sam], 0.6, |_| false);
    // Keep pulling the trigger now and then: on a busy machine the server
    // may still count the badge cooldown when the first shot arrives.
    // (Generous time: these tests share the machine with the others.)
    // (A shot at your own feet can spawn and land in the same server tick,
    // and then only its removal is sent.)
    let splat_done = |b: &[&mut Bot]| {
        b[0].has(|m| matches!(m, ServerMessage::Effect { kind: EffectKind::Splat, .. }))
            && b[0].has(|m| matches!(m, ServerMessage::EntityRemoved(_)))
    };
    let mut splatted = false;
    for _ in 0..12 {
        alex.send(&ClientMessage::UseTool {
            tool: "splat:blaster".into(),
            direction: [0.0, -1.0, 0.0],
            secondary: false,
        });
        if run_until(&mut [&mut alex, &mut sam], 1.0, splat_done) {
            splatted = true;
            break;
        }
    }
    assert!(
        splatted,
        "the shot didn't splat: {:#?}",
        alex.received
            .iter()
            .filter(|m| !matches!(
                m,
                ServerMessage::Column { .. }
                    | ServerMessage::Hud(_)
                    | ServerMessage::PlayerStates { .. }
                    | ServerMessage::YourState { .. }
                    | ServerMessage::BlockChanged { .. }
            ))
            .collect::<Vec<_>>()
    );

    // --- Building is off in this mode.
    alex.send(&ClientMessage::SetBlock {
        pos: [0, 70, 0],
        block: 1,
        held: 0,
    });

    // --- Chat commands come from the mod.
    sam.send(&ClientMessage::Chat {
        text: "/score".into(),
    });
    assert!(run_until(&mut [&mut alex, &mut sam], 3.0, |b| {
        b[1].has(
            |m| matches!(m, ServerMessage::Chat { text, .. } if text.starts_with("Red 0 - Blue 0")),
        )
    }));

    let _ = sam.id;
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

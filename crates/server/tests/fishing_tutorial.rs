//! The fishing tutorial (`mods/tutorials/fishing`): every step's script
//! loads without a problem, and the finished mod works on a real server:
//! a bot casts a line, catches a fish, builds the market, sells its catch
//! and buys bread. Skipped unless the EELisp runner is built
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

const STEPS: [(&str, &str); 8] = [
    (
        "1-hello",
        include_str!("../../../mods/tutorials/fishing/steps/1-hello.el"),
    ),
    (
        "2-draw-a-fish",
        include_str!("../../../mods/tutorials/fishing/steps/2-draw-a-fish.el"),
    ),
    (
        "3-catch-and-cook",
        include_str!("../../../mods/tutorials/fishing/steps/3-catch-and-cook.el"),
    ),
    (
        "4-many-fish",
        include_str!("../../../mods/tutorials/fishing/steps/4-many-fish.el"),
    ),
    (
        "5-fishing-places",
        include_str!("../../../mods/tutorials/fishing/steps/5-fishing-places.el"),
    ),
    (
        "6-better-rods",
        include_str!("../../../mods/tutorials/fishing/steps/6-better-rods.el"),
    ),
    (
        "7-fish-market",
        include_str!("../../../mods/tutorials/fishing/steps/7-fish-market.el"),
    ),
    (
        "8-feast",
        include_str!("../../../mods/tutorials/fishing/steps/8-feast.el"),
    ),
];

/// A little helper mod for the test: /pond digs a pool of water next to
/// you, so there's somewhere to fish.
const POND: &str = r#"
(defn on-chat (player text)
  (when (= text "/pond")
    (let (p (player-pos player) x (nth p 0) y (nth p 1) z (nth p 2))
      (fill (- x 4) (- y 1) (- z 1) (- x 2) (- y 1) (+ z 1) "core:water")
      (tell player (str "pond " (- x 3) " " (- y 1) " " z)))
    true))
"#;

fn runner_built() -> bool {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mods/runtimes/eelisp.wasm"
    ))
    .exists()
}

/// Start a server running these scripts; returns its address and how to
/// stop it.
fn serve(
    tag: &str,
    scripts: &[(&str, &str)],
) -> (
    SocketAddr,
    StopFlag,
    std::thread::JoinHandle<()>,
    std::path::PathBuf,
) {
    let dir = std::env::temp_dir().join(format!("ourcraft-fishing-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mods = dir.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    for (name, source) in scripts {
        std::fs::write(mods.join(format!("{name}.el")), source).unwrap();
    }
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
        creative: false,
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
    (addr, stop, server, dir)
}

fn problems(b: &Bot) -> Vec<String> {
    chats(b)
        .into_iter()
        .filter(|c| c.starts_with('⚠'))
        .collect()
}

fn item_id(bot: &Bot, name: &str) -> Option<u16> {
    bot.received.iter().find_map(|m| match m {
        ServerMessage::Welcome { content, .. } => content
            .items
            .iter()
            .position(|i| i.name == name)
            .map(|i| i as u16),
        _ => None,
    })
}

fn item_name(bot: &Bot, id: u16) -> String {
    bot.received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { content, .. } => {
                content.items.get(id as usize).map(|i| i.name.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn given(bot: &Bot) -> Vec<(String, u16)> {
    bot.received
        .iter()
        .filter_map(|m| match m {
            ServerMessage::GiveItems(s) => Some((item_name(bot, s.item), s.count)),
            _ => None,
        })
        .collect()
}

fn say(bot: &mut Bot, text: &str) {
    bot.send(&ClientMessage::Chat { text: text.into() });
}

#[test]
fn every_step_of_the_tutorial_loads() {
    if !runner_built() {
        eprintln!("skipping: the EELisp runner isn't built");
        return;
    }
    for (i, (name, source)) in STEPS.iter().enumerate() {
        let (addr, stop, server, dir) = serve(name, &[("fishing", source)]);
        let mut bot = join(addr, 700 + i as u64, "Kit");
        say(&mut bot, "/fishing");
        // Every step answers /fishing (the first thing the tutorial tries).
        let answered = run_until(&mut [&mut bot], 3.0, |b| {
            chats(b[0]).iter().filter(|c| !c.starts_with('⚠')).count() >= 2
        });
        run_until(&mut [&mut bot], 0.3, |_| false);
        stop.0.store(true, Ordering::Relaxed);
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(problems(&bot).is_empty(), "{name}: {:?}", problems(&bot));
        assert!(answered, "{name} didn't answer /fishing: {:?}", chats(&bot));
        if i >= 1 {
            assert!(item_id(&bot, "fish:perch").is_some(), "{name} has no perch");
        }
    }
}

#[test]
fn catch_sell_and_buy() {
    if !runner_built() {
        eprintln!("skipping: the EELisp runner isn't built");
        return;
    }
    let (addr, stop, server, dir) = serve("play", &[("fishing", STEPS[7].1), ("pond", POND)]);
    let mut kit = join(addr, 801, "Kit");
    for name in [
        "fish:perch",
        "fish:cooked_perch",
        "fish:lucky_rod",
        "fish:golden_rod",
        "fish:coin",
        "fish:feast",
    ] {
        assert!(item_id(&kit, name).is_some(), "no {name}");
    }
    assert!(block_id(&kit, "fish:market").is_some());
    let rod = item_id(&kit, "fish:lucky_rod").unwrap();

    // Dig a pond and cast into it.
    say(&mut kit, "/pond");
    assert!(run_until(&mut [&mut kit], 3.0, |b| chats(b[0])
        .iter()
        .any(|c| c.starts_with("pond "))));
    let spot: Vec<i32> = chats(&kit)
        .iter()
        .find(|c| c.starts_with("pond "))
        .unwrap()
        .split(' ')
        .skip(1)
        .map(|v| v.parse().unwrap())
        .collect();
    run_until(&mut [&mut kit], 0.5, |_| false);
    kit.send(&ClientMessage::UseBlock {
        pos: [spot[0], spot[1], spot[2]],
        held: rod,
    });
    // A bite comes within 12 seconds: the mod's catch, not a plain fish.
    assert!(
        run_until(&mut [&mut kit], 14.0, |b| !given(b[0]).is_empty()),
        "nothing bit: {:?}",
        chats(&kit)
    );
    let (caught, _) = given(&kit)[0].clone();
    assert!(caught.starts_with("fish:"), "caught {caught}");
    assert!(
        chats(&kit)
            .iter()
            .any(|c| c.starts_with("You caught a") || c.contains("Old Boot")),
        "{:?}",
        chats(&kit)
    );

    // Selling away from a market doesn't work…
    say(&mut kit, "/sell");
    assert!(run_until(&mut [&mut kit], 3.0, |b| chats(b[0])
        .iter()
        .any(|c| c.starts_with("Stand next to a Fish Market"))));
    // …next to one it does.
    let market = block_id(&kit, "fish:market").unwrap();
    say(&mut kit, "/market");
    assert!(run_until(&mut [&mut kit], 3.0, |b| changed_to(
        b[0], market
    )
    .len()
        == 1));
    kit.received
        .retain(|m| !matches!(m, ServerMessage::GiveItems(_)));
    say(&mut kit, "/sell");
    assert!(
        run_until(&mut [&mut kit], 3.0, |b| chats(b[0])
            .iter()
            .any(|c| c.starts_with("You sold your fish for"))),
        "{:?}",
        chats(&kit)
    );
    let coins: u16 = given(&kit)
        .iter()
        .filter(|(n, _)| n == "fish:coin")
        .map(|(_, c)| c)
        .sum();
    assert!(coins >= 1);
    assert!(
        kit.has(|m| matches!(m, ServerMessage::TakeItems(s) if item_name(&kit, s.item) == caught))
    );

    // Spend them: bread costs 2.
    kit.received
        .retain(|m| !matches!(m, ServerMessage::GiveItems(_)));
    say(&mut kit, "/buy 3");
    if coins >= 2 {
        assert!(
            run_until(&mut [&mut kit], 3.0, |b| given(b[0])
                .iter()
                .any(|(n, _)| n == "core:bread")),
            "{:?}",
            chats(&kit)
        );
    } else {
        assert!(run_until(&mut [&mut kit], 3.0, |b| chats(b[0])
            .iter()
            .any(|c| c.starts_with("You need 2"))));
    }
    say(&mut kit, "/buy 9");
    assert!(run_until(&mut [&mut kit], 3.0, |b| chats(b[0])
        .iter()
        .any(|c| c.starts_with("There's nothing with that number"))));
    say(&mut kit, "/journal");
    assert!(run_until(&mut [&mut kit], 3.0, |b| chats(b[0])
        .iter()
        .any(|c| c.starts_with("You have found"))));

    assert!(problems(&kit).is_empty(), "{:?}", problems(&kit));
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A child's typo must show up where they can see it: in the chat.
#[test]
fn a_mistake_in_a_script_is_shown_in_the_chat() {
    if !runner_built() {
        eprintln!("skipping: the EELisp runner isn't built");
        return;
    }
    let broken = r#"
(defn on-chat (player text)
  (when (= text "/oops") (tell player (+ 1 "two")) true))
(register-item {:name "fish:perch" :display "Puddle Perch" :picture ["bz"] :colors {:b [1 2 3]}})
"#;
    let (addr, stop, server, dir) = serve("oops", &[("fishing", broken)]);
    let mut kit = join(addr, 901, "Kit");
    say(&mut kit, "/oops");
    run_until(&mut [&mut kit], 2.0, |b| problems(b[0]).len() >= 2);
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    let _ = std::fs::remove_dir_all(&dir);
    let p = problems(&kit);
    assert!(
        p.iter().any(|c| c.contains("letter z")),
        "the picture mistake: {p:?}"
    );
    assert!(
        p.iter().any(|c| c.contains("on-chat")),
        "the /oops mistake: {p:?}"
    );
    eprintln!("{p:#?}");
}

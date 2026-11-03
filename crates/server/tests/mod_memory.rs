//! Mods remember things: what a mod saves with `save-data` comes back
//! the next time the world opens; a mod switched off for a world doesn't
//! take its blocks and items with it; and a script's mistakes say which
//! line they're on. Skipped unless the EELisp runner is built.

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

fn runner_built() -> bool {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mods/runtimes/eelisp.wasm"
    ))
    .exists()
}

/// Run the world in `world` once, with these mods switched on, and a bot
/// that joins and says each line of `say`. Returns everything it heard.
fn play(
    mods: &std::path::Path,
    world: &std::path::Path,
    on: &[&str],
    say: &[&str],
    id: u64,
) -> Bot {
    let list: Vec<String> = on.iter().map(|s| (*s).to_owned()).collect();
    ourcraft_common::save::save_world_mods(world, &list).unwrap();
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let config = ServerConfig {
        world_dir: world.to_owned(),
        seed: Some(31),
        bind: Some(addr),
        max_view_distance: 2,
        creative: true,
        mods_dir: Some(mods.to_owned()),
        per_world_mods: true,
        ..Default::default()
    };
    let stop = StopFlag::default();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        build_app(config, None, server_stop, SaveNowFlag::default(), false).run();
    });
    std::thread::sleep(Duration::from_millis(300));
    let mut bot = join(addr, id, "Kit");
    run_until(&mut [&mut bot], 1.0, |_| false);
    for line in say {
        bot.send(&ClientMessage::Chat {
            text: (*line).into(),
        });
        run_until(&mut [&mut bot], 1.0, |_| false);
    }
    stop.0.store(true, Ordering::Relaxed);
    let _ = server.join();
    bot
}

fn names(bot: &Bot, blocks: bool) -> Vec<String> {
    bot.received
        .iter()
        .find_map(|m| match m {
            ServerMessage::Welcome { content, .. } => Some(if blocks {
                content.blocks.iter().map(|b| b.name.clone()).collect()
            } else {
                content.items.iter().map(|i| i.name.clone()).collect()
            }),
            _ => None,
        })
        .unwrap_or_default()
}

const NOTEBOOK: &str = r#"
;; Counts how often the world has been opened, and keeps a few kinds of things.
(def opened (+ 1 (or (load-data "opened") 0)))
(save-data "opened" opened)
(defn on-join (player name)
  (tell player (str "opened " opened))
  (tell player (str "kept " (load-data "things"))))
(defn on-chat (player text)
  (when (= text "/keep")
    (save-data "things" {:list [1 "two \"quoted\"" :three -4.5] :ok true :none nil})
    (tell player "kept it")
    true))
"#;

/// Places a glow block (from Glow Garden) and gives a Fish Coin (from
/// Fishing Fun) once; later, says whether they're still there.
const PROBE: &str = r#"
(defn spot () (let (s (world-spawn)) [(+ (nth s 0) 3) (+ (nth s 1) 1) (nth s 2)]))
(defn on-chat (player text)
  (cond
    (= text "/plant")
    (let (p (spot))
      (set-block (nth p 0) (nth p 1) (nth p 2) "garden:glow")
      (give-item player "fish:coin" 5)
      (tell player "planted")
      true)
    (= text "/look")
    (let (p (spot))
      (tell player (str "glow there: " (= (get-block (nth p 0) (nth p 1) (nth p 2)) (block-id "garden:glow"))))
      (tell player (str "coins: " (count-item player "fish:coin")))
      true)
    true nil))
"#;

#[test]
fn what_a_mod_saves_comes_back_next_time() {
    if !runner_built() {
        eprintln!("skipping: the EELisp runner isn't built");
        return;
    }
    let dir = std::env::temp_dir().join(format!("ourcraft-memory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mods = dir.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    std::fs::write(mods.join("notebook.el"), NOTEBOOK).unwrap();
    let world = dir.join("world");

    let first = play(&mods, &world, &["notebook"], &["/keep"], 1001);
    assert!(
        chats(&first).iter().any(|c| c == "opened 1"),
        "{:?}",
        chats(&first)
    );
    assert!(chats(&first).iter().any(|c| c == "kept it"));

    let second = play(&mods, &world, &["notebook"], &[], 1002);
    let heard = chats(&second);
    assert!(heard.iter().any(|c| c == "opened 2"), "{heard:?}");
    let kept = heard.iter().find(|c| c.starts_with("kept ")).unwrap();
    for part in ["two \"quoted\"", ":three", "-4.5", "true"] {
        assert!(kept.contains(part), "{part} missing from {kept}");
    }
    assert!(world.join("mod_data/notebook.dat").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn switching_a_mod_off_keeps_its_blocks_and_items() {
    if !runner_built() {
        eprintln!("skipping: the EELisp runner isn't built");
        return;
    }
    let dir = std::env::temp_dir().join(format!("ourcraft-kept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mods = dir.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    std::fs::write(
        mods.join("glow_garden.el"),
        include_str!("../../../mods/eelisp_runner/examples/glow_garden.el"),
    )
    .unwrap();
    std::fs::write(
        mods.join("fishing.el"),
        include_str!("../../../mods/tutorials/fishing/steps/8-feast.el"),
    )
    .unwrap();
    std::fs::write(mods.join("probe.el"), PROBE).unwrap();
    let world = dir.join("world");

    let all = ["glow_garden", "fishing", "probe"];
    let first = play(&mods, &world, &all, &["/plant", "/look"], 1101);
    assert!(
        chats(&first).iter().any(|c| c == "glow there: true"),
        "{:?}",
        chats(&first)
    );

    // Both mods off: their block and item are still in the game.
    let off = play(&mods, &world, &["probe"], &["/look"], 1102);
    assert!(
        names(&off, true).iter().any(|n| n == "garden:glow"),
        "the block was dropped"
    );
    assert!(
        names(&off, false).iter().any(|n| n == "fish:coin"),
        "the item was dropped"
    );
    assert!(
        chats(&off).iter().any(|c| c == "glow there: true"),
        "{:?}",
        chats(&off)
    );
    assert!(
        chats(&off).iter().any(|c| c == "coins: 5"),
        "{:?}",
        chats(&off)
    );

    // Back on: everything as it was.
    let back = play(&mods, &world, &all, &["/look"], 1103);
    assert!(
        chats(&back).iter().any(|c| c == "glow there: true"),
        "{:?}",
        chats(&back)
    );
    assert!(
        chats(&back).iter().any(|c| c == "coins: 5"),
        "{:?}",
        chats(&back)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_mistake_says_which_line_it_is_on() {
    if !runner_built() {
        eprintln!("skipping: the EELisp runner isn't built");
        return;
    }
    let dir = std::env::temp_dir().join(format!("ourcraft-lines-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mods = dir.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    // A missing bracket (the ( on line 3), and a mistake on line 4.
    std::fs::write(
        mods.join("unclosed.el"),
        "; hello\n\n(defn on-join (player name)\n  (tell player \"hi\")\n",
    )
    .unwrap();
    std::fs::write(
        mods.join("wrong.el"),
        "; hello\n(def a 1)\n\n(def b (+ a \"two\"))\n(defn on-chat (player text)\n  (when (= text \"/oops\") (tell player (nope)) true))\n",
    )
    .unwrap();
    std::fs::write(
        mods.join("later.el"),
        "; line 1\n; line 2\n(defn on-chat (player text)\n  (when (= text \"/oops\") (tell player (nope)) true))\n",
    )
    .unwrap();
    let world = dir.join("world");
    let bot = play(
        &mods,
        &world,
        &["unclosed", "wrong", "later"],
        &["/oops"],
        1201,
    );
    let p: Vec<String> = chats(&bot)
        .into_iter()
        .filter(|c| c.starts_with("(!)"))
        .collect();
    assert!(
        p.iter()
            .any(|c| c.ends_with("line 3: this ( is never closed")),
        "{p:?}"
    );
    assert!(
        p.iter().any(|c| c.contains("line 4: Type mismatch")),
        "{p:?}"
    );
    assert!(
        p.iter()
            .any(|c| c.contains("(on-chat …) failed") && c.ends_with("(on-chat starts on line 3)")),
        "{p:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

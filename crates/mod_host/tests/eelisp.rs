//! The EELisp runner experiment (`mods/eelisp_runner`): a mod whose logic
//! is an EELisp script, run by the interpreter inside the sandbox. Skipped
//! unless it's been built:
//! `cd mods/eelisp_runner && cargo build --release --target wasm32-unknown-unknown`

use ourcraft_mod_api::{API_VERSION, Event, EventReply, HostCall, HostReply, PlayerInfo};
use ourcraft_mod_host::{FUEL_PER_EVENT, Host, ModRuntime};

/// A pretend game: one player standing at (10, 64, 20), and it remembers
/// what the mod did.
#[derive(Default)]
struct World {
    log: Vec<String>,
    chat: Vec<String>,
    blocks: Vec<[i32; 3]>,
}

impl Host for World {
    fn handle(&mut self, _mod_name: &str, call: HostCall) -> HostReply {
        match call {
            HostCall::Log(s) => self.log.push(s),
            HostCall::Chat { text, .. } => self.chat.push(text),
            HostCall::SetBlock { pos, .. } => self.blocks.push(pos),
            HostCall::BlockId(_) => return HostReply::Block(3),
            HostCall::GetPlayers => {
                return HostReply::Players(vec![PlayerInfo {
                    id: 7,
                    name: "Ada".into(),
                    position: [10.5, 64.0, 20.5],
                    velocity: [0.0; 3],
                    look: [0.0, 0.0, 1.0],
                    on_ground: true,
                }]);
            }
            _ => {}
        }
        HostReply::Ok
    }
}

fn chat(rt: &mut ModRuntime<World>, world: World, text: &str) -> (World, EventReply, u64) {
    let (world, reply) = rt.dispatch_until_handled(
        world,
        &Event::Chat {
            player: 7,
            text: text.into(),
        },
    );
    (world, reply, rt.mods[0].last_fuel_used)
}

fn percent(fuel: u64) -> String {
    format!("{:.2}%", fuel as f64 * 100.0 / FUEL_PER_EVENT as f64)
}

#[test]
fn an_eelisp_script_runs_as_a_mod() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../mods/eelisp_runner/target/wasm32-unknown-unknown/release/eelisp_runner.wasm"
    );
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("skipping: {path} not built");
        return;
    };
    let mut rt = ModRuntime::<World>::new().unwrap();
    // Loads: it needs nothing from outside but the game.
    rt.load("eelisp_runner", &bytes).unwrap();
    let (world, _) = rt.dispatch(
        World::default(),
        &Event::Init {
            api_version: API_VERSION,
            game_mode: None,
        },
    );
    let init = rt.mods[0].last_fuel_used;
    assert!(
        world.log.iter().any(|l| l.contains("script loaded")),
        "{:?}",
        world.log
    );
    eprintln!("start-up (interpreter + prelude + script): {}", percent(init));

    let (world, _) = rt.dispatch(
        world,
        &Event::PlayerJoined {
            player: 7,
            name: "Ada".into(),
        },
    );
    assert!(world.chat.last().unwrap().starts_with("Hello Ada!"));

    // /tower: ten blocks straight up, two in front.
    let (world, reply, fuel) = chat(&mut rt, world, "/tower");
    assert_eq!(reply, EventReply::Handled);
    assert_eq!(world.blocks.len(), 10);
    assert_eq!(world.blocks[0], [10, 64, 22]);
    assert_eq!(world.blocks[9], [10, 73, 22]);
    eprintln!("/tower (10 blocks): {}", percent(fuel));

    // Other chat isn't the script's.
    let (world, reply, _) = chat(&mut rt, world, "hello everyone");
    assert_eq!(reply, EventReply::Ok);

    // How far block-by-block Lisp goes in one event…
    let mut world = world;
    for n in [10, 100] {
        let before = world.blocks.len();
        let (w, reply, fuel) = chat(&mut rt, world, &format!("/wall {n}"));
        world = w;
        assert_eq!(reply, EventReply::Handled);
        assert_eq!(world.blocks.len() - before, n * 3);
        eprintln!("/wall {n} ({} blocks, one at a time): {}", n * 3, percent(fuel));
    }
    // …and with `fill` doing the loop.
    let before = world.blocks.len();
    let (world, _, fuel) = chat(&mut rt, world, "/bigwall 100");
    assert_eq!(world.blocks.len() - before, 300);
    eprintln!("/bigwall 100 (300 blocks with fill): {}", percent(fuel));
    assert!(rt.mods[0].enabled);
}

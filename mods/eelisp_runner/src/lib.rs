//! # EELisp runner (experiment)
//!
//! An Ourcraft mod whose game logic is an EELisp script (`demo.el`). The
//! EELisp interpreter runs inside the mod, so the script is in the same
//! sandbox as any other mod: no files, no network, and a time limit on
//! every event.
//!
//! The game's events call Lisp functions the script defines (`on-start`,
//! `on-join`, `on-chat`), and the script changes the game with functions
//! this runner adds to the language (`set-block`, `chat`, `fill`, …).

use std::rc::Rc;

use eelisp::env::{self, Env};
use eelisp::value::{ArgMode, Builtin, OrderedDict};
use eelisp::{Interpreter, LispError, Value};
use ourcraft_mod_api::guest::{self, Mod};
use ourcraft_mod_api::{Event, EventReply};

/// The script. Later the game could hand the runner `.el` files from the
/// mods folder instead.
const SCRIPT: &str = include_str!("../demo.el");

/// Most blocks one `fill` may change.
const MAX_FILL: i64 = 32 * 32 * 32;

#[derive(Default)]
struct Runner {
    it: Option<Interpreter>,
}

impl Mod for Runner {
    fn on_event(&mut self, event: Event) -> EventReply {
        if let Event::Init { .. } = event {
            let it = Interpreter::new();
            it.set_echo(false);
            add_game_functions(&it.global);
            match it.eval_str(SCRIPT) {
                Ok(_) => guest::log("EELisp runner: script loaded"),
                Err(e) => guest::log(&format!("EELisp runner: the script has a problem: {e}")),
            }
            self.it = Some(it);
            return EventReply::Ok;
        }
        let Some(it) = &self.it else {
            return EventReply::Ok;
        };
        let (name, args) = match event {
            Event::Start => ("on-start", vec![]),
            Event::PlayerJoined { player, name } => {
                ("on-join", vec![Value::Number(player as f64), Value::Str(name)])
            }
            Event::Chat { player, text } => {
                ("on-chat", vec![Value::Number(player as f64), Value::Str(text)])
            }
            _ => return EventReply::Ok,
        };
        let handled = call(it, name, &args);
        print_output(it);
        if handled && name == "on-chat" {
            EventReply::Handled
        } else {
            EventReply::Ok
        }
    }
}

ourcraft_mod_api::export_mod!(Runner);

/// Call the script's function `name`, if it has one. True if it returned
/// something other than false or nil.
fn call(it: &Interpreter, name: &str, args: &[Value]) -> bool {
    let Ok(f) = env::get(&it.global, name) else {
        return false; // the script doesn't handle this event
    };
    match eelisp::eval::apply_value(&f, args, &it.global) {
        Ok(v) => !matches!(v, Value::Null | Value::Bool(false)),
        Err(e) => {
            guest::log(&format!("EELisp runner: ({name} …) failed: {e}"));
            false
        }
    }
}

/// Anything the script printed goes to the server log.
fn print_output(it: &Interpreter) {
    let out = it.take_output();
    if !out.trim().is_empty() {
        guest::log(out.trim_end());
    }
}

// ---- The game's functions, as EELisp builtins ----

fn define(env: &Env, name: &str, f: impl Fn(&[Value]) -> Result<Value, LispError> + 'static) {
    env::define(
        env,
        name,
        Value::Builtin(Rc::new(Builtin {
            name: name.into(),
            arg_mode: ArgMode::Eval,
            func: Box::new(move |args, _| f(args)),
        })),
    );
}

fn num(args: &[Value], i: usize, what: &str) -> Result<f64, LispError> {
    match args.get(i) {
        Some(Value::Number(n)) => Ok(*n),
        _ => Err(LispError::Runtime(format!("expected a number for {what}"))),
    }
}

fn int(args: &[Value], i: usize, what: &str) -> Result<i32, LispError> {
    Ok(num(args, i, what)?.floor() as i32)
}

fn text(args: &[Value], i: usize) -> String {
    match args.get(i) {
        Some(Value::Str(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

/// A block given as a name (`"core:stone"`) or an id from `block-id`.
fn block(args: &[Value], i: usize) -> Result<u16, LispError> {
    match args.get(i) {
        Some(Value::Number(n)) => Ok(*n as u16),
        Some(Value::Str(s)) => guest::block_id(s)
            .ok_or_else(|| LispError::Runtime(format!("there's no block called {s}"))),
        _ => Err(LispError::Runtime("expected a block name or id".into())),
    }
}

fn list(items: impl IntoIterator<Item = Value>) -> Value {
    Value::List(Rc::new(items.into_iter().collect()))
}

fn add_game_functions(env: &Env) {
    define(env, "log", |a| {
        guest::log(&text(a, 0));
        Ok(Value::Null)
    });
    // (chat "text"): to everyone.
    define(env, "chat", |a| {
        guest::chat(None, &text(a, 0));
        Ok(Value::Null)
    });
    // (tell player "text"): to one player.
    define(env, "tell", |a| {
        guest::chat(Some(num(a, 0, "the player")? as u64), &text(a, 1));
        Ok(Value::Null)
    });
    // (show-title "text" seconds): big text for everyone.
    define(env, "show-title", |a| {
        let secs = num(a, 1, "the seconds").unwrap_or(3.0) as f32;
        guest::show_title(None, &text(a, 0), secs);
        Ok(Value::Null)
    });
    // (players): a list of {:id :name :x :y :z}.
    define(env, "players", |_| {
        Ok(list(guest::players().into_iter().map(|p| {
            let mut d = OrderedDict::default();
            d.insert("id".into(), Value::Number(p.id as f64));
            d.insert("name".into(), Value::Str(p.name));
            for (k, v) in ["x", "y", "z"].iter().zip(p.position) {
                d.insert((*k).into(), Value::Number(v as f64));
            }
            Value::Dict(Rc::new(d))
        })))
    });
    // (player-pos player): the block a player stands in, as (x y z).
    define(env, "player-pos", |a| {
        let id = num(a, 0, "the player")? as u64;
        let p = guest::players()
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| LispError::Runtime("no such player".into()))?;
        Ok(list(p.position.map(|v| Value::Number(v.floor() as f64))))
    });
    // (block-id "core:stone"): a block's id, or nil.
    define(env, "block-id", |a| {
        Ok(guest::block_id(&text(a, 0)).map_or(Value::Null, |id| Value::Number(id as f64)))
    });
    // (get-block x y z): the id of the block there.
    define(env, "get-block", |a| {
        let pos = [int(a, 0, "x")?, int(a, 1, "y")?, int(a, 2, "z")?];
        Ok(Value::Number(guest::get_block(pos) as f64))
    });
    // (set-block x y z block): true if it worked.
    define(env, "set-block", |a| {
        let pos = [int(a, 0, "x")?, int(a, 1, "y")?, int(a, 2, "z")?];
        Ok(Value::Bool(guest::try_set_block(pos, block(a, 3)?)))
    });
    // (fill x1 y1 z1 x2 y2 z2 block): a whole box at once (done here, in
    // Rust, so big builds stay fast). Returns how many blocks it changed.
    define(env, "fill", |a| {
        let mut c = [0; 6];
        for (i, v) in c.iter_mut().enumerate() {
            *v = int(a, i, "a corner")?;
        }
        let b = block(a, 6)?;
        let (lo, hi) = (
            [c[0].min(c[3]), c[1].min(c[4]), c[2].min(c[5])],
            [c[0].max(c[3]), c[1].max(c[4]), c[2].max(c[5])],
        );
        let size: i64 = (0..3).map(|i| (hi[i] - lo[i] + 1) as i64).product();
        if size > MAX_FILL {
            return Err(LispError::Runtime(format!(
                "that's {size} blocks; fill does at most {MAX_FILL}"
            )));
        }
        let mut done = 0;
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    if guest::try_set_block([x, y, z], b) {
                        done += 1;
                    }
                }
            }
        }
        Ok(Value::Number(done as f64))
    });
}

//! # EELisp runner
//!
//! Runs an Ourcraft mod written in EELisp. The game loads one copy of this
//! runner for each `name.el` file in the mods folder and hands it the
//! script ([`Event::Script`]). The EELisp interpreter runs inside the mod,
//! so the script is in the same sandbox as any other mod: no files, no
//! network, and a time limit on every event.
//!
//! The game's events call Lisp functions the script defines (`on-start`,
//! `on-join`, `on-chat`, `on-tick`, `on-block-change`), and the script
//! changes the game with functions this runner adds to the language
//! (`set-block`, `chat`, `fill`, …). See `README.md` next to this file.

use std::rc::Rc;

use eelisp::env::{self, Env};
use eelisp::value::{ArgMode, Builtin, OrderedDict};
use eelisp::{Interpreter, LispError, Value};
use ourcraft_mod_api::guest::{self, Mod};
use ourcraft_mod_api::{BlockSpec, EffectKind, Event, EventReply, Render, Rgba, speckled};

/// Most blocks one `fill` may change.
const MAX_FILL: i64 = 32 * 32 * 32;

#[derive(Default)]
struct Runner {
    /// The script, until `Init` runs it.
    script: Option<String>,
    it: Option<Interpreter>,
    /// Does the script have an `on-tick`? (Checked once: ticks are often.)
    ticks: bool,
}

impl Mod for Runner {
    fn on_event(&mut self, event: Event) -> EventReply {
        match event {
            Event::Script { source } => {
                self.script = Some(source);
                return EventReply::Ok;
            }
            Event::Init { .. } => {
                self.start();
                return EventReply::Ok;
            }
            _ => {}
        }
        let Some(it) = &self.it else {
            return EventReply::Ok;
        };
        let n = |v: u64| Value::Number(v as f64);
        let (name, args) = match event {
            Event::Start => ("on-start", vec![]),
            Event::Tick { tick, .. } if self.ticks => ("on-tick", vec![n(tick)]),
            Event::PlayerJoined { player, name } => ("on-join", vec![n(player), Value::Str(name)]),
            Event::PlayerLeft { player } => ("on-leave", vec![n(player)]),
            Event::Chat { player, text } => ("on-chat", vec![n(player), Value::Str(text)]),
            Event::BlockChange {
                player,
                pos,
                old,
                new,
            } => {
                let mut args = vec![n(player)];
                args.extend(pos.map(|v| Value::Number(v as f64)));
                args.extend([n(old as u64), n(new as u64)]);
                ("on-block-change", args)
            }
            _ => return EventReply::Ok,
        };
        let result = call(it, name, &args);
        print_output(it);
        match (name, result) {
            ("on-chat", Some(v)) if truthy(&v) => EventReply::Handled,
            ("on-block-change", Some(v)) if is_deny(&v) => EventReply::Deny,
            _ => EventReply::Ok,
        }
    }
}

impl Runner {
    /// Make the interpreter and run the script: it defines its event
    /// functions and registers its blocks now.
    fn start(&mut self) {
        let Some(script) = self.script.take() else {
            guest::log("No script to run (this runner is loaded for .el files)");
            return;
        };
        let it = Interpreter::new();
        it.set_echo(false);
        add_game_functions(&it.global);
        if let Err(e) = it.eval_str(&script) {
            guest::log(&format!("The script has a problem: {e}"));
        }
        print_output(&it);
        self.ticks = env::get(&it.global, "on-tick").is_ok();
        self.it = Some(it);
    }
}

ourcraft_mod_api::export_mod!(Runner);

/// Call the script's function `name`, if it has one, and return what it
/// returned.
fn call(it: &Interpreter, name: &str, args: &[Value]) -> Option<Value> {
    let f = env::get(&it.global, name).ok()?; // the script doesn't handle this event
    match eelisp::eval::apply_value(&f, args, &it.global) {
        Ok(v) => Some(v),
        Err(e) => {
            guest::log(&format!("({name} …) failed: {e}"));
            None
        }
    }
}

fn truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false))
}

/// `:deny` (or `"deny"`) from `on-block-change` stops the change.
fn is_deny(v: &Value) -> bool {
    match v {
        Value::Keyword(k) => k.to_string().trim_start_matches(':') == "deny",
        Value::Str(s) => s == "deny",
        _ => false,
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

/// An optional player: a number, or nil for everyone.
fn who(args: &[Value], i: usize) -> Option<u64> {
    match args.get(i) {
        Some(Value::Number(n)) => Some(*n as u64),
        _ => None,
    }
}

fn pos3(args: &[Value], i: usize) -> Result<[f32; 3], LispError> {
    Ok([
        num(args, i, "x")? as f32,
        num(args, i + 1, "y")? as f32,
        num(args, i + 2, "z")? as f32,
    ])
}

fn field<'a>(d: &'a OrderedDict, key: &str) -> Option<&'a Value> {
    d.get(key).or_else(|| d.get(&format!(":{key}")))
}

/// A colour as a list, `[r g b]`.
fn color(v: Option<&Value>) -> Rgba {
    match v {
        Some(Value::List(l)) => {
            let c = |i: usize| match l.get(i) {
                Some(Value::Number(n)) => n.clamp(0.0, 255.0) as u8,
                _ => 128,
            };
            [c(0), c(1), c(2), 255]
        }
        _ => [128, 128, 128, 255],
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
    // (show-title "text" seconds [player]): big text in the middle of the
    // screen, for everyone or one player.
    define(env, "show-title", |a| {
        let secs = num(a, 1, "the seconds").unwrap_or(3.0) as f32;
        guest::show_title(who(a, 2), &text(a, 0), secs);
        Ok(Value::Null)
    });
    // (set-hud player-or-nil ("line" "line" …)): the panel in the corner.
    define(env, "set-hud", |a| {
        let lines = match a.get(1) {
            Some(Value::List(l)) => l
                .iter()
                .map(|v| match v {
                    Value::Str(s) => guest::hud_line(s.clone()),
                    v => guest::hud_line(v.to_string()),
                })
                .collect(),
            _ => Vec::new(),
        };
        guest::set_hud(who(a, 0), lines);
        Ok(Value::Null)
    });
    // (sound "name") for everyone, or (sound "name" x y z) from a place.
    define(env, "sound", |a| {
        guest::sound(&text(a, 0), pos3(a, 1).ok());
        Ok(Value::Null)
    });
    // (confetti x y z), (hearts x y z), (poof x y z): a little celebration.
    for (name, kind) in [
        ("confetti", EffectKind::Confetti),
        ("hearts", EffectKind::Hearts),
        ("poof", EffectKind::Poof),
    ] {
        define(env, name, move |a| {
            guest::effect(kind, pos3(a, 0)?, [255, 255, 255, 255]);
            Ok(Value::Null)
        });
    }
    // (teleport player x y z)
    define(env, "teleport", |a| {
        guest::teleport(num(a, 0, "the player")? as u64, pos3(a, 1)?);
        Ok(Value::Null)
    });
    // (spawn-mob "core:pig" x y z)
    define(env, "spawn-mob", |a| {
        Ok(match guest::spawn_mob(&text(a, 0), pos3(a, 1)?) {
            Some(id) => Value::Number(id as f64),
            None => Value::Null,
        })
    });
    // (ground-height x z): the y of the top solid block there, or nil.
    define(env, "ground-height", |a| {
        Ok(guest::ground_height(int(a, 0, "x")?, int(a, 1, "z")?)
            .map_or(Value::Null, |h| Value::Number(h as f64)))
    });
    // (time-of-day): 0 is midnight, 0.5 noon. (set-time-of-day 0.25): dawn.
    define(env, "time-of-day", |_| {
        Ok(Value::Number(guest::time_of_day() as f64))
    });
    define(env, "set-time-of-day", |a| {
        guest::set_time_of_day(num(a, 0, "the time")? as f32);
        Ok(Value::Null)
    });
    // (register-block {:name "fun:glow" :display "Glow Block" :color [255 200 50]
    //                  :light 15 :glass false}): a new block, speckled in
    // that colour. Only while the script first runs.
    define(env, "register-block", |a| {
        let Some(Value::Dict(d)) = a.first() else {
            return Err(LispError::Runtime(
                "register-block takes a dict, like {:name \"fun:glow\" :display \"Glow\" :color [255 200 50]}"
                    .into(),
            ));
        };
        let name = match field(d, "name") {
            Some(Value::Str(s)) if s.contains(':') => s.clone(),
            _ => {
                return Err(LispError::Runtime(
                    "a block needs a :name like \"yourmod:thing\"".into(),
                ));
            }
        };
        let display = match field(d, "display") {
            Some(Value::Str(s)) => s.clone(),
            _ => name.clone(),
        };
        let glass = field(d, "glass").is_some_and(truthy);
        let mut rgba = speckled(color(field(d, "color")), name.len() as u32);
        if glass {
            // Clear in the middle, coloured at the edges.
            for (i, px) in rgba.chunks_mut(4).enumerate() {
                let (x, y) = (i % 16, i / 16);
                if (1..15).contains(&x) && (1..15).contains(&y) {
                    px[3] = 0;
                }
            }
        }
        guest::register_texture(&name, rgba);
        let light = match field(d, "light") {
            Some(Value::Number(n)) => n.clamp(0.0, 15.0) as u8,
            _ => 0,
        };
        guest::register_block(BlockSpec {
            render: if glass {
                Render::Cutout
            } else {
                Render::Opaque
            },
            light,
            ..BlockSpec::simple(&name, &display, &name)
        });
        Ok(Value::Str(name))
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

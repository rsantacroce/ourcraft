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

use std::cell::Cell;
use std::rc::Rc;

use eelisp::value::OrderedDict;
use eelisp::{Interpreter, LispError, Value};
use ourcraft_mod_api::guest::{self, Mod};
use ourcraft_mod_api::{
    BlockSpec, EffectKind, Event, EventReply, HostCall, HostReply, ItemSpec, RecipeSpec, Render, Rgba, speckled,
};

/// Most blocks one `fill` may change.
const MAX_FILL: i64 = 32 * 32 * 32;

#[derive(Default)]
struct Runner {
    /// The script, until `Init` runs it.
    script: Option<String>,
    it: Option<Interpreter>,
    /// Does the script have an `on-tick`? (Checked once: ticks are often.)
    ticks: bool,
    /// What went wrong while the script first ran, to tell players when
    /// they join (they can't read the server log).
    problem: Option<String>,
    /// Problems already shown in the chat (each is shown once).
    shown: Vec<String>,
}

thread_local! {
    /// The state of `random`'s number generator.
    static SEED: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) };
}

/// Stir something into the random numbers (the tick, the script).
fn stir(v: u64) {
    SEED.with(|s| s.set(s.get() ^ v.wrapping_mul(0x2545_F491_4F6C_DD1D)));
    next_random();
}

fn next_random() -> u64 {
    SEED.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
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
        if let Event::Tick { tick, .. } = event {
            stir(tick);
        }
        let Some(it) = &self.it else {
            return EventReply::Ok;
        };
        let n = |v: u64| Value::Number(v as f64);
        if let (Event::PlayerJoined { player, .. }, Some(p)) = (&event, &self.problem) {
            guest::chat(Some(*player), &format!("⚠ {p}"));
        }
        let (name, args) = match event {
            Event::Start => ("on-start", vec![]),
            Event::Tick { tick, .. } if self.ticks => ("on-tick", vec![n(tick)]),
            Event::PlayerJoined { player, name } => ("on-join", vec![n(player), Value::Str(name)]),
            Event::FishBite { player, rod, water } => {
                let mut args = vec![n(player), Value::Str(rod)];
                args.extend(water.map(|v| Value::Number(v as f64)));
                ("on-fish-bite", args)
            }
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
        let result = match it.call(name, &args) {
            // The script doesn't handle this event.
            None => None,
            Some(Ok(v)) => Some(v),
            Some(Err(e)) => {
                let text = format!("({name} …) failed: {}", says(&e));
                guest::log(&text);
                // Shown in the chat once, so whoever wrote it can see it.
                if !self.shown.contains(&text) {
                    guest::chat(None, &format!("⚠ {text}"));
                    self.shown.push(text);
                }
                None
            }
        };
        print_output(it);
        match (name, result, &args[..]) {
            ("on-chat", Some(v), _) if truthy(&v) => EventReply::Handled,
            ("on-block-change", Some(v), _) if is_deny(&v) => EventReply::Deny,
            // The catch: an item's name, or [name count].
            ("on-fish-bite", Some(v), [Value::Number(p), ..]) => match catch(&v) {
                Some((item, count)) if guest::give_item(*p as u64, &item, count) => {
                    EventReply::Handled
                }
                _ => EventReply::Ok,
            },
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
        add_game_functions(&it);
        stir(script.len() as u64 ^ script.bytes().fold(0u64, |h, b| h.wrapping_mul(31) ^ b as u64));
        if let Err(e) = it.eval_str(&script) {
            let text = format!("The script has a problem: {}", says(&e));
            guest::log(&text);
            self.problem = Some(text);
        }
        print_output(&it);
        self.ticks = it.eval_str("(fn? on-tick)").is_ok_and(|v| truthy(&v));
        self.it = Some(it);
    }
}

ourcraft_mod_api::export_mod!(Runner);

/// What `on-fish-bite` returned: `"fish:trout"` or `["fish:trout" 2]`.
fn catch(v: &Value) -> Option<(String, u16)> {
    match v {
        Value::Str(s) => Some((s.clone(), 1)),
        Value::List(l) => match (l.first(), l.get(1)) {
            (Some(Value::Str(s)), Some(Value::Number(n))) => Some((s.clone(), n.clamp(1.0, 64.0) as u16)),
            (Some(Value::Str(s)), None) => Some((s.clone(), 1)),
            _ => None,
        },
        _ => None,
    }
}

/// An error as a sentence (without EELisp's "Error: " in front).
fn says(e: &LispError) -> String {
    let text = e.to_string();
    text.strip_prefix("Error: ").unwrap_or(&text).to_owned()
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

/// A 16×16 picture drawn with letters: `:picture` is a list of rows
/// (`"..oo.."`) and `:colors` says which colour each letter is
/// (`{:o [255 140 0]}`). `.` and spaces are see-through. A smaller
/// picture is made bigger to fill the square (8 rows → each letter is
/// 2×2 pixels).
fn picture(d: &OrderedDict) -> Result<Option<Vec<u8>>, LispError> {
    let Some(Value::List(rows)) = field(d, "picture") else {
        return Ok(None);
    };
    let rows: Vec<Vec<char>> = rows
        .iter()
        .map(|r| match r {
            Value::Str(s) => s.chars().collect(),
            v => v.to_string().chars().collect(),
        })
        .collect();
    let h = rows.len();
    let w = rows.iter().map(Vec::len).max().unwrap_or(0);
    if h == 0 || h > 16 || w > 16 {
        return Err(LispError::Runtime(
            "a :picture has 1 to 16 rows, each at most 16 letters long".into(),
        ));
    }
    let mut colors: Vec<(char, Rgba)> = Vec::new();
    if let Some(Value::Dict(c)) = field(d, "colors") {
        for k in &c.keys {
            if let Some(ch) = k.trim_start_matches(':').chars().next() {
                colors.push((ch, color(c.map.get(k))));
            }
        }
    }
    let scale = (16 / w.max(h)).max(1);
    let (left, top) = ((16 - w * scale) / 2, (16 - h * scale) / 2);
    let mut rgba = vec![0u8; 16 * 16 * 4];
    for (y, row) in rows.iter().enumerate() {
        for (x, ch) in row.iter().enumerate() {
            if *ch == '.' || *ch == ' ' {
                continue;
            }
            let Some((_, c)) = colors.iter().find(|(k, _)| k == ch) else {
                return Err(LispError::Runtime(format!(
                    "the picture uses the letter {ch} but :colors doesn't say what colour it is"
                )));
            };
            for dy in 0..scale {
                for dx in 0..scale {
                    let (px, py) = (left + x * scale + dx, top + y * scale + dy);
                    let i = (py * 16 + px) * 4;
                    rgba[i..i + 4].copy_from_slice(c);
                }
            }
        }
    }
    Ok(Some(rgba))
}

/// `:name "yourmod:thing"` from a dict.
fn thing_name(d: &OrderedDict, what: &str) -> Result<String, LispError> {
    match field(d, "name") {
        Some(Value::Str(s)) if s.contains(':') => Ok(s.clone()),
        _ => Err(LispError::Runtime(format!(
            "{what} needs a :name like \"yourmod:thing\""
        ))),
    }
}

fn whole(d: &OrderedDict, key: &str, lo: f64, hi: f64, default: f64) -> f64 {
    match field(d, key) {
        Some(Value::Number(n)) => n.clamp(lo, hi),
        _ => default,
    }
}

/// Ask the game to add something; its "no" becomes the script's error
/// (a name that's taken, or registering after the script first ran).
fn registered(call: HostCall) -> Result<(), LispError> {
    match guest::host(&call) {
        HostReply::Error(e) => Err(LispError::Runtime(e)),
        _ => Ok(()),
    }
}

fn list(items: impl IntoIterator<Item = Value>) -> Value {
    Value::List(Rc::new(items.into_iter().collect()))
}

fn add_game_functions(it: &Interpreter) {
    it.define_fn("log", |a| {
        guest::log(&text(a, 0));
        Ok(Value::Null)
    });
    // (chat "text"): to everyone.
    it.define_fn("chat", |a| {
        guest::chat(None, &text(a, 0));
        Ok(Value::Null)
    });
    // (tell player "text"): to one player.
    it.define_fn("tell", |a| {
        guest::chat(Some(num(a, 0, "the player")? as u64), &text(a, 1));
        Ok(Value::Null)
    });
    // (show-title "text" seconds [player]): big text in the middle of the
    // screen, for everyone or one player.
    it.define_fn("show-title", |a| {
        let secs = num(a, 1, "the seconds").unwrap_or(3.0) as f32;
        guest::show_title(who(a, 2), &text(a, 0), secs);
        Ok(Value::Null)
    });
    // (set-hud player-or-nil ("line" "line" …)): the panel in the corner.
    it.define_fn("set-hud", |a| {
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
    it.define_fn("sound", |a| {
        guest::sound(&text(a, 0), pos3(a, 1).ok());
        Ok(Value::Null)
    });
    // (confetti x y z), (hearts x y z), (poof x y z): a little celebration.
    for (name, kind) in [
        ("confetti", EffectKind::Confetti),
        ("hearts", EffectKind::Hearts),
        ("poof", EffectKind::Poof),
    ] {
        it.define_fn(name, move |a| {
            guest::effect(kind, pos3(a, 0)?, [255, 255, 255, 255]);
            Ok(Value::Null)
        });
    }
    // (teleport player x y z)
    it.define_fn("teleport", |a| {
        guest::teleport(num(a, 0, "the player")? as u64, pos3(a, 1)?);
        Ok(Value::Null)
    });
    // (spawn-mob "core:pig" x y z)
    it.define_fn("spawn-mob", |a| {
        Ok(match guest::spawn_mob(&text(a, 0), pos3(a, 1)?) {
            Some(id) => Value::Number(id as f64),
            None => Value::Null,
        })
    });
    // (ground-height x z): the y of the top solid block there, or nil.
    it.define_fn("ground-height", |a| {
        Ok(guest::ground_height(int(a, 0, "x")?, int(a, 1, "z")?)
            .map_or(Value::Null, |h| Value::Number(h as f64)))
    });
    // (time-of-day): 0 is midnight, 0.5 noon. (set-time-of-day 0.25): dawn.
    it.define_fn("time-of-day", |_| {
        Ok(Value::Number(guest::time_of_day() as f64))
    });
    it.define_fn("set-time-of-day", |a| {
        guest::set_time_of_day(num(a, 0, "the time")? as f32);
        Ok(Value::Null)
    });
    // (register-block {:name "fun:glow" :display "Glow Block" :color [255 200 50]
    //                  :light 15 :glass false}): a new block, speckled in
    // that colour. Only while the script first runs.
    it.define_fn("register-block", |a| {
        let Some(Value::Dict(d)) = a.first() else {
            return Err(LispError::Runtime(
                "register-block takes a dict, like {:name \"fun:glow\" :display \"Glow\" :color [255 200 50]}"
                    .into(),
            ));
        };
        let name = thing_name(d, "a block")?;
        let display = match field(d, "display") {
            Some(Value::Str(s)) => s.clone(),
            _ => name.clone(),
        };
        let glass = field(d, "glass").is_some_and(truthy);
        let mut rgba = match picture(d)? {
            // See-through bits of a block's picture are its :color.
            Some(mut p) => {
                let under = color(field(d, "color"));
                for px in p.chunks_mut(4).filter(|px| px[3] == 0) {
                    px.copy_from_slice(&under);
                }
                p
            }
            None => speckled(color(field(d, "color")), name.len() as u32),
        };
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
        registered(HostCall::RegisterBlock(BlockSpec {
            render: if glass {
                Render::Cutout
            } else {
                Render::Opaque
            },
            light,
            ..BlockSpec::simple(&name, &display, &name)
        }))?;
        Ok(Value::Str(name))
    });
    // (players): a list of {:id :name :x :y :z}.
    it.define_fn("players", |_| {
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
    it.define_fn("player-pos", |a| {
        let id = num(a, 0, "the player")? as u64;
        let p = guest::players()
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| LispError::Runtime("no such player".into()))?;
        Ok(list(p.position.map(|v| Value::Number(v.floor() as f64))))
    });
    // (block-id "core:stone"): a block's id, or nil.
    it.define_fn("block-id", |a| {
        Ok(guest::block_id(&text(a, 0)).map_or(Value::Null, |id| Value::Number(id as f64)))
    });
    // (get-block x y z): the id of the block there.
    it.define_fn("get-block", |a| {
        let pos = [int(a, 0, "x")?, int(a, 1, "y")?, int(a, 2, "z")?];
        Ok(Value::Number(guest::get_block(pos) as f64))
    });
    // (set-block x y z block): true if it worked.
    it.define_fn("set-block", |a| {
        let pos = [int(a, 0, "x")?, int(a, 1, "y")?, int(a, 2, "z")?];
        Ok(Value::Bool(guest::try_set_block(pos, block(a, 3)?)))
    });
    // (fill x1 y1 z1 x2 y2 z2 block): a whole box at once (done here, in
    // Rust, so big builds stay fast). Returns how many blocks it changed.
    it.define_fn("fill", |a| {
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

    // ---- Chance ----
    // (random 6): a whole number from 0 to 5.
    it.define_fn("random", |a| {
        let n = num(a, 0, "how many numbers").unwrap_or(2.0).floor().max(1.0) as u64;
        Ok(Value::Number((next_random() % n) as f64))
    });
    // (chance 25): true 25 times out of 100.
    it.define_fn("chance", |a| {
        let pct = num(a, 0, "the percent")?;
        Ok(Value::Bool(((next_random() % 10_000) as f64) < pct * 100.0))
    });
    // (pick ["a" "b" "c"]): one of them, any one.
    it.define_fn("pick", |a| match a.first() {
        Some(Value::List(l)) if !l.is_empty() => {
            Ok(l[(next_random() % l.len() as u64) as usize].clone())
        }
        _ => Ok(Value::Null),
    });

    // ---- Items ----
    // (register-item {:name "fish:trout" :display "Trout" :color [120 160 200]
    //                 :picture [...] :colors {...} :food 2 :heals 0
    //                 :cooks-into "fish:cooked_trout" :stack 64 :fuel 0}):
    // something to carry. Only while the script first runs.
    it.define_fn("register-item", |a| {
        let Some(Value::Dict(d)) = a.first() else {
            return Err(LispError::Runtime(
                "register-item takes a dict, like {:name \"fish:trout\" :display \"Trout\" :color [120 160 200]}"
                    .into(),
            ));
        };
        let name = thing_name(d, "an item")?;
        let display = match field(d, "display") {
            Some(Value::Str(s)) => s.clone(),
            _ => name.clone(),
        };
        let rgba = match picture(d)? {
            Some(p) => p,
            None => speckled(color(field(d, "color")), name.len() as u32),
        };
        guest::register_texture(&name, rgba);
        let cooks_into = match field(d, "cooks-into") {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        };
        registered(HostCall::RegisterItem(ItemSpec {
            max_stack: whole(d, "stack", 1.0, 64.0, 64.0) as u16,
            food: whole(d, "food", 0.0, 20.0, 0.0) as u8,
            heals: whole(d, "heals", 0.0, 20.0, 0.0) as u8,
            fuel: whole(d, "fuel", 0.0, 600.0, 0.0) as f32,
            smelts_into: cooks_into,
            ..ItemSpec::simple(&name, &display, &name)
        }))?;
        Ok(Value::Str(name))
    });
    // (register-recipe {:pattern [" S" "S "] :key {:S "core:stick"} :result "fish:rod"})
    // or (register-recipe {:ingredients ["fish:trout" "core:bread"] :result "fish:sandwich"}).
    // Add :count 2 to make two. Only while the script first runs.
    it.define_fn("register-recipe", |a| {
        let Some(Value::Dict(d)) = a.first() else {
            return Err(LispError::Runtime(
                "register-recipe takes a dict with :result and :pattern or :ingredients".into(),
            ));
        };
        let result = match field(d, "result") {
            Some(Value::Str(s)) => s.clone(),
            _ => return Err(LispError::Runtime("a recipe needs a :result".into())),
        };
        let count = whole(d, "count", 1.0, 64.0, 1.0) as u16;
        let strings = |v: Option<&Value>| match v {
            Some(Value::List(l)) => l.iter().map(|v| match v {
                Value::Str(s) => s.clone(),
                v => v.to_string(),
            }).collect(),
            _ => Vec::new(),
        };
        let spec = if let Some(Value::Dict(k)) = field(d, "key") {
            let pattern: Vec<String> = strings(field(d, "pattern"));
            if pattern.is_empty() || pattern.len() > 3 || pattern.iter().any(|r| r.chars().count() > 3) {
                return Err(LispError::Runtime(
                    "a recipe's :pattern has 1 to 3 rows of up to 3 letters".into(),
                ));
            }
            let key = k
                .keys
                .iter()
                .filter_map(|name| {
                    let ch = name.trim_start_matches(':').chars().next()?;
                    match k.map.get(name) {
                        Some(Value::Str(item)) => Some((ch, item.clone())),
                        _ => None,
                    }
                })
                .collect();
            RecipeSpec::Shaped { pattern, key, result: result.clone(), count }
        } else {
            let ingredients = strings(field(d, "ingredients"));
            if ingredients.is_empty() || ingredients.len() > 9 {
                return Err(LispError::Runtime(
                    "a recipe needs :ingredients (1 to 9) or a :pattern and :key".into(),
                ));
            }
            RecipeSpec::Shapeless { ingredients, result: result.clone(), count }
        };
        registered(HostCall::RegisterRecipe(spec))?;
        Ok(Value::Str(result))
    });
    // (give-item player "fish:trout" 3): count is 1 if you leave it out.
    it.define_fn("give-item", |a| {
        let count = num(a, 2, "how many").unwrap_or(1.0).clamp(1.0, 2304.0) as u16;
        Ok(Value::Bool(guest::give_item(num(a, 0, "the player")? as u64, &text(a, 1), count)))
    });
    // (take-item player "fish:trout" 3): takes up to 3; returns how many it took.
    it.define_fn("take-item", |a| {
        let count = num(a, 2, "how many").unwrap_or(1.0).clamp(0.0, 2304.0) as u16;
        Ok(Value::Number(
            guest::take_item(num(a, 0, "the player")? as u64, &text(a, 1), count) as f64,
        ))
    });
    // (count-item player "fish:trout"): how many they have.
    it.define_fn("count-item", |a| {
        Ok(Value::Number(
            guest::count_item(num(a, 0, "the player")? as u64, &text(a, 1)) as f64,
        ))
    });
    // (world-spawn): where new players appear, as [x y z].
    it.define_fn("world-spawn", |_| {
        Ok(guest::world_spawn().map_or(Value::Null, |p| list(p.map(|v| Value::Number(v as f64)))))
    });
}

//! Runs mods safely.
//!
//! Each mod is a WebAssembly module, run by [wasmtime](https://wasmtime.dev)
//! in its own sandbox:
//! - **No outside access.** A module may only import one function,
//!   `ourcraft.call`. Anything else (files, network, clock…) simply isn't
//!   provided, so a mod that asks for it fails to load.
//! - **Time limit.** Each event gets a budget of "fuel" (roughly, wasm
//!   instructions). A mod stuck in an endless loop runs out and is stopped;
//!   after a few failures it's switched off.
//! - **Memory limit.** Each mod gets at most [`MAX_MEMORY`] bytes.
//!
//! The game side implements [`Host`] to answer the mod's calls.

use ourcraft_mod_api::{self as api, API_VERSION, Event, EventReply, HostCall, HostReply};
use wasmtime::{
    Caller, Config, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

/// Fuel per event (about this many wasm instructions).
pub const FUEL_PER_EVENT: u64 = 50_000_000;
/// Most memory a mod may use.
pub const MAX_MEMORY: usize = 64 * 1024 * 1024;
/// A mod is switched off after this many crashes or timeouts.
pub const MAX_FAILURES: u32 = 3;

/// The game's side: answers calls from mods.
pub trait Host: Send + 'static {
    /// Handle one call from the mod named `mod_name`.
    fn handle(&mut self, mod_name: &str, call: HostCall) -> HostReply;
}

/// Per-mod data wasmtime keeps for us.
struct Ctx<H> {
    name: String,
    /// The host, lent to the mod for the duration of one event.
    host: Option<H>,
    limits: StoreLimits,
}

pub struct LoadedMod<H: Host> {
    pub name: String,
    store: Store<Ctx<H>>,
    memory: Memory,
    alloc: TypedFunc<u32, u32>,
    event: TypedFunc<(u32, u32), u64>,
    pub failures: u32,
    pub enabled: bool,
}

/// All loaded mods.
pub struct ModRuntime<H: Host> {
    engine: Engine,
    linker: Linker<Ctx<H>>,
    pub mods: Vec<LoadedMod<H>>,
}

impl<H: Host> ModRuntime<H> {
    pub fn new() -> Result<Self, String> {
        let mut config = Config::new();
        config.consume_fuel(true);
        // iOS forbids generating machine code at runtime: use the Pulley
        // interpreter there.
        #[cfg(target_os = "ios")]
        config.target("pulley64").map_err(|e| e.to_string())?;
        let engine = Engine::new(&config).map_err(|e| e.to_string())?;
        let mut linker = Linker::new(&engine);
        linker
            .func_wrap("ourcraft", "call", host_call::<H>)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            engine,
            linker,
            mods: Vec::new(),
        })
    }

    /// Load a mod from its `.wasm` bytes.
    pub fn load(&mut self, name: &str, wasm: &[u8]) -> Result<(), String> {
        let module = Module::new(&self.engine, wasm).map_err(|e| format!("invalid wasm: {e}"))?;
        for import in module.imports() {
            if (import.module(), import.name()) != ("ourcraft", "call") {
                return Err(format!(
                    "it needs `{}.{}`, which mods aren't allowed to use",
                    import.module(),
                    import.name()
                ));
            }
        }
        let ctx = Ctx {
            name: name.to_owned(),
            host: None,
            limits: StoreLimitsBuilder::new()
                .memory_size(MAX_MEMORY)
                .instances(1)
                .build(),
        };
        let mut store = Store::new(&self.engine, ctx);
        store.limiter(|c| &mut c.limits);
        store.set_fuel(FUEL_PER_EVENT).map_err(|e| e.to_string())?;
        let instance = self
            .linker
            .instantiate(&mut store, &module)
            .map_err(|e| format!("couldn't start: {e}"))?;
        let get = |store: &mut Store<Ctx<H>>, export: &str| {
            instance
                .get_typed_func::<(), u32>(&mut *store, export)
                .map_err(|_| format!("missing export `{export}`"))
        };
        let version = get(&mut store, "ourcraft_api_version")?
            .call(&mut store, ())
            .map_err(|e| e.to_string())?;
        if version != API_VERSION {
            return Err(format!(
                "it was made for mod API version {version}, but this game uses version {API_VERSION}"
            ));
        }
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or("missing exported memory")?;
        let alloc = instance
            .get_typed_func::<u32, u32>(&mut store, "ourcraft_alloc")
            .map_err(|_| "missing export `ourcraft_alloc`")?;
        let event = instance
            .get_typed_func::<(u32, u32), u64>(&mut store, "ourcraft_event")
            .map_err(|_| "missing export `ourcraft_event`")?;
        self.mods.push(LoadedMod {
            name: name.to_owned(),
            store,
            memory,
            alloc,
            event,
            failures: 0,
            enabled: true,
        });
        Ok(())
    }

    /// Send an event to every enabled mod, in load order. The host is lent
    /// to each mod in turn and handed back at the end. Returns each mod's
    /// reply (`Ok` for mods that failed or are disabled).
    pub fn dispatch(&mut self, mut host: H, event: &Event) -> (H, Vec<EventReply>) {
        let mut replies = Vec::with_capacity(self.mods.len());
        for m in &mut self.mods {
            let (h, reply) = m.dispatch(host, event);
            host = h;
            replies.push(reply);
        }
        (host, replies)
    }

    /// Send an event to mods one at a time, stopping at the first reply
    /// that isn't `Ok` (used for "can I?" questions and chat commands).
    pub fn dispatch_until_handled(&mut self, mut host: H, event: &Event) -> (H, EventReply) {
        for m in &mut self.mods {
            let (h, reply) = m.dispatch(host, event);
            host = h;
            if reply != EventReply::Ok {
                return (host, reply);
            }
        }
        (host, EventReply::Ok)
    }

    pub fn enabled_count(&self) -> usize {
        self.mods.iter().filter(|m| m.enabled).count()
    }
}

impl<H: Host> LoadedMod<H> {
    fn dispatch(&mut self, host: H, event: &Event) -> (H, EventReply) {
        if !self.enabled {
            return (host, EventReply::Ok);
        }
        self.store.data_mut().host = Some(host);
        let result = self.call_event(event);
        let host = self
            .store
            .data_mut()
            .host
            .take()
            .expect("host is always returned");
        match result {
            Ok(reply) => (host, reply),
            Err(e) => {
                self.failures += 1;
                log::error!("Mod '{}' failed: {e}", self.name);
                if self.failures >= MAX_FAILURES {
                    self.enabled = false;
                    log::error!(
                        "Mod '{}' switched off after {} failures",
                        self.name,
                        self.failures
                    );
                }
                (host, EventReply::Ok)
            }
        }
    }

    fn call_event(&mut self, event: &Event) -> Result<EventReply, String> {
        let store = &mut self.store;
        store.set_fuel(FUEL_PER_EVENT).map_err(|e| e.to_string())?;
        let bytes = api::encode(event);
        let ptr = self
            .alloc
            .call(&mut *store, bytes.len() as u32)
            .map_err(|e| e.to_string())?;
        self.memory
            .write(&mut *store, ptr as usize, &bytes)
            .map_err(|e| e.to_string())?;
        let packed = self
            .event
            .call(&mut *store, (ptr, bytes.len() as u32))
            .map_err(|e| {
                if store.get_fuel().is_ok_and(|f| f == 0) {
                    "took too long (out of fuel)".to_owned()
                } else {
                    e.to_string()
                }
            })?;
        let (rptr, rlen) = ((packed >> 32) as usize, (packed & 0xFFFF_FFFF) as usize);
        let data = self.memory.data(&*store);
        let reply_bytes = data
            .get(rptr..rptr.saturating_add(rlen))
            .ok_or("reply outside memory")?;
        Ok(api::decode(reply_bytes).unwrap_or(EventReply::Ok))
    }
}

/// `ourcraft.call(ptr, len) -> (reply_ptr << 32) | reply_len`, the one
/// function mods can import.
fn host_call<H: Host>(mut caller: Caller<'_, Ctx<H>>, ptr: u32, len: u32) -> wasmtime::Result<u64> {
    let memory = caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| wasmtime::Error::msg("mod has no memory"))?;
    let bytes = memory
        .data(&caller)
        .get(ptr as usize..(ptr as usize).saturating_add(len as usize))
        .ok_or_else(|| wasmtime::Error::msg("call outside memory"))?
        .to_vec();
    let reply = match api::decode::<HostCall>(&bytes) {
        Some(call) => {
            let ctx = caller.data_mut();
            match ctx.host.as_mut() {
                Some(host) => host.handle(&ctx.name, call),
                None => HostReply::Error("no host".into()),
            }
        }
        None => HostReply::Error("unreadable call (wrong API version?)".into()),
    };
    let out = api::encode(&reply);
    let alloc = caller
        .get_export("ourcraft_alloc")
        .and_then(|e| e.into_func())
        .ok_or_else(|| wasmtime::Error::msg("mod has no ourcraft_alloc"))?
        .typed::<u32, u32>(&caller)?;
    let out_ptr = alloc.call(&mut caller, out.len() as u32)?;
    memory.write(&mut caller, out_ptr as usize, &out)?;
    Ok(((out_ptr as u64) << 32) | out.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHost(Vec<String>);

    impl Host for TestHost {
        fn handle(&mut self, _mod_name: &str, call: HostCall) -> HostReply {
            if let HostCall::Log(s) = call {
                self.0.push(s);
            }
            HostReply::Ok
        }
    }

    fn wasm(src: &str) -> Vec<u8> {
        wat::parse_str(src).unwrap()
    }

    const EXPORTS: &str = r#"
        (memory (export "memory") 1)
        (func (export "ourcraft_alloc") (param i32) (result i32) i32.const 1024)
    "#;

    #[test]
    fn rejects_mods_that_want_outside_access() {
        let mut rt = ModRuntime::<TestHost>::new().unwrap();
        let src = format!(
            r#"(module
                (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))
                {EXPORTS}
                (func (export "ourcraft_api_version") (result i32) i32.const 1)
                (func (export "ourcraft_event") (param i32 i32) (result i64) i64.const 0))"#
        );
        let err = rt.load("sneaky", &wasm(&src)).unwrap_err();
        assert!(err.contains("fd_write"), "{err}");
    }

    #[test]
    fn rejects_wrong_api_version() {
        let mut rt = ModRuntime::<TestHost>::new().unwrap();
        let src = format!(
            r#"(module {EXPORTS}
                (func (export "ourcraft_api_version") (result i32) i32.const 999)
                (func (export "ourcraft_event") (param i32 i32) (result i64) i64.const 0))"#
        );
        let err = rt.load("old", &wasm(&src)).unwrap_err();
        assert!(err.contains("999"), "{err}");
    }

    #[test]
    fn endless_loops_are_stopped_and_mod_switched_off() {
        let mut rt = ModRuntime::<TestHost>::new().unwrap();
        let src = format!(
            r#"(module {EXPORTS}
                (func (export "ourcraft_api_version") (result i32) i32.const {API_VERSION})
                (func (export "ourcraft_event") (param i32 i32) (result i64)
                    (loop (br 0))
                    i64.const 0))"#
        );
        rt.load("loopy", &wasm(&src)).unwrap();
        let mut host = TestHost(vec![]);
        for _ in 0..MAX_FAILURES {
            let (h, replies) = rt.dispatch(host, &Event::Start);
            host = h;
            assert_eq!(replies, vec![EventReply::Ok]);
        }
        assert!(!rt.mods[0].enabled);
        assert_eq!(rt.enabled_count(), 0);
    }

    #[test]
    fn runs_the_example_mod_if_built() {
        // Built by `mods/build.sh`; skipped if it hasn't been run.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../mods/dist/paint_blocks.wasm"
        );
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipping: {path} not built");
            return;
        };
        let mut rt = ModRuntime::<TestHost>::new().unwrap();
        rt.load("paint_blocks", &bytes).unwrap();
        let (host, _) = rt.dispatch(
            TestHost(vec![]),
            &Event::Init {
                api_version: API_VERSION,
                game_mode: None,
            },
        );
        assert!(rt.mods[0].enabled);
        assert!(
            host.0.iter().any(|l| l.contains("Paint Blocks")),
            "{:?}",
            host.0
        );
    }
}

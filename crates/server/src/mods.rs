//! Mods on the server: loading them, answering their calls, and passing
//! them game events.
//!
//! Each tick, [`run_mods`] takes the queued events (players joining, block
//! changes waiting for approval, chat), lends the world to the mods through
//! a [`ServerHost`], and afterwards carries out what the mods asked for
//! (messages to send, players to move…).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use ourcraft_common::block::{BlockId, BlockRegistry};
use ourcraft_common::protocol::{Channel, ContentPack, ServerMessage};
use ourcraft_common::world::{ColumnPos, WORLD_HEIGHT};
use ourcraft_mod_api::{self as api, API_VERSION, Event, EventReply, HostCall, HostReply};
use ourcraft_mod_host::{Host, ModRuntime};
use renet::ClientId;

use crate::entities::{Entities, ServerEntity};
use crate::net::Network;
use crate::players::ConnectedPlayer;
use crate::world::{ColumnCache, OpenWorld, Registry, ServerWorld, set_block};

include!(concat!(env!("OUT_DIR"), "/bundled_mods.rs"));

/// Send mods a tick every this many server ticks (60 / 3 = 20 per second).
const TICK_EVERY: u64 = 3;

/// The loaded mods.
#[derive(Resource)]
pub struct Mods(pub Mutex<ModRuntime<ServerHost>>);

/// Content added by mods, sent to clients when they join.
#[derive(Resource, Clone)]
pub struct Content(pub ContentPack);

/// A player's block change, waiting for the mods' approval.
#[derive(Debug, Clone)]
pub struct PendingEdit {
    pub client: ClientId,
    pub pos: IVec3,
    pub old: BlockId,
    pub new: BlockId,
    /// The item the player was holding (decides what a broken block drops).
    pub held: ourcraft_common::item::ItemId,
}

/// Player edits that went through this tick (the survival module drops
/// items for broken blocks).
#[derive(Resource, Default)]
pub struct AppliedEdits(pub Vec<PendingEdit>);

/// Things for the mods to look at this tick.
#[derive(Resource, Default)]
pub struct ModQueue {
    pub events: Vec<Event>,
    pub edits: Vec<PendingEdit>,
    /// (client, player name, text)
    pub chats: Vec<(ClientId, String, String)>,
}

/// Something a mod asked for that needs other parts of the server.
#[derive(Debug, Clone)]
pub enum ModCommand {
    BlockChanged(IVec3, BlockId),
    /// Tell one client a block is really this (its edit was refused).
    Revert(ClientId, IVec3, BlockId),
    Chat {
        to: Option<ClientId>,
        from: Option<String>,
        text: String,
    },
    Teleport(ClientId, Vec3),
    TimeChanged,
    Rules(ClientId, api::PlayerRules),
    Health(ClientId, f32, f32),
    Hud(Option<ClientId>, Vec<api::HudLine>),
    Title(Option<ClientId>, String, f32),
    Tools(ClientId, Vec<String>),
    Broadcast(Box<ServerMessage>),
}

enum Phase {
    /// Mods are registering content.
    Init,
    /// The game is running.
    Running,
}

/// What mods talk to. During a tick it temporarily owns the world.
pub struct ServerHost {
    phase: Phase,
    pub registry: BlockRegistry,
    pub textures: Vec<(String, Vec<u8>)>,
    pub models: Vec<api::ModelSpec>,
    pub tools: Vec<api::ToolSpec>,
    pub recipes: Vec<ourcraft_common::recipes::Recipe>,
    pub mobs: Vec<api::MobSpec>,
    pub items: Vec<ourcraft_common::item::ItemDef>,
    world: Option<ServerWorld>,
    cache: Option<ColumnCache>,
    open: Option<OpenWorld>,
    entities: Option<Entities>,
    players: Vec<api::PlayerInfo>,
    pub out: Vec<ModCommand>,
}

impl ServerHost {
    fn init(registry: BlockRegistry) -> Self {
        Self {
            phase: Phase::Init,
            registry,
            textures: Vec::new(),
            models: Vec::new(),
            tools: Vec::new(),
            recipes: Vec::new(),
            mobs: Vec::new(),
            items: Vec::new(),
            world: None,
            cache: None,
            open: None,
            entities: None,
            players: Vec::new(),
            out: Vec::new(),
        }
    }

    /// The first block or player along a ray.
    fn raycast(
        &self,
        origin: Vec3,
        dir: Vec3,
        max: f32,
        hit_players: bool,
        ignore: Option<u64>,
    ) -> Option<api::Hit> {
        let dir = dir.try_normalize()?;
        let world = self.world.as_ref()?;
        let block = ourcraft_common::raycast::raycast(&world.0, &self.registry, origin, dir, max);
        let block_dist = block.map(|h| {
            ourcraft_common::physics::Aabb::block(h.block)
                .ray_hit(origin, dir, max)
                .unwrap_or(0.0)
        });
        let mut best: Option<(f32, api::Hit)> = block.zip(block_dist).map(|(h, d)| {
            (
                d,
                api::Hit::Block {
                    pos: h.block.to_array(),
                    normal: h.normal.to_array(),
                },
            )
        });
        if hit_players {
            for p in &self.players {
                if Some(p.id) == ignore {
                    continue;
                }
                let aabb = ourcraft_common::physics::Aabb::player(Vec3::from_array(p.position));
                if let Some(d) = aabb.ray_hit(origin, dir, max)
                    && best.as_ref().is_none_or(|(bd, _)| d < *bd)
                {
                    let at = (origin + dir * d).to_array();
                    best = Some((d, api::Hit::Player { id: p.id, at }));
                }
            }
        }
        best.map(|(_, h)| h)
    }

    /// Change a block on behalf of a mod or an approved player edit.
    fn apply_set_block(&mut self, pos: IVec3, block: BlockId) -> bool {
        let (Some(world), Some(cache), Some(open)) =
            (self.world.as_mut(), self.cache.as_mut(), self.open.as_ref())
        else {
            return false;
        };
        if set_block(world, cache, open, pos, block).is_some() {
            self.out.push(ModCommand::BlockChanged(pos, block));
            true
        } else {
            false
        }
    }
}

fn error(text: &str) -> HostReply {
    HostReply::Error(text.into())
}

/// Look direction from yaw and pitch (matches the client's camera).
pub fn look_vector(yaw: f32, pitch: f32) -> [f32; 3] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    [-sy * cp, sp, -cy * cp]
}

impl Host for ServerHost {
    fn handle(&mut self, mod_name: &str, call: HostCall) -> HostReply {
        let init = matches!(self.phase, Phase::Init);
        match call {
            HostCall::Log(text) => {
                info!("[{mod_name}] {text}");
                HostReply::Ok
            }

            // ---- Content (Init only) ----
            HostCall::RegisterBlock(spec) if init => match self.registry.register_spec(&spec) {
                Some(id) => HostReply::Block(id.0),
                None => error("a block with that name already exists"),
            },
            HostCall::RegisterTexture(t) if init => {
                if t.rgba.len() != api::TEXTURE_SIZE * api::TEXTURE_SIZE * 4 {
                    return error("textures must be 16×16 RGBA (1024 bytes)");
                }
                self.textures.push((t.name, t.rgba));
                HostReply::Ok
            }
            HostCall::RegisterModel(m) if init => {
                self.models.push(m);
                HostReply::Ok
            }
            HostCall::RegisterTool(t) if init => {
                self.tools.push(t);
                HostReply::Ok
            }
            HostCall::RegisterRecipe(r) if init => {
                self.recipes.push(r.into());
                HostReply::Ok
            }
            HostCall::RegisterItem(i) if init => {
                let Some(rgba) = self
                    .textures
                    .iter()
                    .find(|t| t.0 == i.texture)
                    .map(|t| t.1.clone())
                else {
                    return error("register the item's texture first");
                };
                if !i.name.contains(':') || i.name.starts_with("core:") {
                    return error("item names look like \"yourmod:thing\"");
                }
                if self.registry.by_name(&i.name).is_some()
                    || self.items.iter().any(|d| d.name == i.name)
                {
                    return error("there's already something with that name");
                }
                // The client draws an item from the texture with its name.
                self.textures.push((i.name.clone(), rgba));
                self.items
                    .push(ourcraft_common::item::ItemDef::from_spec(&i));
                HostReply::Ok
            }
            HostCall::RegisterMob(m) if init => {
                if !self.models.iter().any(|x| x.name == m.model) {
                    return error("register the mob's model first");
                }
                self.mobs.push(m);
                HostReply::Ok
            }
            HostCall::RegisterBlock(_)
            | HostCall::RegisterTexture(_)
            | HostCall::RegisterModel(_)
            | HostCall::RegisterTool(_)
            | HostCall::RegisterRecipe(_)
            | HostCall::RegisterItem(_)
            | HostCall::RegisterMob(_) => error("content can only be registered during Init"),
            HostCall::BlockId(name) => match self.registry.by_name(&name) {
                Some(id) => HostReply::Block(id.0),
                None => error("no such block"),
            },
            _ if init => error("the world isn't running yet (wait for Event::Start)"),

            // ---- World ----
            HostCall::GetBlock(p) => {
                let id = self
                    .world
                    .as_ref()
                    .and_then(|w| w.get_block(p[0], p[1], p[2]))
                    .unwrap_or(BlockId::AIR);
                HostReply::Block(id.0)
            }
            HostCall::SetBlock { pos, block } => {
                if block as usize >= self.registry.len() {
                    return error("no such block id");
                }
                if self.apply_set_block(IVec3::from_array(pos), BlockId(block)) {
                    HostReply::Ok
                } else {
                    error("that part of the world isn't loaded")
                }
            }
            HostCall::GroundHeight { x, z } => {
                let (Some(world), Some(open)) = (self.world.as_ref(), self.open.as_ref()) else {
                    return HostReply::Height(None);
                };
                let col = ColumnPos::from_block(x, z);
                if world.has_column(col) {
                    let h = (0..WORLD_HEIGHT)
                        .rev()
                        .find(|&y| self.registry.is_solid(world.block(x, y, z)));
                    HostReply::Height(h)
                } else {
                    // Not loaded: ask the terrain generator.
                    HostReply::Height(Some(open.generator.surface(x, z).0))
                }
            }
            HostCall::GetTimeOfDay => {
                HostReply::Time(self.open.as_ref().map_or(0.0, |o| o.meta.time_of_day))
            }
            HostCall::SetTimeOfDay(t) => {
                if let Some(open) = self.open.as_mut() {
                    open.meta.time_of_day = t.rem_euclid(1.0);
                    self.out.push(ModCommand::TimeChanged);
                }
                HostReply::Ok
            }

            // ---- Players ----
            HostCall::GetPlayers => HostReply::Players(self.players.clone()),
            HostCall::Chat { to, text } => {
                self.out.push(ModCommand::Chat {
                    to,
                    from: None,
                    text,
                });
                HostReply::Ok
            }
            HostCall::Teleport { player, position } => {
                if position.iter().all(|v| v.is_finite()) {
                    self.out
                        .push(ModCommand::Teleport(player, Vec3::from_array(position)));
                }
                HostReply::Ok
            }
            HostCall::SetRules { player, rules } => {
                self.out.push(ModCommand::Rules(player, rules));
                HostReply::Ok
            }
            HostCall::SetHealth {
                player,
                health,
                max,
            } => {
                self.out.push(ModCommand::Health(player, health, max));
                HostReply::Ok
            }
            HostCall::SetHud { player, lines } => {
                self.out.push(ModCommand::Hud(player, lines));
                HostReply::Ok
            }
            HostCall::ShowTitle {
                player,
                text,
                seconds,
            } => {
                self.out.push(ModCommand::Title(player, text, seconds));
                HostReply::Ok
            }
            HostCall::SetTools { player, tools } => {
                self.out.push(ModCommand::Tools(player, tools));
                HostReply::Ok
            }
            HostCall::Raycast {
                origin,
                direction,
                max_distance,
                hit_players,
                ignore_player,
            } => HostReply::Ray(self.raycast(
                Vec3::from_array(origin),
                Vec3::from_array(direction),
                max_distance.clamp(0.0, 256.0),
                hit_players,
                ignore_player,
            )),

            // ---- Entities and effects ----
            HostCall::SpawnEntity(spec) => {
                let Some(entities) = self.entities.as_mut() else {
                    return error("no entities");
                };
                if entities.list.len() >= 2000 {
                    return error("too many entities");
                }
                let finite = spec
                    .position
                    .iter()
                    .chain(&spec.velocity)
                    .all(|v| v.is_finite());
                if !finite {
                    return error("bad position or velocity");
                }
                let id = entities.spawn(ServerEntity {
                    model: spec.model,
                    position: Vec3::from_array(spec.position),
                    velocity: Vec3::from_array(spec.velocity),
                    motion: spec.motion,
                    lifetime: spec.lifetime.max(0.0),
                    age: 0.0,
                    owner: spec.owner,
                    moved: false,
                    item: None,
                    yaw: 0.0,
                });
                HostReply::Entity(id)
            }
            HostCall::MoveEntity { id, position } => {
                if let Some(e) = self.entities.as_mut().and_then(|es| es.list.get_mut(&id))
                    && position.iter().all(|v| v.is_finite())
                {
                    e.position = Vec3::from_array(position);
                    e.moved = true;
                }
                HostReply::Ok
            }
            HostCall::DespawnEntity(id) => {
                if let Some(es) = self.entities.as_mut() {
                    es.remove(id);
                }
                HostReply::Ok
            }
            HostCall::Effect {
                kind,
                position,
                color,
            } => {
                self.out
                    .push(ModCommand::Broadcast(Box::new(ServerMessage::Effect {
                        kind,
                        position,
                        color,
                    })));
                HostReply::Ok
            }
            HostCall::SpawnMob { name, position } => {
                let known = self
                    .open
                    .as_ref()
                    .is_some_and(|o| o.mobs.iter().any(|m| m.name == name));
                let (true, Some(entities)) = (known, self.entities.as_mut()) else {
                    return error("no such mob (or the world isn't running)");
                };
                let id = crate::mobs::spawn_mob(entities, &name, Vec3::from_array(position), false);
                HostReply::Entity(id)
            }
            HostCall::WorldSpawn => match self.open.as_ref() {
                Some(o) => HostReply::Position(o.spawn.to_array()),
                None => error("the world isn't open"),
            },
            HostCall::Sound { name, at } => {
                self.out
                    .push(ModCommand::Broadcast(Box::new(ServerMessage::Sound {
                        name,
                        at,
                    })));
                HostReply::Ok
            }
        }
    }
}

/// Load bundled mods and any in `mods_dir`, give them `Init`, and return the
/// runtime plus the content they registered.
pub fn load_mods(
    mods_dir: Option<&Path>,
    use_bundled: bool,
    game_mode: Option<String>,
) -> (Mods, BlockRegistry, ContentPack) {
    let mut runtime = match ModRuntime::new() {
        Ok(r) => r,
        Err(e) => {
            error!("Mods disabled: couldn't start the mod runtime: {e}");
            let registry = BlockRegistry::with_builtin_blocks();
            let content = ContentPack {
                blocks: registry.defs().to_vec(),
                items: ourcraft_common::item::ItemRegistry::build(&registry, &[])
                    .defs()
                    .to_vec(),
                recipes: ourcraft_common::recipes::builtin_recipes(),
                mobs: ourcraft_common::mobs::builtin_mobs(),
                ..default()
            };
            let rt = ModRuntime::new().expect("runtime");
            return (Mods(Mutex::new(rt)), registry, content);
        }
    };

    // Files in the mods folder override bundled mods with the same name.
    let mut sources: HashMap<String, Vec<u8>> = HashMap::new();
    if use_bundled {
        for (name, bytes) in BUNDLED_MODS {
            sources.insert((*name).to_owned(), bytes.to_vec());
        }
    }
    if let Some(dir) = mods_dir
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "wasm")
                && let (Some(stem), Ok(bytes)) = (path.file_stem(), std::fs::read(&path))
            {
                sources.insert(stem.to_string_lossy().into_owned(), bytes);
            }
        }
    }
    let mut names: Vec<String> = sources.keys().cloned().collect();
    names.sort();
    for name in names {
        match runtime.load(&name, &sources[&name]) {
            Ok(()) => info!("Loaded mod '{name}'"),
            Err(e) => error!("Mod '{name}' wasn't loaded: {e}"),
        }
    }

    let host = ServerHost::init(BlockRegistry::with_builtin_blocks());
    let (host, _) = runtime.dispatch(
        host,
        &Event::Init {
            api_version: API_VERSION,
            game_mode,
        },
    );
    let items = ourcraft_common::item::ItemRegistry::build(&host.registry, &host.items);
    let mut recipes = ourcraft_common::recipes::builtin_recipes();
    recipes.extend(host.recipes);
    let mut mobs = ourcraft_common::mobs::builtin_mobs();
    for spec in &host.mobs {
        if let Some(model) = host.models.iter().find(|m| m.name == spec.model) {
            mobs.push(crate::mobs::mob_from_spec(spec, model));
        }
    }
    let content = ContentPack {
        blocks: host.registry.defs().to_vec(),
        textures: host.textures,
        models: host.models,
        tools: host.tools,
        items: items.defs().to_vec(),
        recipes,
        mobs,
    };
    (Mods(Mutex::new(runtime)), host.registry, content)
}

/// Run this tick's events through the mods, then carry out the results.
pub fn run_mods(world: &mut World, mut tick: Local<u64>) {
    *tick += 1;
    let send_tick = (*tick).is_multiple_of(TICK_EVERY);
    let queue = std::mem::take(&mut *world.resource_mut::<ModQueue>());
    if queue.events.is_empty() && queue.edits.is_empty() && queue.chats.is_empty() && !send_tick {
        return;
    }

    // Snapshot the players for the mods.
    let mut players_q = world.query::<&ConnectedPlayer>();
    let players: Vec<api::PlayerInfo> = players_q
        .iter(world)
        .filter_map(|p| {
            Some(api::PlayerInfo {
                id: p.client,
                name: p.name.clone()?,
                position: p.state.position,
                velocity: p.state.velocity,
                look: look_vector(p.state.yaw, p.state.pitch),
                on_ground: p.state.on_ground,
            })
        })
        .collect();

    // Lend the world to the mods.
    let registry = world.resource::<Registry>().0.clone();
    let mut host = ServerHost {
        phase: Phase::Running,
        registry: (*registry).clone(),
        textures: Vec::new(),
        models: Vec::new(),
        tools: Vec::new(),
        recipes: Vec::new(),
        mobs: Vec::new(),
        items: Vec::new(),
        world: world.remove_resource::<ServerWorld>(),
        cache: world.remove_resource::<ColumnCache>(),
        open: world.remove_resource::<OpenWorld>(),
        entities: world.remove_resource::<Entities>(),
        players,
        out: Vec::new(),
    };
    let mods = world.remove_resource::<Mods>().expect("mods resource");
    let mut applied = Vec::new();
    {
        let mut rt = mods.0.lock().unwrap();
        for event in &queue.events {
            host = rt.dispatch(host, event).0;
        }
        for edit in &queue.edits {
            let event = Event::BlockChange {
                player: edit.client,
                pos: edit.pos.to_array(),
                old: edit.old.0,
                new: edit.new.0,
            };
            let (h, reply) = rt.dispatch_until_handled(host, &event);
            host = h;
            if reply == EventReply::Deny || !host.apply_set_block(edit.pos, edit.new) {
                host.out
                    .push(ModCommand::Revert(edit.client, edit.pos, edit.old));
            } else {
                applied.push(edit.clone());
            }
        }
        for (client, name, text) in &queue.chats {
            let event = Event::Chat {
                player: *client,
                text: text.clone(),
            };
            let (h, reply) = rt.dispatch_until_handled(host, &event);
            host = h;
            if reply != EventReply::Handled && text.starts_with('/') {
                host.out.push(ModCommand::Chat {
                    to: Some(*client),
                    from: None,
                    text: format!(
                        "Unknown command {}. Try /help",
                        text.split(' ').next().unwrap_or("")
                    ),
                });
            } else if reply != EventReply::Handled {
                info!("<{name}> {text}");
                host.out.push(ModCommand::Chat {
                    to: None,
                    from: Some(name.clone()),
                    text: text.clone(),
                });
            }
        }
        if send_tick {
            let event = Event::Tick {
                tick: *tick / TICK_EVERY,
                dt: TICK_EVERY as f32 / crate::TICK_RATE as f32,
            };
            host = rt.dispatch(host, &event).0;
        }
    }

    // Give everything back.
    world.insert_resource(mods);
    world
        .get_resource_or_insert_with(AppliedEdits::default)
        .0
        .extend(applied);
    if let Some(w) = host.world.take() {
        world.insert_resource(w);
    }
    if let Some(c) = host.cache.take() {
        world.insert_resource(c);
    }
    if let Some(o) = host.open.take() {
        world.insert_resource(o);
    }
    if let Some(e) = host.entities.take() {
        world.insert_resource(e);
    }
    carry_out(world, host.out);
}

/// Do what the mods (and approved edits) asked for.
fn carry_out(world: &mut World, commands: Vec<ModCommand>) {
    if commands.is_empty() {
        return;
    }
    let time = world.resource::<OpenWorld>().meta.time_of_day;
    let mut players_q = world.query::<&mut ConnectedPlayer>();
    let mut players: Vec<Mut<ConnectedPlayer>> = players_q.iter_mut(world).collect();
    let mut outgoing: Vec<(Option<ClientId>, Channel, ServerMessage)> = Vec::new();
    for cmd in commands {
        match cmd {
            ModCommand::BlockChanged(pos, block) => {
                let col = ColumnPos::from_block(pos.x, pos.z);
                let msg = ServerMessage::BlockChanged {
                    pos: pos.to_array(),
                    block: block.0,
                };
                for p in players.iter().filter(|p| p.sent.contains(&col)) {
                    outgoing.push((Some(p.client), Channel::Reliable, msg.clone()));
                }
            }
            ModCommand::Revert(client, pos, block) => outgoing.push((
                Some(client),
                Channel::Reliable,
                ServerMessage::BlockChanged {
                    pos: pos.to_array(),
                    block: block.0,
                },
            )),
            ModCommand::Chat { to, from, text } => {
                outgoing.push((to, Channel::Reliable, ServerMessage::Chat { from, text }))
            }
            ModCommand::Teleport(client, pos) => {
                if let Some(p) = players.iter_mut().find(|p| p.client == client) {
                    p.physics.position = pos;
                    p.physics.velocity = Vec3::ZERO;
                    p.state.position = pos.to_array();
                }
            }
            ModCommand::TimeChanged => {
                outgoing.push((None, Channel::Reliable, ServerMessage::TimeOfDay(time)));
            }
            ModCommand::Rules(client, rules) => {
                if let Some(p) = players.iter_mut().find(|p| p.client == client) {
                    p.rules = rules.clone();
                    if !rules.can_fly {
                        p.physics.flying = false;
                    }
                }
                outgoing.push((
                    None,
                    Channel::Reliable,
                    ServerMessage::Appearance {
                        player: client,
                        color: rules.color,
                        tag: rules.tag.clone(),
                    },
                ));
                outgoing.push((Some(client), Channel::Reliable, ServerMessage::Rules(rules)));
            }
            ModCommand::Health(client, health, max) => outgoing.push((
                Some(client),
                Channel::Reliable,
                ServerMessage::Health { health, max },
            )),
            ModCommand::Hud(to, lines) => {
                outgoing.push((to, Channel::Reliable, ServerMessage::Hud(lines)))
            }
            ModCommand::Title(to, text, seconds) => outgoing.push((
                to,
                Channel::Reliable,
                ServerMessage::Title { text, seconds },
            )),
            ModCommand::Tools(client, tools) => {
                if let Some(p) = players.iter_mut().find(|p| p.client == client) {
                    p.tools = tools.clone();
                }
                outgoing.push((Some(client), Channel::Reliable, ServerMessage::Tools(tools)));
            }
            ModCommand::Broadcast(msg) => outgoing.push((None, Channel::Reliable, *msg)),
        }
    }
    drop(players);
    let mut net = world.resource_mut::<Network>();
    for (to, channel, msg) in outgoing {
        match to {
            Some(c) => net.send(c, channel, &msg),
            None => net.broadcast(channel, &msg),
        }
    }
}

/// Shared registry as an `Arc`, for the `Registry` resource.
pub fn registry_resource(registry: BlockRegistry) -> Registry {
    Registry(Arc::new(registry))
}

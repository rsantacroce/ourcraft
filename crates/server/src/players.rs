//! Connected players: joining, streaming the world to them, checking their
//! block edits, relaying movement and chat, and saving them.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::inventory::{INVENTORY_SLOTS, Inventory, ItemStack};
use ourcraft_common::item::{ItemId, ItemRegistry};
use ourcraft_common::physics::{Aabb, EYE_HEIGHT, PlayerState};
use ourcraft_common::prediction::{InputBuffer, apply_input};
use ourcraft_common::protocol::{
    Channel, ClientMessage, MAX_CHAT_LEN, NetPlayerState, NetSlot, PROTOCOL_VERSION, PlayerInfo,
    ServerMessage, sanitize_name,
};
use ourcraft_common::save::{PLAYER_VERSION, PlayerSave};
use ourcraft_common::world::{ColumnPos, WORLD_HEIGHT};
use renet::ClientId;

use crate::ServerConfig;
use crate::mods::{Content, ModQueue, PendingEdit};
use crate::net::{ClientConnected, ClientDisconnected, FromClient, Network};
use crate::world::{
    ColumnCache, OpenWorld, Registry, ServerWorld, WantedColumns, WorldSet, encoded_column,
};

/// How far players can reach, plus some slack for lag.
pub const MAX_REACH: f32 = 6.0 + 2.0;
/// Columns sent per player per tick, at most.
const COLUMNS_PER_TICK: usize = 6;
/// Movement updates sent per second.
const STATE_RATE: f32 = 20.0;
/// Seconds a new connection has to say hello.
const HELLO_TIMEOUT: f32 = 10.0;

pub struct PlayersPlugin;

impl Plugin for PlayersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Players>()
            .add_systems(
                Update,
                (
                    on_connect,
                    on_disconnect,
                    handle_messages,
                    crate::mods::run_mods,
                    simulate_players,
                    hello_timeout,
                    compute_wanted_columns,
                )
                    .chain()
                    .in_set(PlayersSet)
                    .before(WorldSet),
            )
            .add_systems(
                Update,
                (
                    stream_columns,
                    broadcast_states,
                    broadcast_time,
                    autosave_players,
                )
                    .chain()
                    .after(WorldSet),
            )
            .add_systems(Last, shutdown);
    }
}

/// Joining, messages, mods and movement (other modules run after it).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayersSet;

/// Client id → player entity.
#[derive(Resource, Default)]
pub struct Players(pub HashMap<ClientId, Entity>);

#[derive(Component, Debug)]
pub struct ConnectedPlayer {
    pub client: ClientId,
    /// Set once the client has said hello.
    pub name: Option<String>,
    pub view_distance: i32,
    /// What other players see (updated after each simulation step).
    pub state: NetPlayerState,
    /// The authoritative physics state, advanced by the player's inputs.
    pub physics: PlayerState,
    /// Inputs received but not yet applied.
    pub inputs: InputBuffer,
    /// The player's inventory as last reported by their game (the client
    /// owns it; the server keeps a copy to save and to check pickups).
    pub inventory: Inventory,
    /// Creative (fly, endless blocks, nothing drops) or survival.
    pub creative: bool,
    /// The furnace whose screen this player has open.
    pub open_container: Option<IVec3>,
    /// In bed, waiting for everyone else to sleep too.
    pub sleeping: bool,
    /// Health, hunger and breath (survival).
    pub needs: crate::needs::Needs,
    /// The boat (entity) they're sitting in.
    pub boat: Option<u64>,
    /// The animal (entity) they're riding, and how much faster than
    /// walking it goes.
    pub riding: Option<u64>,
    pub ride_speed: f32,
    /// The mine cart (entity) they're sitting in.
    pub cart: Option<u64>,
    /// Which way they last wanted to go (`x` right, `y` forward).
    pub last_move: Vec2,
    /// Seconds left of each potion effect ([`ourcraft_common::magic::Effect`]).
    pub effects: [f32; 3],
    /// What armour everyone was last told they wear.
    pub armour_sent: Option<[u16; 4]>,
    /// Columns this client has been sent and (as far as we know) still has.
    pub sent: HashSet<ColumnPos>,
    pub connected_for: f32,
    /// Set by game-mode mods.
    pub rules: ourcraft_mod_api::PlayerRules,
    /// Tools the player may use (from a mod), and when each was last used.
    pub tools: Vec<String>,
    pub tool_used_at: HashMap<String, f32>,
}

impl ConnectedPlayer {
    fn column(&self) -> ColumnPos {
        ColumnPos::from_world(self.state.position[0], self.state.position[2])
    }

    fn feet(&self) -> Vec3 {
        Vec3::from_array(self.state.position)
    }

    fn save(&self, items: &ItemRegistry) -> PlayerSave {
        let mut save = PlayerSave {
            version: PLAYER_VERSION,
            position: self.state.position,
            yaw: self.state.yaw,
            pitch: self.state.pitch,
            flying: self.state.flying,
            inventory: Vec::new(),
            selected: 0,
            creative: self.creative,
            vitals: Some(self.needs.saved()),
        };
        save.store_inventory(&self.inventory, items);
        save
    }

    /// Feet position.
    pub fn position(&self) -> Vec3 {
        self.feet()
    }
}

fn on_connect(
    mut commands: Commands,
    mut events: MessageReader<ClientConnected>,
    mut players: ResMut<Players>,
) {
    for ClientConnected(client) in events.read() {
        info!("Client {client} connected");
        let e = commands
            .spawn(ConnectedPlayer {
                client: *client,
                name: None,
                view_distance: 4,
                state: NetPlayerState::default(),
                physics: PlayerState::new(Vec3::ZERO),
                inputs: InputBuffer::default(),
                inventory: Inventory::default(),
                creative: true,
                open_container: None,
                sleeping: false,
                needs: crate::needs::Needs::default(),
                boat: None,
                riding: None,
                ride_speed: 1.0,
                cart: None,
                last_move: Vec2::ZERO,
                effects: [0.0; 3],
                armour_sent: None,
                sent: HashSet::new(),
                connected_for: 0.0,
                rules: ourcraft_mod_api::PlayerRules::default(),
                tools: Vec::new(),
                tool_used_at: HashMap::new(),
            })
            .id();
        players.0.insert(*client, e);
    }
}

fn save_player(open: &OpenWorld, player: &ConnectedPlayer) {
    if let Some(name) = &player.name
        && let Err(e) = open.storage.save_player(name, &player.save(&open.items))
    {
        error!("Saving player {name} failed: {e}");
    }
}

fn on_disconnect(
    mut commands: Commands,
    mut events: MessageReader<ClientDisconnected>,
    mut players: ResMut<Players>,
    query: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut queue: ResMut<ModQueue>,
    mut net: ResMut<Network>,
) {
    for ev in events.read() {
        let Some(e) = players.0.remove(&ev.client) else {
            continue;
        };
        if let Ok(p) = query.get(e) {
            if p.name.is_some() {
                queue
                    .events
                    .push(ourcraft_mod_api::Event::PlayerLeft { player: ev.client });
            }
            save_player(&open, p);
            if let Some(name) = &p.name {
                info!("{name} left ({})", ev.reason);
                net.broadcast(Channel::Reliable, &ServerMessage::PlayerLeft(ev.client));
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::Chat {
                        from: None,
                        text: format!("{name} left the game"),
                    },
                );
            }
        }
        commands.entity(e).despawn();
    }
}

fn hello_timeout(
    time: Res<Time>,
    mut query: Query<&mut ConnectedPlayer>,
    mut net: ResMut<Network>,
) {
    for mut p in &mut query {
        p.connected_for += time.delta_secs();
        if p.name.is_none() && p.connected_for > HELLO_TIMEOUT {
            p.connected_for = f32::MIN; // only kick once
            net.kick(p.client, "No hello received");
        }
    }
}

/// Make a name unique among online players by adding a number.
fn unique_name(wanted: &str, taken: &[String]) -> String {
    if !taken.iter().any(|n| n == wanted) {
        return wanted.to_owned();
    }
    (2..)
        .map(|i| format!("{wanted} {i}"))
        .find(|n| !taken.contains(n))
        .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn handle_messages(
    mut messages: MessageReader<FromClient>,
    players: Res<Players>,
    mut query: Query<&mut ConnectedPlayer>,
    config: Res<ServerConfig>,
    mut open: ResMut<OpenWorld>,
    registry: Res<Registry>,
    world: Res<ServerWorld>,
    content: Res<Content>,
    mut entities: ResMut<crate::entities::Entities>,
    mut weather: ResMut<crate::weather::Weather>,
    mut queue: ResMut<ModQueue>,
    mut net: ResMut<Network>,
) {
    for FromClient { client, msg } in messages.read() {
        let Some(&entity) = players.0.get(client) else {
            continue;
        };
        match msg {
            ClientMessage::Hello {
                name,
                view_distance,
                protocol_version,
            } => {
                if *protocol_version != PROTOCOL_VERSION {
                    net.kick(*client, "Your game version doesn't match the server");
                    continue;
                }
                let taken: Vec<String> = query.iter().filter_map(|p| p.name.clone()).collect();
                let others: Vec<PlayerInfo> = query
                    .iter()
                    .filter_map(|p| {
                        p.name.as_ref().map(|n| PlayerInfo {
                            id: p.client,
                            name: n.clone(),
                        })
                    })
                    .collect();
                let Ok(mut p) = query.get_mut(entity) else {
                    continue;
                };
                if p.name.is_some() {
                    continue; // already said hello
                }
                let name = unique_name(&sanitize_name(name), &taken);
                p.view_distance = (*view_distance as i32).clamp(2, config.max_view_distance as i32);

                // Back where they left off, or at the world spawn.
                let saved = open
                    .storage
                    .load_player(&name, &open.meta.block_names)
                    .unwrap_or_else(|e| {
                        error!("Can't read save for {name}: {e}");
                        None
                    });
                match &saved {
                    Some(s) => {
                        p.state.position = s.position;
                        p.state.yaw = s.yaw;
                        p.state.pitch = s.pitch;
                        p.state.flying = s.flying;
                        p.inventory = s.inventory(&open.items);
                        p.creative = s.creative;
                        if let Some(v) = s.vitals {
                            p.needs = crate::needs::Needs::restore(v);
                        }
                    }
                    None => {
                        p.creative = open.meta.creative;
                        let s = open.spawn;
                        p.state.position = [s.x as f32 + 0.5, s.y as f32 + 1.0, s.z as f32 + 0.5];
                        p.state.yaw = -std::f32::consts::FRAC_PI_4;
                        p.state.pitch = -0.35;
                    }
                }
                let inventory = saved.as_ref().map(|_| {
                    let slots = p
                        .inventory
                        .every_slot()
                        .map(|s| s.map(Into::into))
                        .collect();
                    (slots, p.inventory.selected as u8)
                });
                p.physics = PlayerState::new(Vec3::from_array(p.state.position));
                p.physics.flying = p.state.flying;
                p.name = Some(name.clone());
                info!("{name} joined (client {client})");

                net.send(
                    *client,
                    Channel::Reliable,
                    &ServerMessage::Welcome {
                        your_id: *client,
                        server_name: config.server_name.clone(),
                        motd: config.motd.clone(),
                        creative: p.creative,
                        time_of_day: open.time_of_day(),
                        state: p.state,
                        inventory,
                        content: content.0.clone(),
                        players: others,
                    },
                );
                // Where their compass points.
                let home = open.beds.get(&name).copied().unwrap_or(open.spawn);
                net.send(
                    *client,
                    Channel::Reliable,
                    &ServerMessage::Home {
                        pos: home.to_array(),
                    },
                );
                let info = PlayerInfo {
                    id: *client,
                    name: name.clone(),
                };
                net.broadcast_except(
                    *client,
                    Channel::Reliable,
                    &ServerMessage::PlayerJoined(info),
                );
                queue.events.push(ourcraft_mod_api::Event::PlayerJoined {
                    player: *client,
                    name: name.clone(),
                });
                // Catch the newcomer up on how everyone looks and what's
                // around (flags, projectiles…).
                for other in query
                    .iter()
                    .filter(|o| o.name.is_some() && o.client != *client)
                {
                    if other.rules.color.is_some() || other.rules.tag.is_some() {
                        net.send(
                            *client,
                            Channel::Reliable,
                            &ServerMessage::Appearance {
                                player: other.client,
                                color: other.rules.color,
                                tag: other.rules.tag.clone(),
                            },
                        );
                    }
                }
                for (id, e) in &entities.list {
                    net.send(
                        *client,
                        Channel::Reliable,
                        &crate::entities::Entities::spawn_message(*id, e),
                    );
                }
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::Chat {
                        from: None,
                        text: format!("{name} joined the game"),
                    },
                );
            }
            ClientMessage::UseTool {
                tool,
                direction,
                secondary,
            } => {
                let Ok(mut p) = query.get_mut(entity) else {
                    continue;
                };
                if p.name.is_none() || !p.tools.contains(tool) {
                    continue;
                }
                let dir = Vec3::from_array(*direction);
                let Some(dir) = dir.try_normalize().filter(|d| d.is_finite()) else {
                    continue;
                };
                // Respect the tool's cooldown (with a little slack for lag).
                let cooldown = content
                    .0
                    .tools
                    .iter()
                    .find(|t| &t.name == tool)
                    .map_or(0.1, |t| t.cooldown);
                let now = p.connected_for;
                let last = p.tool_used_at.get(tool).copied().unwrap_or(f32::MIN);
                if now - last < cooldown * 0.75 {
                    continue;
                }
                p.tool_used_at.insert(tool.clone(), now);
                // Aim from where the server knows the player's eyes are.
                let origin = p.physics.position + Vec3::Y * EYE_HEIGHT;
                queue.events.push(ourcraft_mod_api::Event::UseTool {
                    player: *client,
                    tool: tool.clone(),
                    origin: origin.to_array(),
                    direction: dir.to_array(),
                    secondary: *secondary,
                });
            }
            ClientMessage::ViewDistance(vd) => {
                if let Ok(mut p) = query.get_mut(entity) {
                    p.view_distance = (*vd as i32).clamp(2, config.max_view_distance as i32);
                }
            }
            ClientMessage::Inputs(inputs) => {
                if let Ok(mut p) = query.get_mut(entity)
                    && p.name.is_some()
                {
                    // Ignore nonsense values (NaN, out-of-range sticks).
                    let valid: Vec<_> = inputs
                        .iter()
                        .filter(|i| {
                            i.yaw.is_finite()
                                && i.pitch.is_finite()
                                && i.movement.iter().all(|m| m.is_finite() && m.abs() <= 1.0)
                        })
                        .copied()
                        .collect();
                    p.inputs.receive(&valid);
                }
            }
            ClientMessage::Inventory { slots, selected } => {
                if let Ok(mut p) = query.get_mut(entity) {
                    p.inventory = inventory_from_net(slots, *selected, &open.items);
                }
            }
            ClientMessage::GameMode { creative } => {
                if let Ok(mut p) = query.get_mut(entity) {
                    p.creative = *creative;
                }
            }
            // Furnace screens: see the survival module.
            ClientMessage::OpenContainer { .. }
            | ClientMessage::CloseContainer
            | ClientMessage::ContainerClick { .. }
            | ClientMessage::DropItems(_)
            | ClientMessage::UseEntity { .. }
            | ClientMessage::UseBlock { .. }
            | ClientMessage::SetSign { .. }
            | ClientMessage::Eat { .. }
            | ClientMessage::HitEntity { .. }
            | ClientMessage::LeaveBed
            | ClientMessage::LeaveBoat
            | ClientMessage::UseItem { .. } => {}
            ClientMessage::SetBlock { pos, block, held } => {
                let pos = IVec3::from_array(*pos);
                let block = BlockId(*block);
                let Ok(p) = query.get(entity) else { continue };
                if p.name.is_none() {
                    continue;
                }
                let others: Vec<Vec3> = query.iter().map(|o| o.feet()).collect();
                let held_name = open.items.name(ItemId(*held));
                let ok = edit_allowed(&world, &registry.0, p, &others, pos, block, held_name);
                let current = world.get_block(pos.x, pos.y, pos.z);
                match (ok, current) {
                    // Allowed so far: mods get a say before it happens.
                    (true, Some(old)) => queue.edits.push(PendingEdit {
                        client: *client,
                        pos,
                        old,
                        new: block,
                        held: ItemId(*held),
                    }),
                    // Refused: put the client's copy back the way it was.
                    (false, Some(actual)) => net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::BlockChanged {
                            pos: pos.to_array(),
                            block: actual.0,
                        },
                    ),
                    _ => {}
                }
            }
            ClientMessage::Chat { text } => {
                let Ok(p) = query.get(entity) else { continue };
                let Some(name) = p.name.clone() else { continue };
                let text: String = text
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(MAX_CHAT_LEN)
                    .collect();
                let text = text.trim().to_owned();
                if text.is_empty() {
                    continue;
                }
                // /tp x y z: creative players can jump anywhere.
                if let Some(rest) = text.strip_prefix("/tp") {
                    let to: Vec<f32> = rest
                        .split_whitespace()
                        .filter_map(|w| w.parse().ok())
                        .collect();
                    let reply = match (p.creative, to.as_slice()) {
                        (false, _) => "You can use /tp in creative mode (G)".to_owned(),
                        (true, [x, y, z]) if [x, y, z].iter().all(|v| v.abs() < 1.0e6) => {
                            let at = Vec3::new(*x, y.clamp(0.0, WORLD_HEIGHT as f32), *z);
                            if let Ok(mut p) = query.get_mut(entity) {
                                p.physics.position = at;
                                p.physics.velocity = Vec3::ZERO;
                                p.state.position = at.to_array();
                            }
                            format!("Whoosh! You're at {x:.0} {y:.0} {z:.0}.")
                        }
                        _ => "Try /tp x y z (like /tp 0 80 0)".to_owned(),
                    };
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::Chat {
                            from: None,
                            text: reply,
                        },
                    );
                    continue;
                }
                // /summon <animal>: creative players can call an animal over.
                if let Some(what) = text.strip_prefix("/summon") {
                    let what = what.trim();
                    let name = if what.contains(':') {
                        what.to_owned()
                    } else {
                        format!("core:{what}")
                    };
                    let reply = if !p.creative {
                        "You can summon animals in creative mode (G)".to_owned()
                    } else if let Some(def) = open.mobs.iter().find(|m| m.name == name) {
                        let look = crate::mods::look_vector(p.state.yaw, 0.0);
                        let at = p.feet() + Vec3::from_array(look) * 3.0 + Vec3::Y * 0.5;
                        crate::mobs::spawn_mob(&mut entities, &name, at, false);
                        format!("Here comes a {}!", def.display_name.to_lowercase())
                    } else {
                        let names: Vec<&str> = open
                            .mobs
                            .iter()
                            .map(|m| m.name.trim_start_matches("core:"))
                            .collect();
                        format!("Try /summon {}", names.join(", /summon "))
                    };
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::Chat {
                            from: None,
                            text: reply,
                        },
                    );
                    continue;
                }
                if let Some(reply) = run_command(&text, &mut open, &mut weather) {
                    net.send(
                        *client,
                        Channel::Reliable,
                        &ServerMessage::Chat {
                            from: None,
                            text: reply,
                        },
                    );
                    if text.starts_with("/time") {
                        net.broadcast(
                            Channel::Reliable,
                            &ServerMessage::TimeOfDay(open.time_of_day()),
                        );
                    }
                    continue;
                }
                // Mods may want it (commands); otherwise it's shown to everyone.
                queue.chats.push((*client, name, text));
            }
        }
    }
}

/// An inventory reported by a client, with nonsense (unknown items, empty
/// stacks, oversized stacks) removed.
fn inventory_from_net(slots: &[NetSlot], selected: u8, items: &ItemRegistry) -> Inventory {
    let mut inv = Inventory::default();
    for (i, (slot, net)) in inv.every_slot_mut().zip(slots.iter()).enumerate() {
        *slot = net
            .map(ItemStack::from)
            .filter(|s| s.count > 0 && (s.item.0 as usize) < items.len() && s.item.0 != 0)
            .map(|s| ItemStack {
                count: s.count.min(items.max_stack(s.item)),
                ..s
            });
        // Only armour goes in an armour slot (and only the right piece).
        if i >= INVENTORY_SLOTS {
            let fits = slot.is_some_and(|s| {
                matches!(
                    items.get(s.item).map(|d| d.kind),
                    Some(ourcraft_common::item::ItemKind::Armour { slot, .. })
                        if slot as usize == i - INVENTORY_SLOTS
                )
            });
            if !fits {
                *slot = None;
            }
        }
    }
    inv.select(selected as usize);
    inv
}

/// Inputs applied per player per server tick, at most. One arrives per
/// tick normally; a few more let a player catch up after a lag spike.
const MAX_INPUTS_PER_TICK: usize = 4;

/// Move every player by applying their inputs, with the same physics code
/// the clients use to predict.
fn simulate_players(
    mut query: Query<&mut ConnectedPlayer>,
    world: Res<ServerWorld>,
    registry: Res<Registry>,
) {
    for mut p in &mut query {
        if p.name.is_none() {
            continue;
        }
        let p = &mut *p;
        let (speed, can_fly) = (p.rules.speed, p.rules.can_fly);
        for mut input in p.inputs.take(MAX_INPUTS_PER_TICK) {
            // The game mode's rules win over whatever the client assumed.
            input.speed = if p.riding.is_some() {
                speed * p.ride_speed
            } else {
                speed
            };
            input.fly &= can_fly;
            input.boat = p.boat.is_some();
            input.effects = ourcraft_common::magic::Effect::bits(&p.effects);
            p.last_move = Vec2::from_array(input.movement);
            apply_input(&mut p.physics, &input, &world.0, &registry.0);
            p.state.yaw = input.yaw;
            p.state.pitch = input.pitch;
        }
        // Keep players inside the world.
        p.physics.position.y = p
            .physics
            .position
            .y
            .clamp(-64.0, WORLD_HEIGHT as f32 + 64.0);
        p.state.position = p.physics.position.to_array();
        p.state.velocity = p.physics.velocity.to_array();
        p.state.flying = p.physics.flying;
        p.state.on_ground = p.physics.on_ground;
    }
}

/// A few chat commands. Returns a reply if `text` was a command.
fn run_command(
    text: &str,
    open: &mut OpenWorld,
    weather: &mut crate::weather::Weather,
) -> Option<String> {
    let mut words = text.split_whitespace();
    match words.next()? {
        "/help" => Some(
            "Commands: /time day, /time night, /weather rain, /weather clear, /summon sheep (or pig, cow, chicken), /tp x y z, /peaceful (mods may add more)"
                .into(),
        ),
        "/peaceful" => {
            open.peaceful = !open.peaceful;
            Some(if open.peaceful {
                "Peaceful: no monsters, and no getting hungry.".into()
            } else {
                "Peaceful is off: watch out for monsters at night!".into()
            })
        }
        "/weather" => match words.next() {
            Some("rain" | "snow") => {
                weather.set(true, text.len() as u32 * 977);
                Some("Here comes the rain!".into())
            }
            Some("clear" | "sun") => {
                weather.set(false, text.len() as u32 * 977);
                Some("The sun's out!".into())
            }
            _ => Some("Use /weather rain or /weather clear".into()),
        },
        "/time" => match words.next() {
            Some("day") => {
                open.meta.time_of_day = 0.3;
                Some("Good morning!".into())
            }
            Some("night") => {
                open.meta.time_of_day = 0.8;
                Some("Good night!".into())
            }
            _ => Some("Use /time day or /time night".into()),
        },
        // Anything else might be a mod's command.
        _ => None,
    }
}

/// The server's rules for changing blocks.
fn edit_allowed(
    world: &ServerWorld,
    registry: &ourcraft_common::block::BlockRegistry,
    player: &ConnectedPlayer,
    players: &[Vec3],
    pos: IVec3,
    block: BlockId,
    held: &str,
) -> bool {
    let bucket = held == "core:bucket";
    if block.0 as usize >= registry.len() || !player.rules.can_build {
        return false;
    }
    let Some(existing) = world.get_block(pos.x, pos.y, pos.z) else {
        return false; // not loaded
    };
    if !(0..WORLD_HEIGHT).contains(&pos.y) {
        return false;
    }
    let eye = player.feet() + Vec3::Y * EYE_HEIGHT;
    if eye.distance(pos.as_vec3() + Vec3::splat(0.5)) > MAX_REACH {
        return false;
    }
    let near = |o: [i32; 3]| {
        world
            .get_block(pos.x + o[0], pos.y + o[1], pos.z + o[2])
            .unwrap_or_default()
    };
    if block.is_air() {
        // Breaking (or scooping up water or lava with a bucket).
        !existing.is_air()
            && (registry.is_breakable(existing) || (bucket && registry.is_fluid_source(existing)))
    } else if registry.is_farmland(block) {
        // A hoe turns dirt and grass into farmland (with air above).
        let soil = existing == ourcraft_common::block::blocks::DIRT
            || existing == ourcraft_common::block::blocks::GRASS;
        held.ends_with("_hoe") && soil && near([0, 1, 0]).is_air()
    } else {
        // Placing: only into empty space (tall grass counts), never inside
        // anyone, and plants only where they can grow.
        let replaceable =
            existing.is_air() || registry.is_liquid(existing) || registry.is_soft(existing);
        if !registry.fits(block, near) {
            return false;
        }
        let blocks_someone = registry.is_solid(block)
            && players
                .iter()
                .any(|feet| Aabb::player(*feet).intersects(&Aabb::block(pos)));
        replaceable && !blocks_someone
    }
}

/// Columns within `r` of `center`, as (position, squared distance).
fn columns_around(center: ColumnPos, r: i32) -> impl Iterator<Item = (ColumnPos, i32)> {
    (-r..=r).flat_map(move |dz| {
        (-r..=r).map(move |dx| {
            (
                ColumnPos::new(center.x + dx, center.z + dz),
                dx * dx + dz * dz,
            )
        })
    })
}

fn compute_wanted_columns(query: Query<&ConnectedPlayer>, mut wanted: ResMut<WantedColumns>) {
    wanted.columns.clear();
    wanted.keep.clear();
    for p in query.iter().filter(|p| p.name.is_some()) {
        // Clients mesh a column only once its neighbours arrive, so send
        // one ring beyond their view distance.
        for (col, d) in columns_around(p.column(), p.view_distance + 1) {
            let e = wanted.columns.entry(col).or_insert(i32::MAX);
            *e = (*e).min(d);
        }
        for (col, _) in columns_around(p.column(), p.view_distance + 3) {
            wanted.keep.insert(col);
        }
    }
}

fn stream_columns(
    mut query: Query<&mut ConnectedPlayer>,
    world: Res<ServerWorld>,
    open: Res<OpenWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
) {
    for mut p in &mut query {
        if p.name.is_none() {
            continue;
        }
        let center = p.column();
        let r = p.view_distance + 1;
        // Forget columns the client will have unloaded (it keeps two rings
        // beyond its view distance), so they're re-sent if it comes back.
        p.sent.retain(|c| c.chebyshev_distance(center) <= r + 1);

        let mut todo: Vec<(ColumnPos, i32)> = columns_around(center, r)
            .filter(|(c, _)| !p.sent.contains(c) && world.has_column(*c))
            .collect();
        todo.sort_by_key(|(_, d)| *d);
        for (col, _) in todo.into_iter().take(COLUMNS_PER_TICK) {
            let Some(data) = encoded_column(&mut cache, &world, col) else {
                continue;
            };
            // Don't overfill the reliable channel (that would disconnect).
            if !net.has_room(p.client, data.len() + 64) {
                break;
            }
            net.send(
                p.client,
                Channel::Reliable,
                &ServerMessage::Column {
                    x: col.x,
                    z: col.z,
                    data: (*data).clone(),
                },
            );
            p.sent.insert(col);
            // And what the signs in it say.
            for (pos, lines) in open
                .signs
                .iter()
                .filter(|(pos, _)| ColumnPos::from_block(pos.x, pos.z) == col)
            {
                net.send(
                    p.client,
                    Channel::Reliable,
                    &ServerMessage::SignText {
                        pos: pos.to_array(),
                        lines: lines.clone(),
                    },
                );
            }
            // And what's in its item frames.
            for (pos, item) in open
                .frames
                .iter()
                .filter(|(pos, _)| ColumnPos::from_block(pos.x, pos.z) == col)
            {
                net.send(
                    p.client,
                    Channel::Reliable,
                    &ServerMessage::FrameItem {
                        pos: pos.to_array(),
                        item: item.0,
                    },
                );
            }
        }
    }
}

fn broadcast_states(
    time: Res<Time>,
    query: Query<&ConnectedPlayer>,
    mut net: ResMut<Network>,
    mut timer: Local<f32>,
    mut tick: Local<u32>,
) {
    *timer += time.delta_secs();
    if *timer < 1.0 / STATE_RATE {
        return;
    }
    *timer = 0.0;
    *tick += 1;
    // Each player's own authoritative state, for reconciliation.
    for p in query.iter().filter(|p| p.name.is_some()) {
        if p.inputs.last_applied > 0 {
            net.send(
                p.client,
                Channel::Unreliable,
                &ServerMessage::YourState {
                    ack_tick: p.inputs.last_applied,
                    state: p.state,
                },
            );
        }
    }
    let players: Vec<_> = query
        .iter()
        .filter(|p| p.name.is_some())
        .map(|p| (p.client, p.state))
        .collect();
    if players.len() > 1 {
        net.broadcast(
            Channel::Unreliable,
            &ServerMessage::PlayerStates {
                tick: *tick,
                players,
            },
        );
    }
}

fn broadcast_time(
    time: Res<Time>,
    open: Res<OpenWorld>,
    mut net: ResMut<Network>,
    mut timer: Local<f32>,
) {
    *timer += time.delta_secs();
    if *timer >= 5.0 {
        *timer = 0.0;
        net.broadcast(
            Channel::Reliable,
            &ServerMessage::TimeOfDay(open.time_of_day()),
        );
    }
}

fn autosave_players(
    time: Res<Time>,
    query: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut timer: Local<f32>,
) {
    *timer += time.delta_secs();
    if *timer >= 30.0 {
        *timer = 0.0;
        for p in &query {
            save_player(&open, p);
        }
    }
}

/// On shutdown: save everyone and everything, then say goodbye. Also
/// handles "save now" requests from the embedding game.
fn shutdown(
    mut exit: MessageReader<AppExit>,
    save_now: Res<crate::SaveNowFlag>,
    query: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    mut net: ResMut<Network>,
) {
    if save_now.0.swap(false, std::sync::atomic::Ordering::Relaxed) {
        for p in &query {
            save_player(&open, p);
        }
        crate::world::save_all(&open);
    }
    if exit.read().count() == 0 {
        return;
    }
    info!("Server stopping: saving…");
    for p in &query {
        save_player(&open, p);
    }
    crate::world::save_all(&open);
    crate::net::disconnect_all(&mut net);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_names_get_numbers() {
        let taken = vec!["Sam".to_string(), "Sam 2".to_string()];
        assert_eq!(unique_name("Alex", &taken), "Alex");
        assert_eq!(unique_name("Sam", &taken), "Sam 3");
    }

    #[test]
    fn columns_around_is_a_square() {
        let cols: Vec<_> = columns_around(ColumnPos::new(5, -5), 2).collect();
        assert_eq!(cols.len(), 25);
        assert!(cols.contains(&(ColumnPos::new(7, -3), 8)));
    }
}

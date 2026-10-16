//! Circuits on the server (milestone 22): levers, buttons and pressure
//! plates switch on, power spreads along spark wire, and lamps, doors,
//! pistons, note blocks and powered rails react.
//!
//! Power isn't stored: whenever something changes near a circuit, the
//! wire around it is traced and its power worked out again from the
//! sources (see `ourcraft_common::circuits`).

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::prelude::*;
use ourcraft_common::block::BlockId;
use ourcraft_common::circuits::MAX_POWER;
use ourcraft_common::protocol::{Channel, ClientMessage, ServerMessage};
use ourcraft_common::shapes::VariantKind;

use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::world::{ColumnCache, OpenWorld, Registry, ServerWorld};

/// A button stays pressed this long (seconds).
const BUTTON_TIME: f32 = 1.5;
/// Most wire traced from one change.
const MAX_WIRE: usize = 2048;
/// Most blocks a piston pushes.
const MAX_PUSH: usize = 12;

pub struct CircuitsPlugin;

impl Plugin for CircuitsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PowerDirty>()
            .init_resource::<Buttons>()
            .add_systems(
                Update,
                (use_switches, release_buttons, pressure_plates, recompute)
                    .chain()
                    .after(crate::players::PlayersSet),
            );
    }
}

/// Spots where something changed: the power around them is worked out
/// again.
#[derive(Resource, Default)]
pub struct PowerDirty(pub Vec<IVec3>);

/// Pressed buttons and how long until they pop back up.
#[derive(Resource, Default)]
struct Buttons(Vec<(IVec3, f32)>);

const SIDES: [IVec3; 6] = [
    IVec3::X,
    IVec3::NEG_X,
    IVec3::Y,
    IVec3::NEG_Y,
    IVec3::Z,
    IVec3::NEG_Z,
];

fn change(
    world: &mut ServerWorld,
    cache: &mut ColumnCache,
    open: &OpenWorld,
    net: &mut Network,
    pos: IVec3,
    block: BlockId,
) {
    crate::building::change(world, cache, open, net, pos, block);
}

/// Flipping levers, pressing buttons, playing note blocks by hand.
#[allow(clippy::too_many_arguments)]
fn use_switches(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    players: Query<&ConnectedPlayer>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
    mut dirty: ResMut<PowerDirty>,
    mut buttons: ResMut<Buttons>,
) {
    let reg = &registry.0;
    for FromClient { client, msg } in messages.read() {
        let ClientMessage::UseBlock { pos, .. } = msg else {
            continue;
        };
        let Some(p) = players_map.0.get(client).and_then(|e| players.get(*e).ok()) else {
            continue;
        };
        let pos = IVec3::from_array(*pos);
        if p.position().distance(pos.as_vec3()) > crate::players::MAX_REACH + 1.0 {
            continue;
        }
        let Some(b) = world.get_block(pos.x, pos.y, pos.z) else {
            continue;
        };
        let click = |net: &mut Network, name: &str| {
            net.broadcast(
                Channel::Reliable,
                &ServerMessage::Sound {
                    name: name.into(),
                    at: Some((pos.as_vec3() + Vec3::splat(0.5)).to_array()),
                },
            );
        };
        match reg.variant_kind(b) {
            Some(VariantKind::Lever) => {
                if let Some(flipped) = reg.powered(b, !reg.is_on(b)) {
                    change(&mut world, &mut cache, &open, &mut net, pos, flipped);
                    dirty.0.push(pos);
                    click(&mut net, "click");
                }
            }
            Some(VariantKind::Button) if !reg.is_on(b) => {
                if let Some(pressed) = reg.powered(b, true) {
                    change(&mut world, &mut cache, &open, &mut net, pos, pressed);
                    buttons.0.push((pos, BUTTON_TIME));
                    dirty.0.push(pos);
                    click(&mut net, "click");
                }
            }
            _ if reg.get(b).name == "core:note_block" => click(&mut net, "pop"),
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn release_buttons(
    time: Res<Time>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
    mut dirty: ResMut<PowerDirty>,
    mut buttons: ResMut<Buttons>,
) {
    let dt = time.delta_secs();
    let mut up = Vec::new();
    buttons.0.retain_mut(|(pos, left)| {
        *left -= dt;
        if *left <= 0.0 {
            up.push(*pos);
            false
        } else {
            true
        }
    });
    for pos in up {
        let Some(b) = world.get_block(pos.x, pos.y, pos.z) else {
            continue;
        };
        if registry.0.variant_kind(b) == Some(VariantKind::Button)
            && let Some(released) = registry.0.powered(b, false)
        {
            change(&mut world, &mut cache, &open, &mut net, pos, released);
            dirty.0.push(pos);
        }
    }
}

/// Plates are pressed while someone (or some animal) stands on them.
#[allow(clippy::too_many_arguments)]
fn pressure_plates(
    time: Res<Time>,
    players: Query<&ConnectedPlayer>,
    entities: Res<crate::entities::Entities>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
    mut dirty: ResMut<PowerDirty>,
    mut timer: Local<f32>,
    mut pressed: Local<HashSet<IVec3>>,
) {
    *timer += time.delta_secs();
    if *timer < 0.2 {
        return;
    }
    *timer = 0.0;
    let reg = &registry.0;
    // Where feet are: players, and animals (entities shown as "@mob:").
    let mut feet: Vec<IVec3> = players
        .iter()
        .filter(|p| p.name.is_some())
        .map(|p| (p.position() + Vec3::Y * 0.1).floor().as_ivec3())
        .collect();
    feet.extend(
        entities
            .list
            .values()
            .filter(|e| e.model.starts_with("@mob:"))
            .map(|e| (e.position + Vec3::Y * 0.1).floor().as_ivec3()),
    );
    let mut now: HashSet<IVec3> = HashSet::new();
    for f in &feet {
        if let Some(b) = world.get_block(f.x, f.y, f.z)
            && reg.variant_kind(b) == Some(VariantKind::PressurePlate)
        {
            now.insert(*f);
        }
    }
    let changed: Vec<(IVec3, bool)> = now
        .difference(&pressed)
        .map(|p| (*p, true))
        .chain(pressed.difference(&now).map(|p| (*p, false)))
        .collect();
    for (pos, on) in changed {
        if let Some(b) = world.get_block(pos.x, pos.y, pos.z)
            && reg.variant_kind(b) == Some(VariantKind::PressurePlate)
            && let Some(new) = reg.powered(b, on)
            && new != b
        {
            change(&mut world, &mut cache, &open, &mut net, pos, new);
            dirty.0.push(pos);
        }
    }
    *pressed = now;
}

/// Work out the power around everything that changed, and switch things.
#[allow(clippy::too_many_arguments)]
fn recompute(
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut net: ResMut<Network>,
    mut dirty: ResMut<PowerDirty>,
    mut last: Local<HashMap<IVec3, bool>>,
) {
    if dirty.0.is_empty() {
        return;
    }
    let reg = &registry.0;
    let starts: Vec<IVec3> = std::mem::take(&mut dirty.0);
    let get = |w: &ServerWorld, p: IVec3| w.get_block(p.x, p.y, p.z).unwrap_or_default();
    let carries = |b: BlockId| reg.is_wire(b) || reg.variant_kind(b) == Some(VariantKind::Repeater);

    // ---- Trace the wire (and repeaters) touching the changes. ----
    let mut net_nodes: HashSet<IVec3> = HashSet::new();
    let mut queue: VecDeque<IVec3> = VecDeque::new();
    for s in &starts {
        for d in SIDES.iter().copied().chain([IVec3::ZERO]) {
            let p = *s + d;
            if carries(get(&world, p)) && net_nodes.insert(p) {
                queue.push_back(p);
            }
        }
    }
    while let Some(p) = queue.pop_front() {
        if net_nodes.len() >= MAX_WIRE {
            break;
        }
        for d in SIDES {
            let q = p + d;
            if !net_nodes.contains(&q) && carries(get(&world, q)) {
                net_nodes.insert(q);
                queue.push_back(q);
            }
        }
    }
    let facing_of = |b: BlockId| {
        reg.get(b)
            .variant
            .as_ref()
            .map(|v| IVec3::from_array(v.facing.vector()))
    };

    // ---- Power levels, with repeaters settling over a few passes. ----
    let mut repeaters_on: HashMap<IVec3, bool> = net_nodes
        .iter()
        .filter(|p| reg.variant_kind(get(&world, **p)) == Some(VariantKind::Repeater))
        .map(|p| (*p, false))
        .collect();
    let mut levels: HashMap<IVec3, u8> = HashMap::new();
    // Does `from` power its neighbour `to` by itself (a source, or an on
    // repeater pointing at it)?
    let gives = |w: &ServerWorld, rep: &HashMap<IVec3, bool>, from: IVec3, to: IVec3| -> u8 {
        let b = get(w, from);
        let s = reg.source_power(b);
        if s > 0 {
            return s;
        }
        if rep.get(&from).copied().unwrap_or(false) && facing_of(b).map(|f| from + f) == Some(to) {
            return MAX_POWER;
        }
        0
    };
    for _ in 0..6 {
        levels.clear();
        let mut q: VecDeque<IVec3> = VecDeque::new();
        for p in net_nodes.iter().filter(|p| reg.is_wire(get(&world, **p))) {
            let lvl = SIDES
                .iter()
                .map(|d| gives(&world, &repeaters_on, *p + *d, *p))
                .max()
                .unwrap_or(0);
            if lvl > 0 {
                levels.insert(*p, lvl);
                q.push_back(*p);
            }
        }
        while let Some(p) = q.pop_front() {
            let lvl = levels[&p];
            if lvl <= 1 {
                continue;
            }
            for d in SIDES {
                let n = p + d;
                if net_nodes.contains(&n)
                    && reg.is_wire(get(&world, n))
                    && levels.get(&n).copied().unwrap_or(0) < lvl - 1
                {
                    levels.insert(n, lvl - 1);
                    q.push_back(n);
                }
            }
        }
        // A repeater is on when the block behind it is powered.
        let mut settled = true;
        for (p, on) in repeaters_on.clone() {
            let Some(f) = facing_of(get(&world, p)) else {
                continue;
            };
            let back = p - f;
            let now_on = levels.get(&back).copied().unwrap_or(0) > 0
                || gives(&world, &repeaters_on, back, p) > 0;
            if now_on != on {
                repeaters_on.insert(p, now_on);
                settled = false;
            }
        }
        if settled {
            break;
        }
    }

    // ---- Switch the wire and repeaters. ----
    for p in &net_nodes {
        let b = get(&world, *p);
        let on = if reg.is_wire(b) {
            levels.get(p).copied().unwrap_or(0) > 0
        } else {
            repeaters_on.get(p).copied().unwrap_or(false)
        };
        if let Some(new) = reg.powered(b, on).filter(|n| *n != b) {
            change(&mut world, &mut cache, &open, &mut net, *p, new);
        }
    }

    // ---- Everything next to the circuit (and the changes) that reacts. ----
    let mut around: HashSet<IVec3> = HashSet::new();
    for p in net_nodes.iter().chain(starts.iter()) {
        around.insert(*p);
        for d in SIDES {
            around.insert(*p + d);
        }
    }
    for c in around {
        let b = get(&world, c);
        if !reg.reacts_to_power(b) {
            continue;
        }
        let powered = SIDES.iter().any(|d| {
            let n = c + *d;
            levels.get(&n).copied().unwrap_or(0) > 0 || gives(&world, &repeaters_on, n, c) > 0
        });
        // What the power was before (for something we haven't seen yet:
        // a door's open state, otherwise off).
        let seen = matches!(
            reg.variant_kind(b),
            Some(VariantKind::Door | VariantKind::Trapdoor | VariantKind::FenceGate)
        ) && reg.is_on(b);
        let before = last.insert(c, powered).or(Some(seen));
        // Only react when the power really changed (so a door you opened
        // by hand stays open until the power does something).
        let edge = before != Some(powered);
        match reg.variant_kind(b) {
            Some(VariantKind::SparkLamp | VariantKind::PoweredRail) => {
                if let Some(new) = reg.powered(b, powered).filter(|n| *n != b) {
                    change(&mut world, &mut cache, &open, &mut net, c, new);
                }
            }
            Some(VariantKind::Door | VariantKind::Trapdoor | VariantKind::FenceGate)
                if edge && reg.is_on(b) != powered =>
            {
                if let Some(changes) = reg.toggled(b) {
                    for (o, new) in changes {
                        let at = c + IVec3::from_array(o);
                        change(&mut world, &mut cache, &open, &mut net, at, new);
                    }
                }
            }
            Some(VariantKind::Piston) => {
                if powered != reg.is_on(b) {
                    let moved =
                        move_piston(&mut world, &mut cache, &open, &mut net, reg, c, b, powered);
                    dirty.0.extend(moved);
                }
            }
            _ if reg.get(b).name == "core:note_block" && edge && powered => {
                net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::Sound {
                        name: "pop".into(),
                        at: Some((c.as_vec3() + Vec3::splat(0.5)).to_array()),
                    },
                );
            }
            _ => {}
        }
    }
}

/// Blocks a piston can't push: storage and things with data, and the
/// unbreakable ones.
fn immovable(reg: &ourcraft_common::block::BlockRegistry, b: BlockId) -> bool {
    let name = reg.get(b).name.as_str();
    !reg.is_breakable(b)
        || matches!(
            name,
            "core:chest" | "core:barrel" | "core:furnace" | "core:obsidian"
        )
        || matches!(
            reg.variant_kind(b),
            Some(
                VariantKind::Sign
                    | VariantKind::WallSign
                    | VariantKind::WallHanging
                    | VariantKind::PistonHead
                    | VariantKind::Bed
                    | VariantKind::Door
            )
        )
}

/// Push a piston out (or pull it in). Returns the spots that changed.
#[allow(clippy::too_many_arguments)]
fn move_piston(
    world: &mut ServerWorld,
    cache: &mut ColumnCache,
    open: &OpenWorld,
    net: &mut Network,
    reg: &ourcraft_common::block::BlockRegistry,
    at: IVec3,
    piston: BlockId,
    out: bool,
) -> Vec<IVec3> {
    let Some(v) = reg.get(piston).variant.clone() else {
        return Vec::new();
    };
    let f = IVec3::from_array(v.facing.vector());
    let get = |w: &ServerWorld, p: IVec3| w.get_block(p.x, p.y, p.z).unwrap_or_default();
    let empty = |b: BlockId| b.is_air() || reg.is_liquid(b) || reg.is_soft(b);
    let sticky = v.family.contains("sticky");
    let head_family = if sticky {
        "core:sticky_piston_head"
    } else {
        "core:piston_head"
    };
    let mut changed = Vec::new();
    if out {
        // The line of blocks in front, up to the first gap.
        let mut line = Vec::new();
        let mut p = at + f;
        while !empty(get(world, p)) {
            let b = get(world, p);
            if immovable(reg, b) || line.len() >= MAX_PUSH {
                return Vec::new(); // too heavy: nothing happens
            }
            line.push((p, b));
            p += f;
        }
        // Move them one step, from the far end.
        for (p, b) in line.iter().rev() {
            change(world, cache, open, net, *p + f, *b);
            changed.push(*p + f);
        }
        let Some(head) = reg.find_variant(head_family, v.facing, false, false) else {
            return changed;
        };
        change(world, cache, open, net, at + f, head);
        changed.push(at + f);
    } else {
        let front = at + f;
        if reg.variant_kind(get(world, front)) == Some(VariantKind::PistonHead) {
            change(world, cache, open, net, front, BlockId::AIR);
            changed.push(front);
            // A sticky piston pulls back what's in front of its head.
            let beyond = front + f;
            let b = get(world, beyond);
            if sticky && !empty(b) && !immovable(reg, b) {
                change(world, cache, open, net, front, b);
                change(world, cache, open, net, beyond, BlockId::AIR);
                changed.push(beyond);
            }
        }
    }
    if let Some(new) = reg.powered(piston, out) {
        change(world, cache, open, net, at, new);
    }
    changed
}

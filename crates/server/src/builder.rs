//! Building tools for creative mode: chat commands that change many blocks
//! at once. Mark two corners (`/pos1`, `/pos2`, at the block you're looking
//! at), then fill, clear, replace or wall in the box between them; copy and
//! paste whole builds; make spheres; and undo the last few changes.
//!
//! The blocks to use are given by name (`/fill stone`, `/fill oak_planks`)
//! or, left out, are whatever block you're holding.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use ourcraft_common::block::{BlockId, BlockRegistry};
use ourcraft_common::dimension::Dimension;
use ourcraft_common::physics::EYE_HEIGHT;
use ourcraft_common::protocol::{Channel, ClientMessage, MAX_CHAT_LEN, ServerMessage};
use ourcraft_common::raycast::raycast;
use ourcraft_common::world::ColumnPos;

use crate::net::{FromClient, Network};
use crate::players::{ConnectedPlayer, Players};
use crate::updates::BlockUpdates;
use crate::world::{ColumnCache, OpenWorld, Registry, ServerWorld, set_blocks};

/// The most blocks one command may change.
pub const MAX_BLOCKS: usize = 128 * 128 * 16;
/// Biggest sphere radius.
const MAX_RADIUS: i32 = 32;
/// How far away `/pos1` and friends find the block you're looking at.
const LOOK_REACH: f32 = 96.0;
/// Changes remembered for `/undo`, per player.
const UNDO_STEPS: usize = 8;
/// Bigger changes than this re-send whole columns instead of one message
/// per block.
const RESEND_OVER: usize = 512;

pub struct BuilderPlugin;

impl Plugin for BuilderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Builders>().add_systems(
            Update,
            run_builder_commands.after(crate::players::PlayersSet),
        );
    }
}

/// The commands handled here (the players module leaves these alone).
const COMMANDS: &[&str] = &[
    "/build", "/pos1", "/pos2", "/sel", "/fill", "/clear", "/replace", "/walls", "/box", "/sphere",
    "/copy", "/paste", "/undo",
];

/// Is this chat line one of the building commands?
pub fn is_command(text: &str) -> bool {
    text.split_whitespace()
        .next()
        .is_some_and(|w| COMMANDS.contains(&w))
}

/// What each builder is working with, by player name.
#[derive(Resource, Default)]
struct Builders(HashMap<String, BuilderState>);

#[derive(Default)]
struct BuilderState {
    pos1: Option<IVec3>,
    pos2: Option<IVec3>,
    /// Copied blocks, relative to where the copier stood.
    clipboard: Vec<(IVec3, BlockId)>,
    /// What changed blocks held before, newest last.
    undo: Vec<Vec<(IVec3, BlockId)>>,
}

impl BuilderState {
    /// The selected box (min and max corners), if both corners are set and
    /// in the same dimension.
    fn selection(&self) -> Result<(IVec3, IVec3), String> {
        match (self.pos1, self.pos2) {
            (Some(a), Some(b)) => {
                if Dimension::of_block_y(a.y) != Dimension::of_block_y(b.y) {
                    return Err("Your two corners are in different worlds.".into());
                }
                Ok((a.min(b), a.max(b)))
            }
            _ => Err(
                "First mark two corners: look at a block and type /pos1, then another and /pos2."
                    .into(),
            ),
        }
    }
}

fn volume(min: IVec3, max: IVec3) -> usize {
    let s = (max - min + IVec3::ONE).as_i64vec3();
    (s.x * s.y * s.z) as usize
}

fn too_big(n: usize) -> String {
    format!("That's {n} blocks, too many at once (the most is {MAX_BLOCKS}).")
}

/// A block from what someone typed: a name like `stone`, `core:stone` or
/// `oak planks`, `air`, or (nothing typed) the block they're holding.
fn block_named(
    word: Option<&str>,
    reg: &BlockRegistry,
    held: Option<BlockId>,
) -> Result<BlockId, String> {
    let Some(word) = word else {
        return held.ok_or_else(|| {
            "Hold a block, or say which one (like /fill stone, or /fill air).".to_owned()
        });
    };
    let word = word.to_lowercase();
    if word == "air" || word == "nothing" {
        return Ok(BlockId::AIR);
    }
    if word == "hand" {
        return held.ok_or_else(|| "You're not holding a block.".to_owned());
    }
    let by_name = if word.contains(':') {
        reg.by_name(&word)
    } else {
        reg.by_name(&format!("core:{word}"))
    };
    let simple = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let wanted = simple(&word);
    by_name
        .or_else(|| {
            reg.iter()
                .find(|(_, d)| {
                    simple(&d.display_name) == wanted
                        || simple(d.name.rsplit(':').next().unwrap_or("")) == wanted
                })
                .map(|(id, _)| id)
        })
        .ok_or_else(|| format!("There's no block called \"{word}\"."))
}

/// What the player is looking at (the block itself), within reach.
fn looked_at(world: &ServerWorld, reg: &BlockRegistry, p: &ConnectedPlayer) -> Option<IVec3> {
    let eye = p.position() + Vec3::Y * EYE_HEIGHT;
    let dir = Vec3::from_array(crate::mods::look_vector(p.state.yaw, p.state.pitch));
    raycast(&world.0, reg, eye, dir, LOOK_REACH).map(|hit| hit.block)
}

/// A position as the player would type it for `/tp` (their dimension's own
/// heights).
fn show(pos: IVec3) -> String {
    let dim = Dimension::of_block_y(pos.y).unwrap_or(Dimension::Overworld);
    let y = pos.y - dim.base_y();
    format!("{} {y} {}", pos.x, pos.z)
}

#[allow(clippy::too_many_arguments)]
fn run_builder_commands(
    mut messages: MessageReader<FromClient>,
    players_map: Res<Players>,
    mut players: Query<&'static mut ConnectedPlayer>,
    mut builders: ResMut<Builders>,
    open: Res<OpenWorld>,
    registry: Res<Registry>,
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    mut updates: ResMut<BlockUpdates>,
    mut net: ResMut<Network>,
) {
    for FromClient { client, msg } in messages.read() {
        let ClientMessage::Chat { text } = msg else {
            continue;
        };
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_CHAT_LEN)
            .collect();
        if !is_command(text.trim()) {
            continue;
        }
        let Some(&entity) = players_map.0.get(client) else {
            continue;
        };
        let Ok(p) = players.get(entity) else { continue };
        let Some(name) = p.name.clone() else { continue };
        let reply = if !p.creative || !p.rules.can_build {
            "The building tools work in creative mode (press G).".to_owned()
        } else {
            let held = p
                .inventory
                .held()
                .and_then(|id| open.items.get(id))
                .and_then(|def| def.block());
            let target = looked_at(&world, &registry.0, p);
            let feet = p.position().floor().as_ivec3();
            let state = builders.0.entry(name).or_default();
            let mut ctx = Ctx {
                world: &mut world,
                cache: &mut cache,
                open: &open,
                reg: &registry.0,
                updates: &mut updates,
                net: &mut net,
                players: &mut players,
            };
            match command(text.trim(), state, &mut ctx, held, target, feet) {
                Ok(s) | Err(s) => s,
            }
        };
        net.send(
            *client,
            Channel::Reliable,
            &ServerMessage::Chat {
                from: None,
                text: reply,
            },
        );
    }
}

/// Everything a command needs to change the world.
struct Ctx<'a, 'w, 's> {
    world: &'a mut ServerWorld,
    cache: &'a mut ColumnCache,
    open: &'a OpenWorld,
    reg: &'a BlockRegistry,
    updates: &'a mut BlockUpdates,
    net: &'a mut Network,
    players: &'a mut Query<'w, 's, &'static mut ConnectedPlayer>,
}

impl Ctx<'_, '_, '_> {
    /// Blocks that hold things (chests, signs, frames, banners) are left
    /// alone, and so is anything that can't be broken (bedrock).
    fn keep(&self, pos: IVec3) -> bool {
        let o = self.open;
        if o.containers.contains_key(&pos)
            || o.signs.contains_key(&pos)
            || o.frames.contains_key(&pos)
            || o.banners.contains_key(&pos)
        {
            return true;
        }
        let Some(b) = self.world.get_block(pos.x, pos.y, pos.z) else {
            return true;
        };
        !(b.is_air() || self.reg.is_breakable(b) || self.reg.is_liquid(b) || self.reg.is_soft(b))
    }

    /// Make the changes and tell everyone. Returns what was there before.
    fn apply(&mut self, changes: Vec<(IVec3, BlockId)>) -> Vec<(IVec3, BlockId)> {
        let changes: Vec<_> = changes
            .into_iter()
            .filter(|(pos, _)| !self.keep(*pos))
            .collect();
        let old = set_blocks(self.world, self.cache, self.open, changes);
        if old.len() <= RESEND_OVER {
            for (pos, _) in &old {
                let block = self.world.block(pos.x, pos.y, pos.z);
                self.net.broadcast(
                    Channel::Reliable,
                    &ServerMessage::BlockChanged {
                        pos: pos.to_array(),
                        block: block.0,
                    },
                );
            }
        } else {
            // Lots: send the changed columns again instead.
            let cols: HashSet<(Dimension, ColumnPos)> = old
                .iter()
                .filter_map(|(pos, _)| {
                    Some((
                        Dimension::of_block_y(pos.y)?,
                        ColumnPos::from_block(pos.x, pos.z),
                    ))
                })
                .collect();
            for mut p in self.players.iter_mut() {
                let dim = p.dimension;
                p.sent.retain(|c| !cols.contains(&(dim, *c)));
            }
        }
        // Water flows and sand falls where things changed.
        for (pos, _) in &old {
            self.updates.around(*pos, 1);
        }
        old
    }
}

fn remember(state: &mut BuilderState, old: Vec<(IVec3, BlockId)>) {
    if old.is_empty() {
        return;
    }
    state.undo.push(old);
    if state.undo.len() > UNDO_STEPS {
        state.undo.remove(0);
    }
}

fn changed(n: usize) -> String {
    match n {
        0 => "Nothing changed.".into(),
        1 => "Done: 1 block changed. (/undo to take it back)".into(),
        n => format!("Done: {n} blocks changed. (/undo to take it back)"),
    }
}

fn command(
    text: &str,
    state: &mut BuilderState,
    ctx: &mut Ctx,
    held: Option<BlockId>,
    target: Option<IVec3>,
    feet: IVec3,
) -> Result<String, String> {
    let mut words = text.split_whitespace();
    let cmd = words.next().unwrap_or_default();
    let arg1 = words.next();
    let arg2 = words.next();
    match cmd {
        "/build" => Ok(HELP.into()),
        "/pos1" | "/pos2" => {
            let at = target.unwrap_or(feet - IVec3::Y);
            if cmd == "/pos1" {
                state.pos1 = Some(at);
            } else {
                state.pos2 = Some(at);
            }
            let size = match state.selection() {
                Ok((min, max)) => {
                    let s = max - min + IVec3::ONE;
                    format!(
                        " The box is {}x{}x{} ({} blocks).",
                        s.x,
                        s.y,
                        s.z,
                        volume(min, max)
                    )
                }
                Err(_) => String::new(),
            };
            Ok(format!("Corner {} at {}.{size}", &cmd[4..], show(at)))
        }
        "/sel" => {
            let (min, max) = state.selection()?;
            let s = max - min + IVec3::ONE;
            Ok(format!(
                "From {} to {}: {}x{}x{} ({} blocks).",
                show(min),
                show(max),
                s.x,
                s.y,
                s.z,
                volume(min, max)
            ))
        }
        "/fill" | "/clear" | "/walls" | "/box" => {
            let block = if cmd == "/clear" {
                BlockId::AIR
            } else {
                block_named(arg1, ctx.reg, held)?
            };
            let (min, max) = state.selection()?;
            let n = volume(min, max);
            if n > MAX_BLOCKS {
                return Err(too_big(n));
            }
            let mut changes = Vec::new();
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    for x in min.x..=max.x {
                        let side = x == min.x || x == max.x || z == min.z || z == max.z;
                        let lid = y == min.y || y == max.y;
                        let wanted = match cmd {
                            "/walls" => side,
                            "/box" => side || lid,
                            _ => true,
                        };
                        if wanted {
                            changes.push((IVec3::new(x, y, z), block));
                        }
                    }
                }
            }
            let old = ctx.apply(changes);
            let msg = changed(old.len());
            remember(state, old);
            Ok(msg)
        }
        "/replace" => {
            let Some(from) = arg1 else {
                return Err(
                    "Try /replace dirt stone (or /replace dirt to use what you hold).".into(),
                );
            };
            let from = block_named(Some(from), ctx.reg, held)?;
            let to = block_named(arg2, ctx.reg, held)?;
            let (min, max) = state.selection()?;
            let n = volume(min, max);
            if n > MAX_BLOCKS {
                return Err(too_big(n));
            }
            let mut changes = Vec::new();
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    for x in min.x..=max.x {
                        if ctx.world.get_block(x, y, z) == Some(from) {
                            changes.push((IVec3::new(x, y, z), to));
                        }
                    }
                }
            }
            let old = ctx.apply(changes);
            let msg = changed(old.len());
            remember(state, old);
            Ok(msg)
        }
        "/sphere" => {
            let r: i32 = arg1
                .and_then(|w| w.parse().ok())
                .filter(|r| (1..=MAX_RADIUS).contains(r))
                .ok_or_else(|| {
                    format!("Try /sphere 5 stone: a ball 1 to {MAX_RADIUS} blocks round, where you're looking.")
                })?;
            let block = block_named(arg2, ctx.reg, held)?;
            let centre = target.ok_or("Look at the block the ball should be around.")?;
            let mut changes = Vec::new();
            let r2 = (r as f32 + 0.5).powi(2);
            for y in -r..=r {
                for z in -r..=r {
                    for x in -r..=r {
                        if (x * x + y * y + z * z) as f32 <= r2 {
                            changes.push((centre + IVec3::new(x, y, z), block));
                        }
                    }
                }
            }
            let old = ctx.apply(changes);
            let msg = changed(old.len());
            remember(state, old);
            Ok(msg)
        }
        "/copy" => {
            let (min, max) = state.selection()?;
            let n = volume(min, max);
            if n > MAX_BLOCKS {
                return Err(too_big(n));
            }
            let mut clip = Vec::with_capacity(n);
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    for x in min.x..=max.x {
                        let Some(b) = ctx.world.get_block(x, y, z) else {
                            return Err("Part of the box is too far away to copy.".into());
                        };
                        clip.push((IVec3::new(x, y, z) - feet, b));
                    }
                }
            }
            state.clipboard = clip;
            Ok(format!(
                "Copied {n} blocks. Walk somewhere and type /paste: it lands in the same place around you as it was when you copied."
            ))
        }
        "/paste" => {
            if state.clipboard.is_empty() {
                return Err("Nothing copied yet: mark a box and type /copy.".into());
            }
            // "/paste" copies everything; "/paste solid" skips the air.
            let skip_air = matches!(arg1, Some("solid" | "noair"));
            let changes = state
                .clipboard
                .iter()
                .filter(|(_, b)| !(skip_air && b.is_air()))
                .map(|(o, b)| (feet + *o, *b))
                .collect();
            let old = ctx.apply(changes);
            let msg = changed(old.len());
            remember(state, old);
            Ok(msg)
        }
        "/undo" => {
            let Some(old) = state.undo.pop() else {
                return Err("Nothing to undo.".into());
            };
            let n = ctx.apply(old).len();
            Ok(format!(
                "Undone ({n} blocks put back). {} more to undo.",
                state.undo.len()
            ))
        }
        _ => Err(HELP.into()),
    }
}

const HELP: &str = "Building tools (creative): look at a block and type /pos1, look at another and /pos2 to mark a box. \
Then /fill [block], /clear, /walls [block], /box [block] (hollow), /replace <old> [new], /copy and /paste (/paste solid skips air). \
Also /sphere <size> [block] where you look, /sel to see the box, /undo. Leave out the block to use the one you're holding.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_commands() {
        assert!(is_command("/fill stone"));
        assert!(is_command("/undo"));
        assert!(!is_command("/filler"));
        assert!(!is_command("hello /fill"));
    }

    #[test]
    fn finds_blocks_by_name() {
        let reg = BlockRegistry::with_builtin_blocks();
        let stone = reg.by_name("core:stone").unwrap();
        assert_eq!(block_named(Some("stone"), &reg, None), Ok(stone));
        assert_eq!(block_named(Some("Core:Stone"), &reg, None), Ok(stone));
        assert_eq!(block_named(Some("air"), &reg, None), Ok(BlockId::AIR));
        assert_eq!(block_named(None, &reg, Some(stone)), Ok(stone));
        assert!(block_named(None, &reg, None).is_err());
        assert!(block_named(Some("nonsense"), &reg, None).is_err());
    }

    #[test]
    fn selection_needs_both_corners() {
        let mut s = BuilderState::default();
        assert!(s.selection().is_err());
        s.pos1 = Some(IVec3::new(5, 70, 5));
        s.pos2 = Some(IVec3::new(1, 72, 9));
        let (min, max) = s.selection().unwrap();
        assert_eq!(min, IVec3::new(1, 70, 5));
        assert_eq!(max, IVec3::new(5, 72, 9));
        assert_eq!(volume(min, max), 5 * 3 * 5);
    }
}

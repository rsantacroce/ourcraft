//! # Splat Squad
//!
//! A cartoon team game for Ourcraft. Two teams, Red and Blue, each with a
//! base and a flag. Grab the other team's flag and bring it home to your own
//! flag to score. The first team to 3 captures wins the round.
//!
//! Pick a class with the badges you get when you join:
//!
//! | Class   | Paint (health) | Speed | Gadget                                   |
//! |---------|----------------|-------|------------------------------------------|
//! | Dasher  | 80             | fast  | Splat Blaster: rapid little paint pellets |
//! | Bulwark | 170            | slow  | Bubble Cannon: big, slow, heavy bubbles   |
//! | Lobber  | 110            | normal| Paint Lobber: arcing bombs that splash    |
//! | Patcher | 100            | quick | Mend Beam: refills teammates' paint       |
//!
//! Getting hit drains your paint meter. When it's empty you're *splatted*:
//! you pop in a puff of confetti and are back at your base five seconds
//! later. No blood, no gore, just paint.
//!
//! Everything here goes through the mod API. The engine doesn't know
//! anything about teams or flags.
//!
//! Start a world with this mode: `ourcraft --world Arena --mode team_shooter`
//! (or `ourcraft-server --mode team_shooter`).

use std::collections::BTreeMap;

use ourcraft_mod_api::guest::{self, Mod, hud_line};
use ourcraft_mod_api::*;

/// The game mode name worlds use to switch this mod on.
const MODE: &str = "team_shooter";
const WIN_SCORE: u32 = 3;
const TICKS_PER_SEC: u64 = 20;
const RESPAWN_SECS: u64 = 5;
const ROUND_OVER_SECS: u64 = 8;
const FLAG_RETURN_SECS: u64 = 20;
/// Blocks from the arena centre to each base.
const BASE_DISTANCE: i32 = 22;
/// Half the width of a base platform.
const BASE_HALF: i32 = 4;
/// Blocks placed per tick while building bases (keeps ticks quick).
const BUILD_PER_TICK: usize = 250;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Team {
    Red,
    Blue,
}

impl Team {
    const BOTH: [Team; 2] = [Team::Red, Team::Blue];

    fn idx(self) -> usize {
        self as usize
    }
    fn other(self) -> Team {
        match self {
            Team::Red => Team::Blue,
            Team::Blue => Team::Red,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Team::Red => "Red",
            Team::Blue => "Blue",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Team::Red => "red",
            Team::Blue => "blue",
        }
    }
    fn color(self) -> Rgba {
        match self {
            Team::Red => [235, 64, 70, 255],
            Team::Blue => [60, 130, 245, 255],
        }
    }
    fn light(self) -> Rgba {
        match self {
            Team::Red => [255, 150, 150, 255],
            Team::Blue => [150, 200, 255, 255],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Dasher,
    Bulwark,
    Lobber,
    Patcher,
}

impl Class {
    const ALL: [Class; 4] = [Class::Dasher, Class::Bulwark, Class::Lobber, Class::Patcher];

    fn name(self) -> &'static str {
        match self {
            Class::Dasher => "Dasher",
            Class::Bulwark => "Bulwark",
            Class::Lobber => "Lobber",
            Class::Patcher => "Patcher",
        }
    }
    fn badge(self) -> String {
        format!("splat:badge_{}", self.name().to_lowercase())
    }
    fn weapon(self) -> &'static str {
        match self {
            Class::Dasher => "splat:blaster",
            Class::Bulwark => "splat:bubble_cannon",
            Class::Lobber => "splat:lobber",
            Class::Patcher => "splat:mend_beam",
        }
    }
    fn max_paint(self) -> f32 {
        match self {
            Class::Dasher => 80.0,
            Class::Bulwark => 170.0,
            Class::Lobber => 110.0,
            Class::Patcher => 100.0,
        }
    }
    fn speed(self) -> f32 {
        match self {
            Class::Dasher => 1.35,
            Class::Bulwark => 0.8,
            Class::Lobber => 1.0,
            Class::Patcher => 1.1,
        }
    }
    fn blurb(self) -> &'static str {
        match self {
            Class::Dasher => "fast, with a rapid Splat Blaster",
            Class::Bulwark => "tough, with a heavy Bubble Cannon",
            Class::Lobber => "lobs paint bombs that splash",
            Class::Patcher => "refills teammates' paint with the Mend Beam",
        }
    }
    fn from_badge(tool: &str) -> Option<Class> {
        Class::ALL.into_iter().find(|c| c.badge() == tool)
    }
    fn from_name(name: &str) -> Option<Class> {
        Class::ALL
            .into_iter()
            .find(|c| c.name().eq_ignore_ascii_case(name))
    }
}

/// What a fired projectile does when it lands.
#[derive(Clone, Copy, Debug)]
struct Shot {
    owner: PlayerId,
    team: Team,
    damage: f32,
    /// Splash radius and damage at the centre (Lobber).
    splash: Option<(f32, f32)>,
}

struct Player {
    name: String,
    team: Team,
    class: Option<Class>,
    paint: f32,
    /// Tick when a splatted player comes back.
    respawn_at: Option<u64>,
    carrying: bool,
}

struct Flag {
    team: Team,
    home: Vec3,
    pos: Vec3,
    entity: Option<EntityId>,
    carrier: Option<PlayerId>,
    dropped_at: Option<u64>,
}

impl Flag {
    fn at_home(&self) -> bool {
        self.carrier.is_none() && dist2(self.pos, self.home) < 0.01
    }
}

#[derive(Default)]
struct Game {
    active: bool,
    tick: u64,
    seed: u64,
    players: BTreeMap<PlayerId, Player>,
    score: [u32; 2],
    /// Base centres (block on top of the platform), once chosen.
    bases: Option<[[i32; 3]; 2]>,
    flags: Vec<Flag>,
    /// Blocks still to place for the bases.
    to_build: Vec<([i32; 3], BlockId)>,
    shots: BTreeMap<EntityId, Shot>,
    round_over_until: Option<u64>,
    /// Latest player positions (refreshed every tick).
    seen: BTreeMap<PlayerId, PlayerInfo>,
}

fn dist2(a: Vec3, b: Vec3) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

impl Mod for Game {
    fn on_event(&mut self, event: Event) -> EventReply {
        match event {
            Event::Init { game_mode, .. } => {
                // Always register content, so block and tool ids are the same
                // in every world; only run the game in team_shooter worlds.
                register_content();
                self.active = game_mode.as_deref() == Some(MODE);
                if self.active {
                    guest::log("Splat Squad is on! Teams, flags and paint at the ready.");
                }
                EventReply::Ok
            }
            _ if !self.active => EventReply::Ok,
            Event::Start => {
                // The arena is always around the world's spawn point, so it's
                // in the same place every time the server starts.
                if let Some(s) = guest::world_spawn() {
                    self.setup_bases([s[0] as f32, s[1] as f32, s[2] as f32]);
                }
                EventReply::Ok
            }
            Event::PlayerJoined { player, name } => {
                self.join(player, name);
                EventReply::Ok
            }
            Event::PlayerLeft { player } => {
                self.drop_flag(player);
                self.players.remove(&player);
                EventReply::Ok
            }
            Event::UseTool {
                player,
                tool,
                origin,
                direction,
                ..
            } => {
                if let Some(class) = Class::from_badge(&tool) {
                    self.pick_class(player, class);
                } else {
                    self.fire(player, origin, direction);
                }
                EventReply::Ok
            }
            Event::ProjectileHit { entity, hit, .. } => {
                self.projectile_hit(entity, hit);
                EventReply::Ok
            }
            Event::Tick { tick, .. } => {
                self.tick = tick;
                self.update();
                EventReply::Ok
            }
            // The arena stays as it is: no building or breaking.
            Event::BlockChange { .. } => EventReply::Deny,
            Event::Chat { player, text } => self.command(player, &text),
            _ => EventReply::Ok,
        }
    }
}

impl Game {
    fn random(&mut self) -> f32 {
        self.seed = self
            .seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407 ^ self.tick);
        ((self.seed >> 33) % 10_000) as f32 / 10_000.0
    }

    fn secs(&self, s: u64) -> u64 {
        self.tick + s * TICKS_PER_SEC
    }

    // ---- Joining and classes ----

    fn join(&mut self, player: PlayerId, name: String) {
        // Join the smaller team.
        let reds = self
            .players
            .values()
            .filter(|p| p.team == Team::Red)
            .count();
        let blues = self.players.len() - reds;
        let team = if reds <= blues { Team::Red } else { Team::Blue };
        self.players.insert(
            player,
            Player {
                name: name.clone(),
                team,
                class: None,
                paint: 0.0,
                respawn_at: None,
                carrying: false,
            },
        );
        if self.bases.is_none()
            && let Some(p) = guest::players().into_iter().find(|p| p.id == player)
        {
            self.setup_bases(p.position);
        }
        self.apply_rules(player);
        guest::set_health(player, 0.0, 0.0);
        let badges = Class::ALL.iter().map(|c| c.badge()).collect();
        guest::set_tools(player, badges);
        self.send_to_base(player);
        guest::show_title(
            Some(player),
            &format!("You're on {} team!", team.name()),
            4.0,
        );
        guest::chat(
            Some(player),
            "Welcome to Splat Squad! Pick a class: select a badge (1-4) and click. \
             Grab the other team's flag and bring it to yours!",
        );
        guest::chat(None, &format!("{name} joined {} team", team.name()));
    }

    fn pick_class(&mut self, player: PlayerId, class: Class) {
        let Some(p) = self.players.get_mut(&player) else {
            return;
        };
        p.class = Some(class);
        p.paint = class.max_paint();
        p.respawn_at = None;
        guest::set_health(player, p.paint, class.max_paint());
        guest::set_tools(player, vec![class.weapon().into()]);
        guest::show_title(
            Some(player),
            &format!("{}: {}", class.name(), class.blurb()),
            3.5,
        );
        self.apply_rules(player);
        self.send_to_base(player);
    }

    fn apply_rules(&self, player: PlayerId) {
        let Some(p) = self.players.get(&player) else {
            return;
        };
        let speed = match (p.class, p.respawn_at) {
            (_, Some(_)) => 0.0,
            (Some(c), None) => c.speed(),
            (None, None) => 1.0,
        };
        guest::set_rules(
            player,
            PlayerRules {
                speed,
                can_fly: false,
                can_build: false,
                color: Some(p.team.color()),
                tag: Some(format!("[{}]", p.team.name())),
            },
        );
    }

    fn send_to_base(&mut self, player: PlayerId) {
        let Some(team) = self.players.get(&player).map(|p| p.team) else {
            return;
        };
        let Some(bases) = self.bases else { return };
        let b = bases[team.idx()];
        let (dx, dz) = (self.random() * 4.0 - 2.0, self.random() * 4.0 - 2.0);
        guest::teleport(
            player,
            [b[0] as f32 + 0.5 + dx, b[1] as f32, b[2] as f32 + 0.5 + dz],
        );
    }

    // ---- The arena ----

    fn setup_bases(&mut self, around: Vec3) {
        let (cx, cz) = (around[0].floor() as i32, around[2].floor() as i32);
        let mut bases = [[0; 3]; 2];
        for team in Team::BOTH {
            let x = cx
                + if team == Team::Red {
                    -BASE_DISTANCE
                } else {
                    BASE_DISTANCE
                };
            let ground = guest::ground_height(x, cz).unwrap_or(64);
            bases[team.idx()] = [x, ground + 1, cz];
            self.plan_base(team, x, ground, cz);
            let home = [x as f32 + 0.5, (ground + 1) as f32, cz as f32 + 0.5];
            let entity = guest::spawn_entity(EntitySpec {
                model: format!("splat:flag_{}", team.key()),
                position: home,
                velocity: [0.0; 3],
                motion: EntityMotion::Fixed,
                lifetime: 0.0,
                owner: None,
            });
            self.flags.push(Flag {
                team,
                home,
                pos: home,
                entity,
                carrier: None,
                dropped_at: None,
            });
        }
        self.bases = Some(bases);
        guest::log("Bases placed; building them…");
    }

    /// Queue the blocks of a base: a platform in team colours, cleared space
    /// above it, a low wall with gaps, and lamps on the corners.
    fn plan_base(&mut self, team: Team, x: i32, ground: i32, z: i32) {
        let pick = |names: &[&str]| names.iter().find_map(|n| guest::block_id(n)).unwrap_or(1);
        let floor = match team {
            Team::Red => pick(&["paint:red", "core:bricks"]),
            Team::Blue => pick(&["paint:blue", "core:cobblestone"]),
        };
        let trim = pick(&["paint:white", "core:planks"]);
        let lamp = pick(&["core:lamp"]);
        let support = pick(&["core:stone"]);
        let r = BASE_HALF;
        for dx in -r..=r {
            for dz in -r..=r {
                let (bx, bz) = (x + dx, z + dz);
                let edge = dx.abs() == r || dz.abs() == r;
                let gap = dx.abs() <= 1 || dz.abs() <= 1;
                // Fill under the platform so it doesn't float on slopes.
                for y in (ground - 3)..ground {
                    self.to_build.push(([bx, y, bz], support));
                }
                let centre = dx == 0 && dz == 0;
                self.to_build
                    .push(([bx, ground, bz], if centre { lamp } else { floor }));
                for y in (ground + 1)..(ground + 7) {
                    let wall = edge && !gap && y == ground + 1;
                    let corner_lamp = dx.abs() == r && dz.abs() == r && y == ground + 2;
                    let block = if wall {
                        trim
                    } else if corner_lamp {
                        lamp
                    } else {
                        0 // air: clear trees and hills off the base
                    };
                    self.to_build.push(([bx, y, bz], block));
                }
            }
        }
    }

    fn build_some(&mut self) {
        let mut placed = 0;
        let mut later = Vec::new();
        while let Some((pos, block)) = self.to_build.pop() {
            if placed >= BUILD_PER_TICK {
                later.push((pos, block));
                continue;
            }
            if guest::get_block(pos) == block {
                continue;
            }
            if guest::try_set_block(pos, block) {
                placed += 1;
            } else {
                later.push((pos, block)); // not loaded yet
            }
        }
        self.to_build = later;
    }

    // ---- Shooting ----

    fn fire(&mut self, player: PlayerId, origin: Vec3, dir: Vec3) {
        let Some(p) = self.players.get(&player) else {
            return;
        };
        let (Some(class), None) = (p.class, p.respawn_at) else {
            return;
        };
        if self.round_over_until.is_some() {
            return;
        }
        let team = p.team;
        // (model, speed, gravity, radius, lifetime, damage, splash)
        let (model, speed, gravity, radius, life, damage, splash) = match class {
            Class::Dasher => ("pellet", 38.0, 2.0, 0.15, 2.0, 16.0, None),
            Class::Bulwark => ("bubble", 20.0, 4.0, 0.45, 3.0, 42.0, None),
            Class::Lobber => ("bomb", 17.0, 16.0, 0.3, 4.0, 30.0, Some((3.0, 40.0))),
            Class::Patcher => return self.mend_beam(player, team, origin, dir),
        };
        let start = add(origin, scale(dir, 0.6));
        let Some(id) = guest::spawn_entity(EntitySpec {
            model: format!("splat:{model}_{}", team.key()),
            position: start,
            velocity: scale(dir, speed),
            motion: EntityMotion::Projectile {
                gravity,
                radius,
                hits_players: true,
            },
            lifetime: life,
            owner: Some(player),
        }) else {
            return;
        };
        self.shots.insert(
            id,
            Shot {
                owner: player,
                team,
                damage,
                splash,
            },
        );
        guest::sound("pop", Some(start));
    }

    /// The Patcher's beam: instantly refills a teammate's paint, or gives an
    /// opponent a little splash.
    fn mend_beam(&mut self, player: PlayerId, team: Team, origin: Vec3, dir: Vec3) {
        match guest::raycast(origin, dir, 14.0, true, Some(player)) {
            Some(Hit::Player { id, at }) => {
                let friend = self.players.get(&id).is_some_and(|p| p.team == team);
                if friend {
                    self.heal(id, 9.0);
                    guest::effect(EffectKind::Splat, at, [120, 230, 120, 255]);
                } else {
                    self.damage(id, 5.0, player, at);
                }
            }
            Some(Hit::Block { pos, normal }) => {
                let at = [
                    pos[0] as f32 + 0.5 + normal[0] as f32 * 0.55,
                    pos[1] as f32 + 0.5 + normal[1] as f32 * 0.55,
                    pos[2] as f32 + 0.5 + normal[2] as f32 * 0.55,
                ];
                guest::effect(EffectKind::Splat, at, [120, 230, 120, 255]);
            }
            None => {}
        }
    }

    fn projectile_hit(&mut self, entity: EntityId, hit: Hit) {
        let Some(shot) = self.shots.remove(&entity) else {
            return;
        };
        let at = match hit {
            Hit::Player { id, at } => {
                self.damage(id, shot.damage, shot.owner, at);
                at
            }
            Hit::Block { pos, normal } => [
                pos[0] as f32 + 0.5 + normal[0] as f32 * 0.55,
                pos[1] as f32 + 0.5 + normal[1] as f32 * 0.55,
                pos[2] as f32 + 0.5 + normal[2] as f32 * 0.55,
            ],
        };
        guest::effect(EffectKind::Splat, at, shot.team.color());
        if let Some((radius, max)) = shot.splash {
            let targets: Vec<(PlayerId, Vec3)> = self
                .seen
                .values()
                .map(|p| (p.id, add(p.position, [0.0, 0.9, 0.0])))
                .collect();
            for (id, centre) in targets {
                let d = dist2(centre, at).sqrt();
                if d < radius {
                    self.damage(id, max * (1.0 - d / radius), shot.owner, centre);
                }
            }
            guest::effect(EffectKind::Splat, at, shot.team.light());
        }
    }

    fn heal(&mut self, player: PlayerId, amount: f32) {
        let Some(p) = self.players.get_mut(&player) else {
            return;
        };
        let (Some(class), None) = (p.class, p.respawn_at) else {
            return;
        };
        p.paint = (p.paint + amount).min(class.max_paint());
        guest::set_health(player, p.paint, class.max_paint());
    }

    fn damage(&mut self, target: PlayerId, amount: f32, attacker: PlayerId, at: Vec3) {
        let attacker_team = self.players.get(&attacker).map(|p| p.team);
        let Some(p) = self.players.get_mut(&target) else {
            return;
        };
        // No friendly fire, and no hitting players who are respawning or
        // still choosing a class.
        if Some(p.team) == attacker_team || p.respawn_at.is_some() || p.class.is_none() {
            return;
        }
        let max = p.class.map_or(100.0, |c| c.max_paint());
        p.paint -= amount;
        guest::set_health(target, p.paint.max(0.0), max);
        if p.paint <= 0.0 {
            self.splat_out(target, attacker, at);
        }
    }

    /// Out of paint: pop, drop any flag, and wait at base to come back.
    fn splat_out(&mut self, target: PlayerId, attacker: PlayerId, at: Vec3) {
        guest::effect(EffectKind::Poof, at, [255, 255, 255, 255]);
        guest::effect(EffectKind::Confetti, at, [255, 255, 255, 255]);
        guest::sound("splat", Some(at));
        self.drop_flag(target);
        let respawn = self.secs(RESPAWN_SECS);
        let names = (
            self.players.get(&attacker).map(|p| p.name.clone()),
            self.players.get(&target).map(|p| p.name.clone()),
        );
        if let (Some(a), Some(t)) = names {
            guest::chat(None, &format!("{a} splatted {t}!"));
        }
        if let Some(p) = self.players.get_mut(&target) {
            p.respawn_at = Some(respawn);
            p.paint = 0.0;
        }
        guest::show_title(
            Some(target),
            &format!("Splatted! Back in {RESPAWN_SECS}..."),
            1.5,
        );
        self.apply_rules(target);
        self.send_to_base(target);
    }

    fn respawn(&mut self, player: PlayerId) {
        let Some(p) = self.players.get_mut(&player) else {
            return;
        };
        p.respawn_at = None;
        let Some(class) = p.class else { return };
        p.paint = class.max_paint();
        guest::set_health(player, p.paint, class.max_paint());
        guest::show_title(Some(player), "Go!", 1.0);
        self.apply_rules(player);
        self.send_to_base(player);
    }

    // ---- Flags ----

    fn drop_flag(&mut self, player: PlayerId) {
        let pos = self.seen.get(&player).map(|p| p.position);
        for flag in &mut self.flags {
            if flag.carrier == Some(player) {
                flag.carrier = None;
                flag.dropped_at = Some(self.tick);
                if let Some(pos) = pos {
                    flag.pos = pos;
                }
                if let Some(e) = flag.entity {
                    guest::move_entity(e, flag.pos);
                }
                guest::chat(None, &format!("The {} flag was dropped!", flag.team.name()));
            }
        }
        if let Some(p) = self.players.get_mut(&player) {
            p.carrying = false;
        }
    }

    fn return_flag(&mut self, team: Team) {
        if let Some(flag) = self.flags.iter_mut().find(|f| f.team == team) {
            if let Some(c) = flag.carrier.take()
                && let Some(p) = self.players.get_mut(&c)
            {
                p.carrying = false;
            }
            flag.pos = flag.home;
            flag.dropped_at = None;
            if let Some(e) = flag.entity {
                guest::move_entity(e, flag.home);
            }
        }
    }

    fn update_flags(&mut self) {
        for i in 0..self.flags.len() {
            let team = self.flags[i].team;
            if let Some(carrier) = self.flags[i].carrier {
                // Carried: float above the carrier's head.
                let Some(info) = self.seen.get(&carrier) else {
                    self.drop_flag(carrier);
                    continue;
                };
                let pos = add(info.position, [0.0, 2.1, 0.0]);
                self.flags[i].pos = pos;
                if let Some(e) = self.flags[i].entity {
                    guest::move_entity(e, pos);
                }
                // Capture: the carrier reaches their own flag, and it's home.
                let carrier_team = team.other();
                let own = &self.flags[carrier_team.idx()];
                let near_home = dist2(info.position, own.home) < 2.5 * 2.5;
                if near_home && own.at_home() {
                    self.capture(carrier, carrier_team, team);
                }
                continue;
            }
            // On the ground: touched by someone?
            let flag_pos = self.flags[i].pos;
            let touching: Vec<PlayerId> = self
                .seen
                .values()
                .filter(|p| dist2(p.position, flag_pos) < 1.6 * 1.6)
                .map(|p| p.id)
                .collect();
            for id in touching {
                let Some(p) = self.players.get(&id) else {
                    continue;
                };
                if p.respawn_at.is_some() || p.class.is_none() {
                    continue;
                }
                if p.team != team && !p.carrying {
                    let name = p.name.clone();
                    self.flags[i].carrier = Some(id);
                    self.flags[i].dropped_at = None;
                    if let Some(p) = self.players.get_mut(&id) {
                        p.carrying = true;
                    }
                    guest::chat(None, &format!("{name} grabbed the {} flag!", team.name()));
                    guest::show_title(Some(id), "You've got the flag! Run home!", 2.5);
                    guest::sound("grab", None);
                    break;
                } else if p.team == team && !self.flags[i].at_home() {
                    let name = p.name.clone();
                    self.return_flag(team);
                    guest::chat(None, &format!("{name} returned the {} flag", team.name()));
                    break;
                }
            }
            // Left lying around too long: back home it goes.
            if let Some(t) = self.flags[i].dropped_at
                && self.tick > t + FLAG_RETURN_SECS * TICKS_PER_SEC
            {
                self.return_flag(team);
                guest::chat(None, &format!("The {} flag went home", team.name()));
            }
        }
    }

    fn capture(&mut self, carrier: PlayerId, scorer: Team, flag_team: Team) {
        self.return_flag(flag_team);
        self.score[scorer.idx()] += 1;
        let name = self
            .players
            .get(&carrier)
            .map_or("Someone".into(), |p| p.name.clone());
        let (r, b) = (self.score[0], self.score[1]);
        if let Some(bases) = self.bases {
            let base = bases[scorer.idx()];
            let at = [
                base[0] as f32 + 0.5,
                base[1] as f32 + 1.5,
                base[2] as f32 + 0.5,
            ];
            guest::effect(EffectKind::Confetti, at, scorer.color());
        }
        guest::sound("cheer", None);
        if self.score[scorer.idx()] >= WIN_SCORE {
            guest::show_title(
                None,
                &format!("{} team wins! ({r} - {b})", scorer.name()),
                6.0,
            );
            guest::chat(None, &format!("{} team wins the round!", scorer.name()));
            self.round_over_until = Some(self.secs(ROUND_OVER_SECS));
        } else {
            guest::show_title(
                None,
                &format!("{name} scores for {}! ({r} - {b})", scorer.name()),
                3.0,
            );
        }
    }

    fn new_round(&mut self) {
        self.round_over_until = None;
        self.score = [0, 0];
        for team in Team::BOTH {
            self.return_flag(team);
        }
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            if let Some(p) = self.players.get_mut(&id) {
                p.respawn_at = None;
                p.carrying = false;
            }
            self.respawn(id);
        }
        guest::show_title(None, "New round! Go!", 2.0);
    }

    // ---- Every tick ----

    fn update(&mut self) {
        self.seen = guest::players().into_iter().map(|p| (p.id, p)).collect();
        if !self.to_build.is_empty() {
            self.build_some();
        }
        if let Some(until) = self.round_over_until {
            if self.tick >= until {
                self.new_round();
            }
            return;
        }
        // Respawns, with a countdown.
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            let Some(at) = self.players.get(&id).and_then(|p| p.respawn_at) else {
                continue;
            };
            if self.tick >= at {
                self.respawn(id);
            } else if (at - self.tick).is_multiple_of(TICKS_PER_SEC) {
                let left = (at - self.tick) / TICKS_PER_SEC;
                guest::show_title(Some(id), &format!("Splatted! Back in {left}..."), 1.2);
            }
        }
        self.update_flags();
        if self.tick.is_multiple_of(10) {
            self.update_hud();
        }
    }

    fn update_hud(&self) {
        let yellow = [255, 220, 90, 255];
        for (&id, p) in &self.players {
            let flag = |team: Team| {
                let f = self.flags.iter().find(|f| f.team == team);
                match f {
                    Some(f) if f.carrier.is_some() => "taken!",
                    Some(f) if !f.at_home() => "dropped",
                    _ => "safe at base",
                }
            };
            let mut lines = vec![
                HudLine {
                    text: "SPLAT SQUAD".into(),
                    color: yellow,
                },
                HudLine {
                    text: format!("Red {}", self.score[0]),
                    color: Team::Red.light(),
                },
                HudLine {
                    text: format!("Blue {}", self.score[1]),
                    color: Team::Blue.light(),
                },
                hud_line(format!("First to {WIN_SCORE} captures")),
                HudLine {
                    text: format!(
                        "You: {} {}",
                        p.team.name(),
                        p.class.map_or("(pick a class)", |c| c.name())
                    ),
                    color: p.team.light(),
                },
                hud_line(format!("Your flag: {}", flag(p.team))),
            ];
            if p.carrying {
                lines.push(HudLine {
                    text: "Carrying the flag: run home!".into(),
                    color: yellow,
                });
            }
            guest::set_hud(Some(id), lines);
        }
    }

    // ---- Chat commands ----

    fn command(&mut self, player: PlayerId, text: &str) -> EventReply {
        let mut words = text.split_whitespace();
        match words.next() {
            Some("/class") => {
                match words.next().and_then(Class::from_name) {
                    Some(c) => self.pick_class(player, c),
                    None => guest::chat(
                        Some(player),
                        "Classes: dasher, bulwark, lobber, patcher. Example: /class lobber",
                    ),
                }
                EventReply::Handled
            }
            Some("/team") => {
                self.drop_flag(player);
                if let Some(p) = self.players.get_mut(&player) {
                    p.team = p.team.other();
                    let msg = format!("{} switched to {} team", p.name, p.team.name());
                    guest::chat(None, &msg);
                }
                self.apply_rules(player);
                self.send_to_base(player);
                EventReply::Handled
            }
            Some("/score") => {
                let msg = format!("Red {} - Blue {}", self.score[0], self.score[1]);
                guest::chat(Some(player), &msg);
                EventReply::Handled
            }
            Some("/help") => {
                guest::chat(
                    Some(player),
                    "Splat Squad: /class NAME, /team (switch), /score. Grab their flag, bring it home!",
                );
                EventReply::Ok // let the server add its own help too
            }
            _ => EventReply::Ok,
        }
    }
}

// ---- Content: models, tools and their icons ----

fn register_content() {
    for team in Team::BOTH {
        let c = team.color();
        let l = team.light();
        let k = team.key();
        guest::register_model(ModelSpec {
            name: format!("splat:flag_{k}"),
            boxes: vec![
                ModelBox {
                    min: [-0.06, 0.0, -0.06],
                    max: [0.06, 2.3, 0.06],
                    color: [120, 90, 60, 255],
                },
                ModelBox {
                    min: [0.06, 1.55, -0.03],
                    max: [0.95, 2.2, 0.03],
                    color: c,
                },
                ModelBox {
                    min: [-0.1, 2.3, -0.1],
                    max: [0.1, 2.5, 0.1],
                    color: [255, 215, 80, 255],
                },
            ],
            spin: false,
        });
        guest::register_model(ModelSpec {
            name: format!("splat:pellet_{k}"),
            boxes: vec![ModelBox {
                min: [-0.12; 3],
                max: [0.12; 3],
                color: c,
            }],
            spin: true,
        });
        guest::register_model(ModelSpec {
            name: format!("splat:bubble_{k}"),
            boxes: vec![
                ModelBox {
                    min: [-0.3; 3],
                    max: [0.3; 3],
                    color: l,
                },
                ModelBox {
                    min: [-0.12, 0.12, -0.32],
                    max: [0.0, 0.24, -0.3],
                    color: [255, 255, 255, 255],
                },
            ],
            spin: true,
        });
        guest::register_model(ModelSpec {
            name: format!("splat:bomb_{k}"),
            boxes: vec![
                ModelBox {
                    min: [-0.2; 3],
                    max: [0.2; 3],
                    color: [60, 60, 70, 255],
                },
                ModelBox {
                    min: [-0.21, -0.06, -0.21],
                    max: [0.21, 0.06, 0.21],
                    color: c,
                },
            ],
            spin: true,
        });
    }

    let tool = |name: &str, display: &str, icon: Vec<u8>, cooldown: f32| {
        guest::register_tool(ToolSpec {
            name: name.into(),
            display_name: display.into(),
            icon,
            cooldown,
        });
    };
    tool("splat:blaster", "Splat Blaster", icon_blaster(), 0.15);
    tool(
        "splat:bubble_cannon",
        "Bubble Cannon",
        icon_bubble_cannon(),
        0.8,
    );
    tool("splat:lobber", "Paint Lobber", icon_lobber(), 1.0);
    tool("splat:mend_beam", "Mend Beam", icon_mend_beam(), 0.12);
    tool("splat:badge_dasher", "Dasher (fast)", icon_badge('D'), 0.5);
    tool(
        "splat:badge_bulwark",
        "Bulwark (tough)",
        icon_badge('B'),
        0.5,
    );
    tool(
        "splat:badge_lobber",
        "Lobber (splash)",
        icon_badge('L'),
        0.5,
    );
    tool(
        "splat:badge_patcher",
        "Patcher (helper)",
        icon_badge('P'),
        0.5,
    );
}

/// A cartoon toy blaster: yellow body, pink paint tank, red nozzle.
fn icon_blaster() -> Vec<u8> {
    pixel_art(
        &[
            "................",
            "................",
            "....KKKKKK......",
            "...KPPPPPPK.....",
            "...KPHHPPPK.....",
            "...KPPPPPPK.....",
            ".KKKKKKKKKKKKK..",
            "KYWWWYYYYYYYYKK.",
            "KYYYYYYYYYYYYRRK",
            "KOOOOOOOOOOOOKK.",
            ".KKKKOOOKKKKK...",
            "....KOYK........",
            "....KOYK........",
            "....KOYK........",
            "....KKKK........",
            "................",
        ],
        &[
            ('K', [45, 35, 55, 255]),
            ('Y', [255, 210, 60, 255]),
            ('W', [255, 246, 200, 255]),
            ('O', [232, 140, 40, 255]),
            ('P', [255, 95, 165, 255]),
            ('H', [255, 205, 230, 255]),
            ('R', [225, 60, 60, 255]),
        ],
    )
}

/// A chunky blue cannon on wheels, blowing bubbles.
fn icon_bubble_cannon() -> Vec<u8> {
    pixel_art(
        &[
            "................",
            "...........WW...",
            "..........W..W..",
            "...........WW...",
            "..KKKKKKKKKK..W.",
            ".KBBBBBBBBBBKK..",
            ".KBLLLBBBBBBDDK.",
            ".KBBBBBBBBBBDDK.",
            ".KDDDDDDDDDDKK..",
            "..KKKKKKKKKK....",
            "...KGGK..KGGK...",
            "...KGGK..KGGK...",
            "...KKKK..KKKK...",
            "................",
            "................",
            "................",
        ],
        &[
            ('K', [45, 35, 55, 255]),
            ('B', [90, 150, 245, 255]),
            ('L', [190, 220, 255, 255]),
            ('D', [50, 95, 190, 255]),
            ('W', [215, 240, 255, 255]),
            ('G', [110, 110, 125, 255]),
        ],
    )
}

/// A round paint bomb with a pink stripe and a sparkly fuse.
fn icon_lobber() -> Vec<u8> {
    pixel_art(
        &[
            "................",
            "..........Y.Y...",
            "...........Y....",
            "..........S.....",
            ".....KKKKKS.....",
            "....KMMMMMK.....",
            "...KMLMMMMMK....",
            "..KMLMMMMMMMK...",
            "..KPPPPPPPPPK...",
            "..KPPPPPPPPPK...",
            "..KMMMMMMMMMK...",
            "...KMMMMMMMK....",
            "....KMMMMMK.....",
            ".....KKKKK......",
            "................",
            "................",
        ],
        &[
            ('K', [45, 35, 55, 255]),
            ('M', [75, 75, 95, 255]),
            ('L', [160, 160, 185, 255]),
            ('P', [255, 95, 165, 255]),
            ('S', [170, 130, 90, 255]),
            ('Y', [255, 220, 80, 255]),
        ],
    )
}

/// A wand topped with a green heart.
fn icon_mend_beam() -> Vec<u8> {
    pixel_art(
        &[
            "......W.........",
            ".........KK.KK..",
            "........KGGKGGK.",
            "........KGWGGGK.",
            "........KGGGGGK.",
            ".........KGGGK..",
            "..........KGK...",
            "........SSK.....",
            ".......SS.......",
            "......SS........",
            ".....SS.........",
            "....SS..........",
            "...SS...........",
            "..KK............",
            "................",
            "................",
        ],
        &[
            ('K', [45, 35, 55, 255]),
            ('G', [100, 220, 120, 255]),
            ('W', [240, 255, 240, 255]),
            ('S', [215, 180, 120, 255]),
        ],
    )
}

/// A round badge with a big letter.
fn icon_badge(letter: char) -> Vec<u8> {
    let glyph: [&str; 7] = match letter {
        'D' => ["XXX.", "X..X", "X..X", "X..X", "X..X", "X..X", "XXX."],
        'B' => ["XXX.", "X..X", "X..X", "XXX.", "X..X", "X..X", "XXX."],
        'L' => ["X...", "X...", "X...", "X...", "X...", "X...", "XXXX"],
        _ => ["XXX.", "X..X", "X..X", "XXX.", "X...", "X...", "X..."],
    };
    let mut rows: Vec<String> = Vec::new();
    for y in 0..16i32 {
        let mut row = String::new();
        for x in 0..16i32 {
            let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
            let d = (dx * dx + dy * dy).sqrt();
            let gx = x - 6;
            let gy = y - 4;
            let in_glyph = (0..4).contains(&gx)
                && (0..7).contains(&gy)
                && glyph[gy as usize].as_bytes()[gx as usize] == b'X';
            row.push(if d > 7.6 {
                '.'
            } else if d > 6.4 {
                'R'
            } else if in_glyph {
                'W'
            } else {
                'F'
            });
        }
        rows.push(row);
    }
    let rows: [&str; 16] = core::array::from_fn(|i| rows[i].as_str());
    pixel_art(
        &rows,
        &[
            ('R', [255, 200, 60, 255]),
            ('F', [90, 80, 160, 255]),
            ('W', [255, 255, 255, 255]),
        ],
    )
}

ourcraft_mod_api::export_mod!(Game);

#[cfg(test)]
mod tests {
    //! The game rules, tested natively (calls to the game do nothing here).
    use super::*;

    fn info(id: PlayerId, pos: Vec3) -> PlayerInfo {
        PlayerInfo {
            id,
            name: format!("p{id}"),
            position: pos,
            velocity: [0.0; 3],
            look: [0.0, 0.0, -1.0],
            on_ground: true,
        }
    }

    /// Red player 1 and blue player 2, flags at x = -20 (red) and 20 (blue).
    fn game() -> Game {
        let mut g = Game {
            active: true,
            bases: Some([[-20, 64, 0], [20, 64, 0]]),
            ..Default::default()
        };
        for (team, x) in [(Team::Red, -20.5), (Team::Blue, 20.5)] {
            g.flags.push(Flag {
                team,
                home: [x, 64.0, 0.5],
                pos: [x, 64.0, 0.5],
                entity: None,
                carrier: None,
                dropped_at: None,
            });
        }
        for (id, team, class) in [
            (1, Team::Red, Class::Dasher),
            (2, Team::Blue, Class::Bulwark),
        ] {
            g.players.insert(
                id,
                Player {
                    name: format!("p{id}"),
                    team,
                    class: Some(class),
                    paint: class.max_paint(),
                    respawn_at: None,
                    carrying: false,
                },
            );
        }
        g
    }

    #[test]
    fn grab_carry_and_capture_scores() {
        let mut g = game();
        // Blue player walks onto the red flag: picks it up.
        g.seen.insert(2, info(2, [-20.5, 64.0, 0.5]));
        g.update_flags();
        assert_eq!(g.flags[0].carrier, Some(2));
        assert!(g.players[&2].carrying);
        // Carries it home to the blue flag: Blue scores, red flag goes home.
        g.seen.insert(2, info(2, [20.0, 64.0, 0.5]));
        g.update_flags();
        assert_eq!(g.score, [0, 1]);
        assert!(g.flags[0].at_home());
        assert!(!g.players[&2].carrying);
    }

    #[test]
    fn own_flag_is_not_picked_up() {
        let mut g = game();
        g.seen.insert(1, info(1, [-20.5, 64.0, 0.5]));
        g.update_flags();
        assert_eq!(g.flags[0].carrier, None);
    }

    #[test]
    fn no_capture_while_your_own_flag_is_taken() {
        let mut g = game();
        g.flags[0].carrier = Some(2); // Blue has the red flag…
        g.players.get_mut(&2).unwrap().carrying = true;
        g.flags[1].carrier = Some(1); // …but Red has the blue flag.
        g.players.get_mut(&1).unwrap().carrying = true;
        g.seen.insert(2, info(2, [20.0, 64.0, 0.5]));
        g.seen.insert(1, info(1, [0.0, 64.0, 0.5]));
        g.update_flags();
        assert_eq!(g.score, [0, 0]);
    }

    #[test]
    fn third_capture_wins_the_round() {
        let mut g = game();
        g.score = [0, WIN_SCORE - 1];
        g.capture(2, Team::Blue, Team::Red);
        assert_eq!(g.score[1], WIN_SCORE);
        assert!(g.round_over_until.is_some());
        g.tick = g.round_over_until.unwrap();
        g.update();
        assert_eq!(g.score, [0, 0], "a new round starts");
    }

    #[test]
    fn running_out_of_paint_splats_and_drops_the_flag() {
        let mut g = game();
        g.flags[0].carrier = Some(2);
        g.players.get_mut(&2).unwrap().carrying = true;
        g.seen.insert(2, info(2, [0.0, 64.0, 0.5]));
        g.damage(2, 500.0, 1, [0.0, 65.0, 0.5]);
        let p = &g.players[&2];
        assert!(p.respawn_at.is_some());
        assert!(!p.carrying);
        assert_eq!(g.flags[0].carrier, None, "flag dropped");
        assert_eq!(g.flags[0].pos, [0.0, 64.0, 0.5]);
        // Back after the respawn time, with a full paint meter.
        g.tick = p.respawn_at.unwrap();
        g.update();
        let p = &g.players[&2];
        assert!(p.respawn_at.is_none());
        assert_eq!(p.paint, Class::Bulwark.max_paint());
    }

    #[test]
    fn no_friendly_fire() {
        let mut g = game();
        g.players.insert(
            3,
            Player {
                name: "p3".into(),
                team: Team::Red,
                class: Some(Class::Lobber),
                paint: 110.0,
                respawn_at: None,
                carrying: false,
            },
        );
        g.damage(3, 50.0, 1, [0.0; 3]);
        assert_eq!(g.players[&3].paint, 110.0);
    }

    #[test]
    fn patcher_heals_up_to_the_maximum() {
        let mut g = game();
        g.players.get_mut(&1).unwrap().paint = 10.0;
        g.heal(1, 9.0);
        assert_eq!(g.players[&1].paint, 19.0);
        g.heal(1, 500.0);
        assert_eq!(g.players[&1].paint, Class::Dasher.max_paint());
    }
}

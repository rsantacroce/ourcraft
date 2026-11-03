//! # Build Contest
//!
//! A building game for Ourcraft. Everyone gets a plot of their own and the
//! same secret theme ("A rocket!", "A house for a cat"…). When the time
//! is up, everyone visits each plot in turn and votes on it with the vote
//! badges in their hotbar, from "Meh" to "WOW!". The most points wins.
//!
//! - `/start` starts a round (`/start 3` for three minutes of building).
//!   With nobody typing it, a round starts a minute after someone joins.
//! - `/theme` says the theme again.
//!
//! During a round you can only build on your own plot. Everything goes
//! through the mod API: the engine knows nothing about plots or votes.
//!
//! Start a world with this mode: pick "Game: Build Contest" when making a
//! world, or `ourcraft-server --mode build_contest`.

use std::collections::BTreeMap;

use ourcraft_mod_api::guest::{self, Mod, hud_line};
use ourcraft_mod_api::*;

/// The game mode name worlds use to switch this mod on.
const MODE: &str = "build_contest";
const TICKS_PER_SEC: u64 = 20;
/// Building time, in minutes, unless `/start` says otherwise.
const BUILD_MINUTES: u64 = 5;
/// A round starts on its own this long after the first player joins.
const AUTO_START_SECS: u64 = 60;
/// How long everyone looks at each plot.
const SHOW_SECS: u64 = 20;
/// Once everyone has voted on a plot, move on after this long.
const VOTED_SECS: u64 = 3;
const RESULTS_SECS: u64 = 10;
/// The buildable inside of a plot (blocks across).
const INNER: i32 = 11;
/// How high you can build above the plot's floor.
const BUILD_HEIGHT: i32 = 20;
/// From one plot's corner to the next.
const STRIDE: i32 = INNER + 2 + 3;
const PLOTS_ACROSS: i32 = 4;
const PLOT_ROWS: i32 = 2;
/// Blocks placed per tick while making or clearing plots.
const BUILD_PER_TICK: usize = 300;
/// Blocks looked at per tick, at most (placed or not).
const LOOKS_PER_TICK: usize = 1500;
/// Flat ground round the plots, so they aren't in a pit on a hillside.
const FIELD_MARGIN: i32 = 4;

const THEMES: [&str; 20] = [
    "A rocket ship",
    "A house for a cat",
    "A castle",
    "Your dream treehouse",
    "A giant piece of food",
    "A pirate ship",
    "Your favourite animal",
    "A robot",
    "A rainbow bridge",
    "A snowman's house",
    "A dragon",
    "A lighthouse",
    "A spooky house",
    "An ice cream shop",
    "A car or a train",
    "A flower garden",
    "A tower as tall as you can",
    "A zoo",
    "An underwater base",
    "A house for a giant",
];

/// The vote badges: (tool name, shown as, points, colour).
const VOTES: [(&str, &str, u32, Rgba); 5] = [
    ("contest:vote_1", "Meh", 1, [150, 150, 156, 255]),
    ("contest:vote_2", "OK", 2, [170, 120, 70, 255]),
    ("contest:vote_3", "Good", 3, [110, 190, 90, 255]),
    ("contest:vote_4", "Great", 4, [90, 160, 240, 255]),
    ("contest:vote_5", "WOW!", 5, [250, 200, 50, 255]),
];

/// Border colours for the plots, so they're easy to tell apart.
const BORDERS: [&str; 8] = [
    "core:red_wool",
    "core:orange_wool",
    "core:yellow_wool",
    "core:lime_wool",
    "core:light_blue_wool",
    "core:blue_wool",
    "core:purple_wool",
    "core:pink_wool",
];

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    /// Waiting to start. `auto_at`: the tick a round starts by itself.
    Lobby {
        auto_at: Option<u64>,
    },
    /// Clearing the plots, then building starts for this many minutes.
    Preparing {
        minutes: u64,
    },
    Build {
        ends_at: u64,
    },
    /// Showing `order[current]` until `until`.
    Vote {
        order: Vec<usize>,
        current: usize,
        until: u64,
    },
    Results {
        until: u64,
    },
}

#[derive(Clone, Debug)]
struct Plot {
    /// The inside's corner with the smallest x and z.
    x: i32,
    z: i32,
    owner: Option<PlayerId>,
    owner_name: String,
    /// Votes on this plot: voter → points.
    votes: BTreeMap<PlayerId, u32>,
}

impl Plot {
    fn contains(&self, x: i32, z: i32) -> bool {
        x >= self.x && x < self.x + INNER && z >= self.z && z < self.z + INNER
    }

    fn centre(&self, floor: i32) -> Vec3 {
        [
            (self.x + INNER / 2) as f32 + 0.5,
            (floor + 1) as f32,
            (self.z + INNER / 2) as f32 + 0.5,
        ]
    }

    fn points(&self) -> u32 {
        self.votes.values().sum()
    }
}

#[derive(Default)]
struct Game {
    active: bool,
    tick: u64,
    seed: u64,
    phase: Option<Phase>,
    theme: &'static str,
    /// Players in the game, by id, with their names.
    players: BTreeMap<PlayerId, String>,
    plots: Vec<Plot>,
    /// Height of every plot's floor.
    floor: i32,
    to_build: Vec<([i32; 3], BlockId)>,
}

impl Mod for Game {
    fn on_event(&mut self, event: Event) -> EventReply {
        match event {
            Event::Init { game_mode, .. } => {
                // Always register the badges, so tool ids match in every
                // world; only run the game in build_contest worlds.
                register_badges();
                self.active = game_mode.as_deref() == Some(MODE);
                if self.active {
                    guest::log("Build Contest is on! Plots, themes and votes at the ready.");
                }
                EventReply::Ok
            }
            _ if !self.active => EventReply::Ok,
            Event::Start => {
                self.phase = Some(Phase::Lobby { auto_at: None });
                if let Some(s) = guest::world_spawn() {
                    self.lay_out_plots(s);
                }
                EventReply::Ok
            }
            Event::PlayerJoined { player, name } => {
                self.join(player, name);
                EventReply::Ok
            }
            Event::PlayerLeft { player } => {
                self.players.remove(&player);
                for plot in &mut self.plots {
                    plot.votes.remove(&player);
                }
                EventReply::Ok
            }
            Event::Tick { tick, .. } => {
                self.tick = tick;
                self.update();
                EventReply::Ok
            }
            Event::BlockChange { player, pos, .. } => {
                if self.may_build(player, pos) {
                    EventReply::Ok
                } else {
                    EventReply::Deny
                }
            }
            Event::UseTool { player, tool, .. } => {
                self.vote(player, &tool);
                EventReply::Ok
            }
            Event::Chat { player, text } => self.command(player, &text),
            _ => EventReply::Ok,
        }
    }
}

/// A star badge in a colour, for voting.
fn badge_icon(color: Rgba) -> Vec<u8> {
    let rows = [
        "................",
        ".......XX.......",
        ".......XX.......",
        "......XXXX......",
        "......XXXX......",
        "XXXXXXXXXXXXXXXX",
        ".XXXXXXXXXXXXXX.",
        "..XXXXXXXXXXXX..",
        "...XXXXXXXXXX...",
        "....XXXXXXXX....",
        "....XXXXXXXX....",
        "...XXXXXXXXXX...",
        "...XXXX..XXXX...",
        "..XXX......XXX..",
        "..XX........XX..",
        "................",
    ];
    pixel_art(&rows, &[('X', color)])
}

fn register_badges() {
    for (name, label, _, color) in VOTES {
        guest::register_tool(ToolSpec {
            name: name.into(),
            display_name: format!("Vote: {label}"),
            icon: badge_icon(color),
            cooldown: 0.3,
        });
    }
}

impl Game {
    fn random(&mut self, n: usize) -> usize {
        self.seed = self
            .seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407 ^ self.tick);
        ((self.seed >> 33) % n as u64) as usize
    }

    fn secs(&self, s: u64) -> u64 {
        self.tick + s * TICKS_PER_SEC
    }

    fn secs_left(&self, until: u64) -> u64 {
        until.saturating_sub(self.tick).div_ceil(TICKS_PER_SEC)
    }

    // ---- Plots ----

    /// Plots in two rows just south of the spawn point, at the spawn's
    /// height, made the first time the server starts (and kept after).
    fn lay_out_plots(&mut self, spawn: BlockPos) {
        self.floor = spawn[1];
        let left = spawn[0] - (PLOTS_ACROSS * STRIDE) / 2;
        let top = spawn[2] + 8;
        self.plots.clear();
        for row in 0..PLOT_ROWS {
            for col in 0..PLOTS_ACROSS {
                self.plots.push(Plot {
                    x: left + col * STRIDE + 1,
                    z: top + row * STRIDE + 1,
                    owner: None,
                    owner_name: String::new(),
                    votes: BTreeMap::new(),
                });
            }
        }
        // Flatten the whole field first (plots last, so they're made
        // first: the queue is worked from the end).
        for i in 0..self.plots.len() {
            self.plan_plot(i);
        }
        let field = (
            left - FIELD_MARGIN,
            top - FIELD_MARGIN,
            left + PLOTS_ACROSS * STRIDE + FIELD_MARGIN,
            top + PLOT_ROWS * STRIDE + FIELD_MARGIN,
        );
        self.plan_ground(field, None);
    }

    /// Queue flat ground over a rectangle (x0, z0, x1, z1, all inclusive):
    /// a little dirt, a floor, and nothing above. `border` paints the
    /// rectangle's edge in that block instead of grass.
    fn plan_ground(&mut self, (x0, z0, x1, z1): (i32, i32, i32, i32), border: Option<BlockId>) {
        let pick = |n: &str| guest::block_id(n).unwrap_or(1);
        let grass = pick("core:grass");
        let dirt = pick("core:dirt");
        let f = self.floor;
        let mut blocks = Vec::new();
        for x in x0..=x1 {
            for z in z0..=z1 {
                let edge = x == x0 || x == x1 || z == z0 || z == z1;
                for y in f - 3..f {
                    blocks.push(([x, y, z], dirt));
                }
                let top = match border {
                    Some(b) if edge => b,
                    _ => grass,
                };
                blocks.push(([x, f, z], top));
                for y in f + 1..=f + BUILD_HEIGHT {
                    blocks.push(([x, y, z], 0));
                }
            }
        }
        blocks.reverse();
        self.to_build.extend(blocks);
    }

    /// Queue a fresh plot: grass inside, a coloured border, and nothing
    /// above it.
    fn plan_plot(&mut self, i: usize) {
        let border = guest::block_id(BORDERS[i % BORDERS.len()]).unwrap_or(1);
        let (px, pz) = (self.plots[i].x, self.plots[i].z);
        self.plan_ground((px - 1, pz - 1, px + INNER, pz + INNER), Some(border));
    }

    /// Place some of the queued blocks. Each tick looks at a limited
    /// number (every look is a call to the game), and blocks that can't be
    /// placed yet (not loaded) go to the back of the queue.
    fn build_some(&mut self) {
        let mut placed = 0;
        let mut looked = 0;
        let mut later = Vec::new();
        while placed < BUILD_PER_TICK && looked < LOOKS_PER_TICK {
            let Some((pos, block)) = self.to_build.pop() else {
                break;
            };
            looked += 1;
            if guest::get_block(pos) == block {
                continue;
            }
            if guest::try_set_block(pos, block) {
                placed += 1;
            } else {
                later.push((pos, block));
            }
        }
        // The queue is worked from the end, so the back is the front.
        later.reverse();
        self.to_build.splice(0..0, later);
    }

    fn plot_of(&self, player: PlayerId) -> Option<usize> {
        self.plots.iter().position(|p| p.owner == Some(player))
    }

    fn may_build(&self, player: PlayerId, pos: BlockPos) -> bool {
        let in_plots = self.plots.iter().any(|p| {
            pos[0] >= p.x - 1 && pos[0] <= p.x + INNER && pos[2] >= p.z - 1 && pos[2] <= p.z + INNER
        });
        match self.phase {
            // Between rounds: build anywhere except the plots.
            Some(Phase::Lobby { .. }) | None => !in_plots,
            Some(Phase::Build { .. }) => self.plot_of(player).is_some_and(|i| {
                self.plots[i].contains(pos[0], pos[2])
                    && pos[1] > self.floor
                    && pos[1] <= self.floor + BUILD_HEIGHT
            }),
            _ => false,
        }
    }

    // ---- Players ----

    fn join(&mut self, player: PlayerId, name: String) {
        self.players.insert(player, name.clone());
        guest::set_rules(player, PlayerRules::default());
        guest::set_tools(player, Vec::new());
        match self.phase.clone() {
            Some(Phase::Lobby { auto_at: None }) => {
                let at = self.secs(AUTO_START_SECS);
                self.phase = Some(Phase::Lobby { auto_at: Some(at) });
            }
            // Late, but there may be a free plot.
            Some(Phase::Build { .. }) if self.give_plot(player) => {
                guest::chat(
                    Some(player),
                    &format!(
                        "You made it! The theme is: {}. Build on your plot!",
                        self.theme
                    ),
                );
            }
            _ => {}
        }
        guest::show_title(Some(player), "Build Contest!", 4.0);
        guest::chat(
            Some(player),
            &format!(
                "Welcome, {name}! Everyone builds the same secret theme on their own plot, then you all vote. Type /start when everyone's here."
            ),
        );
    }

    /// Give a player the first free plot and send them there.
    fn give_plot(&mut self, player: PlayerId) -> bool {
        let Some(i) = self.plots.iter().position(|p| p.owner.is_none()) else {
            guest::chat(
                Some(player),
                "All the plots are taken: you can watch and vote!",
            );
            return false;
        };
        let name = self.players.get(&player).cloned().unwrap_or_default();
        self.plots[i].owner = Some(player);
        self.plots[i].owner_name = name;
        guest::teleport(player, self.plots[i].centre(self.floor));
        true
    }

    // ---- Commands ----

    fn command(&mut self, player: PlayerId, text: &str) -> EventReply {
        let mut words = text.split_whitespace();
        match words.next() {
            Some("/start") => {
                if matches!(self.phase, Some(Phase::Lobby { .. }) | None) {
                    let minutes = words
                        .next()
                        .and_then(|w| w.parse::<u64>().ok())
                        .unwrap_or(BUILD_MINUTES)
                        .clamp(1, 15);
                    self.prepare(minutes);
                } else {
                    guest::chat(Some(player), "A round is already going!");
                }
                EventReply::Handled
            }
            Some("/theme") => {
                let msg = if self.theme.is_empty() {
                    "No theme yet: type /start to begin a round.".to_owned()
                } else {
                    format!("The theme is: {}", self.theme)
                };
                guest::chat(Some(player), &msg);
                EventReply::Handled
            }
            Some("/help") => {
                guest::chat(
                    Some(player),
                    "Build Contest: /start (or /start 3 for three minutes) begins a round, /theme says the theme. Vote with the badges in your hotbar.",
                );
                EventReply::Ok
            }
            _ => EventReply::Ok,
        }
    }

    // ---- The round ----

    /// Clear the plots, then start building.
    fn prepare(&mut self, minutes: u64) {
        for i in 0..self.plots.len() {
            self.plots[i].owner = None;
            self.plots[i].owner_name.clear();
            self.plots[i].votes.clear();
            self.plan_plot(i);
        }
        self.phase = Some(Phase::Preparing { minutes });
        guest::show_title(None, "Getting the plots ready...", 3.0);
    }

    fn start_building(&mut self, minutes: u64) {
        self.theme = THEMES[self.random(THEMES.len())];
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            self.give_plot(id);
        }
        self.phase = Some(Phase::Build {
            ends_at: self.secs(minutes * 60),
        });
        guest::show_title(None, &format!("Build: {}!", self.theme), 5.0);
        guest::sound("cheer", None);
        guest::chat(
            None,
            &format!(
                "The theme is: {}! You have {minutes} minute{} to build it on your own plot.",
                self.theme,
                if minutes == 1 { "" } else { "s" }
            ),
        );
    }

    fn start_voting(&mut self) {
        let order: Vec<usize> = (0..self.plots.len())
            .filter(|&i| self.plots[i].owner.is_some())
            .collect();
        if order.is_empty() {
            self.phase = Some(Phase::Lobby { auto_at: None });
            return;
        }
        if self.players.len() < 2 {
            guest::show_title(None, "Time's up! Lovely building!", 5.0);
            guest::chat(None, "Nobody else to vote this time, so everyone wins!");
            let i = order[0];
            self.celebrate(i);
            self.phase = Some(Phase::Results {
                until: self.secs(RESULTS_SECS),
            });
            return;
        }
        let tools: Vec<String> = VOTES.iter().map(|v| v.0.to_owned()).collect();
        for id in self.players.keys() {
            guest::set_tools(*id, tools.clone());
        }
        guest::show_title(None, "Time's up! Let's vote!", 3.0);
        guest::chat(
            None,
            "Visit each build and vote with a badge from your hotbar: Meh, OK, Good, Great or WOW!",
        );
        self.show_plot(order, 0);
    }

    fn show_plot(&mut self, order: Vec<usize>, current: usize) {
        let i = order[current];
        let plot = &self.plots[i];
        // Stand just outside the plot's front edge, a little up.
        let spot = [
            (plot.x + INNER / 2) as f32 + 0.5,
            (self.floor + 3) as f32,
            (plot.z + INNER + 2) as f32 + 0.5,
        ];
        for id in self.players.keys() {
            guest::teleport(*id, spot);
        }
        guest::show_title(None, &format!("{}'s build", plot.owner_name), 3.0);
        self.phase = Some(Phase::Vote {
            order,
            current,
            until: self.secs(SHOW_SECS),
        });
    }

    fn vote(&mut self, player: PlayerId, tool: &str) {
        let Some(Phase::Vote {
            order,
            current,
            until,
        }) = &self.phase
        else {
            return;
        };
        let Some(&(_, label, points, _)) = VOTES.iter().find(|v| v.0 == tool) else {
            return;
        };
        let i = order[*current];
        if self.plots[i].owner == Some(player) {
            guest::chat(
                Some(player),
                "That's your own build! Wait for the others to vote.",
            );
            return;
        }
        self.plots[i].votes.insert(player, points);
        guest::chat(Some(player), &format!("You voted: {label}"));
        guest::sound("click", None);
        // Everyone (but the builder) has voted: move on soon.
        let voters = self
            .players
            .keys()
            .filter(|p| self.plots[i].owner != Some(**p))
            .count();
        let soon = self.secs(VOTED_SECS);
        if self.plots[i].votes.len() >= voters && *until > soon {
            let (order, current) = (order.clone(), *current);
            self.phase = Some(Phase::Vote {
                order,
                current,
                until: soon,
            });
        }
    }

    fn finish(&mut self) {
        for id in self.players.keys() {
            guest::set_tools(*id, Vec::new());
        }
        let mut ranked: Vec<usize> = (0..self.plots.len())
            .filter(|&i| self.plots[i].owner.is_some())
            .collect();
        ranked.sort_by_key(|&i| std::cmp::Reverse(self.plots[i].points()));
        let Some(&best) = ranked.first() else { return };
        let top = self.plots[best].points();
        let winners: Vec<String> = ranked
            .iter()
            .filter(|&&i| self.plots[i].points() == top)
            .map(|&i| self.plots[i].owner_name.clone())
            .collect();
        let title = if winners.len() == 1 {
            format!("{} wins!", winners[0])
        } else {
            format!("A tie: {}!", winners.join(" and "))
        };
        guest::show_title(None, &title, 6.0);
        guest::sound("cheer", None);
        let table: Vec<String> = ranked
            .iter()
            .map(|&i| format!("{} {}", self.plots[i].owner_name, self.plots[i].points()))
            .collect();
        guest::chat(
            None,
            &format!("Points: {}. Type /start to play again!", table.join(", ")),
        );
        for &i in ranked.iter().filter(|&&i| self.plots[i].points() == top) {
            self.celebrate(i);
        }
        self.phase = Some(Phase::Results {
            until: self.secs(RESULTS_SECS),
        });
    }

    fn celebrate(&self, i: usize) {
        let c = self.plots[i].centre(self.floor);
        for (dx, dz) in [
            (0.0, 0.0),
            (-3.0, -3.0),
            (3.0, 3.0),
            (-3.0, 3.0),
            (3.0, -3.0),
        ] {
            guest::effect(
                EffectKind::Confetti,
                [c[0] + dx, c[1] + 2.0, c[2] + dz],
                [255, 255, 255, 255],
            );
        }
        // Everyone gathers round the winner's build.
        let spot = [c[0], c[1] + 2.0, (self.plots[i].z + INNER + 2) as f32 + 0.5];
        for id in self.players.keys() {
            guest::teleport(*id, spot);
        }
    }

    fn update(&mut self) {
        if !self.to_build.is_empty() {
            self.build_some();
        }
        match self.phase.clone() {
            Some(Phase::Lobby { auto_at: Some(at) }) if self.tick >= at => {
                if !self.players.is_empty() {
                    self.prepare(BUILD_MINUTES);
                }
            }
            Some(Phase::Preparing { minutes }) if self.to_build.is_empty() => {
                self.start_building(minutes);
            }
            Some(Phase::Build { ends_at }) if self.tick >= ends_at => self.start_voting(),
            Some(Phase::Vote {
                order,
                current,
                until,
            }) if self.tick >= until => {
                if current + 1 < order.len() {
                    self.show_plot(order, current + 1);
                } else {
                    self.finish();
                }
            }
            Some(Phase::Results { until }) if self.tick >= until => {
                self.phase = Some(Phase::Lobby { auto_at: None });
            }
            _ => {}
        }
        if self.tick.is_multiple_of(TICKS_PER_SEC) {
            self.update_hud();
        }
    }

    fn update_hud(&self) {
        let gold = [250, 210, 80, 255];
        let mut lines = vec![HudLine {
            text: "Build Contest".into(),
            color: gold,
        }];
        match &self.phase {
            Some(Phase::Lobby { auto_at }) => {
                lines.push(hud_line("Type /start to begin"));
                if let Some(at) = auto_at {
                    lines.push(hud_line(format!("Starting in {}s", self.secs_left(*at))));
                }
            }
            Some(Phase::Preparing { .. }) => lines.push(hud_line("Getting plots ready...")),
            Some(Phase::Build { ends_at }) => {
                let left = self.secs_left(*ends_at);
                lines.push(hud_line(format!("Theme: {}", self.theme)));
                lines.push(hud_line(format!(
                    "Time left: {}:{:02}",
                    left / 60,
                    left % 60
                )));
            }
            Some(Phase::Vote {
                order,
                current,
                until,
            }) => {
                let plot = &self.plots[order[*current]];
                lines.push(hud_line(format!("Theme: {}", self.theme)));
                lines.push(hud_line(format!(
                    "Build {} of {}: {}",
                    current + 1,
                    order.len(),
                    plot.owner_name
                )));
                lines.push(hud_line(format!("Vote! {}s", self.secs_left(*until))));
            }
            Some(Phase::Results { .. }) => lines.push(hud_line("Well done, everyone!")),
            None => {}
        }
        guest::set_hud(None, lines);
    }
}

ourcraft_mod_api::export_mod!(Game);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plots_do_not_overlap_and_fit_their_borders() {
        let mut g = Game::default();
        g.lay_out_plots([0, 64, 0]);
        assert_eq!(g.plots.len(), (PLOTS_ACROSS * PLOT_ROWS) as usize);
        for (i, a) in g.plots.iter().enumerate() {
            for b in &g.plots[i + 1..] {
                // Borders included, with a gap.
                assert!((a.x - b.x).abs() >= INNER + 2 || (a.z - b.z).abs() >= INNER + 2);
            }
            // Spawn is clear of the plots.
            assert!(!a.contains(0, 0));
        }
        // Plots, borders, a little ground under and the space above, and
        // the field round them.
        let column = (3 + 1 + BUILD_HEIGHT) as usize;
        let plots = ((INNER + 2) * (INNER + 2)) as usize * g.plots.len();
        let field = ((PLOTS_ACROSS * STRIDE + 2 * FIELD_MARGIN + 1)
            * (PLOT_ROWS * STRIDE + 2 * FIELD_MARGIN + 1)) as usize;
        assert_eq!(g.to_build.len(), (plots + field) * column);
    }

    #[test]
    fn building_only_on_your_own_plot_during_a_round() {
        let mut g = Game::default();
        g.lay_out_plots([0, 64, 0]);
        g.plots[0].owner = Some(7);
        let (x, z) = (g.plots[0].x + 2, g.plots[0].z + 2);
        g.phase = Some(Phase::Build { ends_at: 100 });
        assert!(g.may_build(7, [x, 66, z]));
        assert!(!g.may_build(7, [x, 64, z]), "not the floor");
        assert!(!g.may_build(8, [x, 66, z]), "not someone else's");
        assert!(!g.may_build(7, [g.plots[1].x + 2, 66, g.plots[1].z]));
        g.phase = Some(Phase::Lobby { auto_at: None });
        assert!(!g.may_build(7, [x, 66, z]), "plots rest between rounds");
        assert!(
            g.may_build(7, [0, 66, 0]),
            "but the rest of the world is fine"
        );
    }

    #[test]
    fn badges_are_all_different() {
        for (i, a) in VOTES.iter().enumerate() {
            assert_eq!(a.2 as usize, i + 1);
            assert_eq!(badge_icon(a.3).len(), 16 * 16 * 4);
            for b in &VOTES[i + 1..] {
                assert_ne!(a.0, b.0);
            }
        }
    }
}

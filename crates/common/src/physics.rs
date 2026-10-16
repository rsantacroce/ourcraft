//! Player movement physics, shared by client and server.
//!
//! Lives in `common` (not the client) on purpose: in M7 the client predicts
//! its own movement by running *exactly* this code, and the server checks it
//! by running it too. Same input + same world = same result.
//!
//! **Fixed timestep.** Physics advances in fixed steps of 1/60 s, no matter
//! the frame rate. That keeps jumps the same height at 30 or 144 FPS and is
//! essential for networking (client and server must step identically). The
//! camera is smoothed between steps so it still looks fluid at high FPS.
//!
//! **Collision.** The player is an axis-aligned box (AABB). Each step we move
//! along Y, then X, then Z separately; if the box would overlap a solid
//! block, we stop it flush against that block and zero that axis' velocity.
//! Moving one axis at a time is what lets you slide along walls.

use glam::{IVec3, Vec2, Vec3};

use crate::block::{BlockId, BlockRegistry};
use crate::world::BlockSource;

/// Physics steps per second.
pub const TICK_RATE: f64 = 60.0;
/// Seconds per physics step.
pub const TICK_DT: f32 = 1.0 / TICK_RATE as f32;

pub const PLAYER_WIDTH: f32 = 0.6;
pub const PLAYER_HEIGHT: f32 = 1.8;
/// Eye height above the feet.
pub const EYE_HEIGHT: f32 = 1.62;

const GRAVITY: f32 = 28.0;
const JUMP_SPEED: f32 = 8.6;
const TERMINAL_SPEED: f32 = 60.0;
const WALK_SPEED: f32 = 4.3;
const SPRINT_SPEED: f32 = 6.2;
const FLY_SPEED: f32 = 11.0;
const SWIM_SPEED: f32 = 3.0;
/// Rowing a boat on water (on land a boat only shuffles along).
const BOAT_SPEED: f32 = 6.0;
const BOAT_LAND_SPEED: f32 = 0.8;
const GROUND_ACCEL: f32 = 60.0;
const AIR_ACCEL: f32 = 12.0;
const FLY_ACCEL: f32 = 40.0;
/// Tiny gap kept between the player and walls so floating-point rounding
/// never leaves the box overlapping a block.
const SKIN: f32 = 1e-3;

/// A hop potion makes jumps higher (about two blocks instead of one).
fn jump_boost(input: &PlayerInput) -> f32 {
    if input.effects & crate::magic::Effect::Hop.bit() != 0 {
        1.4
    } else {
        1.0
    }
}

/// A swim potion makes swimming much quicker.
fn swim_boost(input: &PlayerInput) -> f32 {
    if input.effects & crate::magic::Effect::Swim.bit() != 0 {
        2.2
    } else {
        1.0
    }
}

/// What the player is asking to do this step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerInput {
    /// `x` = strafe right, `y` = forward, each `-1..=1`.
    pub movement: Vec2,
    pub jump: bool,
    /// Fly/swim down.
    pub descend: bool,
    pub sprint: bool,
    /// Facing direction around the vertical axis, radians.
    pub yaw: f32,
    /// Hop up one-block steps automatically when walking into them. Used
    /// with touch controls, where pressing jump while walking is fiddly.
    pub auto_jump: bool,
    /// Speed multiplier set by the game mode (1 = normal, 0 = can't move).
    pub speed: f32,
    /// Sitting in a boat: float on water and row quickly.
    pub boat: bool,
    /// Potion effects working right now (bits of [`crate::magic::Effect`]).
    pub effects: u8,
}

impl Default for PlayerInput {
    fn default() -> Self {
        Self {
            movement: Vec2::ZERO,
            jump: false,
            descend: false,
            sprint: false,
            yaw: 0.0,
            auto_jump: false,
            speed: 1.0,
            boat: false,
            effects: 0,
        }
    }
}

/// Everything about a player's movement that carries over between steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerState {
    /// Centre of the bottom of the player's box (their feet).
    pub position: Vec3,
    pub velocity: Vec3,
    pub on_ground: bool,
    pub flying: bool,
    pub in_water: bool,
}

impl PlayerState {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            on_ground: false,
            flying: false,
            in_water: false,
        }
    }

    pub fn eye(&self) -> Vec3 {
        self.position + Vec3::Y * EYE_HEIGHT
    }

    pub fn aabb(&self) -> Aabb {
        Aabb::player(self.position)
    }
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn player(feet: Vec3) -> Self {
        let half = PLAYER_WIDTH / 2.0;
        Self {
            min: feet - Vec3::new(half, 0.0, half),
            max: feet + Vec3::new(half, PLAYER_HEIGHT, half),
        }
    }

    /// The box of the block at integer position `b`.
    pub fn block(b: IVec3) -> Self {
        Self {
            min: b.as_vec3(),
            max: b.as_vec3() + Vec3::ONE,
        }
    }

    /// Distance along a ray (`dir` normalized) to where it enters the box,
    /// if it does within `max`. Uses the "slab" method: intersect the ray
    /// with each pair of parallel faces and keep the overlap.
    pub fn ray_hit(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let mut t_min: f32 = 0.0;
        let mut t_max = max;
        for axis in 0..3 {
            let (o, d) = (origin[axis], dir[axis]);
            if d.abs() < 1e-8 {
                if o < self.min[axis] || o > self.max[axis] {
                    return None;
                }
                continue;
            }
            let (mut t1, mut t2) = ((self.min[axis] - o) / d, (self.max[axis] - o) / d);
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_min = t_min.max(t1);
            t_max = t_max.min(t2);
            if t_min > t_max {
                return None;
            }
        }
        Some(t_min)
    }

    /// The box grown by `r` on every side.
    pub fn inflate(&self, r: f32) -> Aabb {
        Aabb {
            min: self.min - Vec3::splat(r),
            max: self.max + Vec3::splat(r),
        }
    }

    pub fn intersects(&self, other: &Aabb) -> bool {
        self.min.cmplt(other.max).all() && self.max.cmpgt(other.min).all()
    }

    /// Integer block positions this box overlaps.
    pub fn blocks(&self) -> impl Iterator<Item = IVec3> {
        let lo = self.min.floor().as_ivec3();
        let hi = (self.max - Vec3::splat(1e-4)).floor().as_ivec3();
        (lo.y..=hi.y).flat_map(move |y| {
            (lo.z..=hi.z).flat_map(move |z| (lo.x..=hi.x).map(move |x| IVec3::new(x, y, z)))
        })
    }
}

/// The solid boxes of the block at `b`: a full cube for most solid blocks,
/// smaller boxes for slabs, stairs, fences and doors, nothing for air.
/// Unloaded chunks count as solid, so players can't fall out of the world
/// while terrain is still streaming in.
fn solid_boxes(world: &impl BlockSource, reg: &BlockRegistry, b: IVec3) -> Vec<Aabb> {
    let Some(id) = world.get_block(b.x, b.y, b.z) else {
        return vec![Aabb::block(b)];
    };
    if !reg.is_solid(id) {
        return Vec::new();
    }
    if reg.shape(id).is_cube() {
        return vec![Aabb::block(b)];
    }
    let near = |o: [i32; 3]| {
        world
            .get_block(b.x + o[0], b.y + o[1], b.z + o[2])
            .unwrap_or(BlockId::AIR)
    };
    reg.boxes_at(id, near)
        .into_iter()
        .map(|(min, max)| Aabb {
            min: b.as_vec3() + Vec3::from(min),
            max: b.as_vec3() + Vec3::from(max),
        })
        .collect()
}

/// Is any part of the player box touching a ladder?
fn on_ladder(world: &impl BlockSource, reg: &BlockRegistry, feet: Vec3) -> bool {
    Aabb::player(feet).inflate(0.05).blocks().any(|b| {
        world
            .get_block(b.x, b.y, b.z)
            .is_some_and(|id| reg.is_climbable(id))
    })
}

fn is_liquid(world: &impl BlockSource, reg: &BlockRegistry, p: Vec3) -> bool {
    let b = p.floor().as_ivec3();
    world
        .get_block(b.x, b.y, b.z)
        .is_some_and(|id| reg.is_liquid(id))
}

/// Does a player box at `feet` overlap any solid block?
pub fn collides(world: &impl BlockSource, reg: &BlockRegistry, feet: Vec3) -> bool {
    let me = Aabb::player(feet);
    me.blocks()
        .any(|b| solid_boxes(world, reg, b).iter().any(|s| s.intersects(&me)))
}

/// How high players walk up without jumping (slabs and stair steps).
const STEP_HEIGHT: f32 = 0.55;
/// Climbing speed on ladders.
const CLIMB_SPEED: f32 = 2.8;

/// Move `value` towards `target` by at most `max_delta`.
fn approach(value: f32, target: f32, max_delta: f32) -> f32 {
    if value < target {
        (value + max_delta).min(target)
    } else {
        (value - max_delta).max(target)
    }
}

/// Advance one physics step of `TICK_DT` seconds.
pub fn step(
    state: &mut PlayerState,
    input: &PlayerInput,
    world: &impl BlockSource,
    reg: &BlockRegistry,
) {
    let dt = TICK_DT;
    state.in_water = is_liquid(world, reg, state.position + Vec3::Y * 0.4);

    // Desired horizontal velocity from the input, rotated by where we face.
    let (sin, cos) = input.yaw.sin_cos();
    let forward = Vec3::new(-sin, 0.0, -cos);
    let right = Vec3::new(cos, 0.0, -sin);
    let mult = input.speed.clamp(0.0, 4.0);
    let wish = (right * input.movement.x + forward * input.movement.y).clamp_length_max(1.0) * mult;
    let vertical_input = (input.jump as i32 as f32 - input.descend as i32 as f32) * mult.min(1.0);
    // Frozen players (speed 0) can't jump either.
    let can_jump = mult > 0.0;

    if state.flying {
        let speed = if input.sprint {
            FLY_SPEED * 2.0
        } else {
            FLY_SPEED
        };
        let target = wish * speed + Vec3::Y * vertical_input * speed;
        let a = FLY_ACCEL * dt;
        state.velocity = Vec3::new(
            approach(state.velocity.x, target.x, a),
            approach(state.velocity.y, target.y, a),
            approach(state.velocity.z, target.z, a),
        );
    } else if input.boat {
        // Afloat: the boat sits on the water, wherever its surface is.
        let under = is_liquid(world, reg, state.position + Vec3::Y * 0.1);
        let afloat = under || is_liquid(world, reg, state.position - Vec3::Y * 0.2);
        let speed = if !afloat {
            BOAT_LAND_SPEED
        } else if input.sprint {
            BOAT_SPEED * 1.3
        } else {
            BOAT_SPEED
        };
        let target = wish * speed;
        let a = GROUND_ACCEL * 0.4 * dt;
        state.velocity.x = approach(state.velocity.x, target.x, a);
        state.velocity.z = approach(state.velocity.z, target.z, a);
        state.velocity.y = if under {
            approach(state.velocity.y, 2.5, 30.0 * dt)
        } else if afloat {
            0.0
        } else {
            (state.velocity.y - GRAVITY * dt).max(-TERMINAL_SPEED)
        };
    } else if state.in_water {
        let target = wish * SWIM_SPEED * swim_boost(input);
        let a = GROUND_ACCEL * 0.5 * dt;
        state.velocity.x = approach(state.velocity.x, target.x, a);
        state.velocity.z = approach(state.velocity.z, target.z, a);
        // Sink slowly; hold jump to swim up.
        let target_y = if input.jump {
            3.5
        } else if input.descend {
            -4.0
        } else {
            -1.5
        };
        state.velocity.y = approach(state.velocity.y, target_y, 20.0 * dt);
    } else {
        let speed = if input.sprint {
            SPRINT_SPEED
        } else {
            WALK_SPEED
        };
        let target = wish * speed;
        let a = if state.on_ground {
            GROUND_ACCEL
        } else {
            AIR_ACCEL
        } * dt;
        state.velocity.x = approach(state.velocity.x, target.x, a);
        state.velocity.z = approach(state.velocity.z, target.z, a);
        if input.jump && state.on_ground && can_jump {
            state.velocity.y = JUMP_SPEED * jump_boost(input);
        }
        state.velocity.y = (state.velocity.y - GRAVITY * dt).max(-TERMINAL_SPEED);
        // Ladders: hold jump to climb, "down" to go down, otherwise slide
        // down slowly.
        if on_ladder(world, reg, state.position) {
            state.velocity.y = if input.jump && can_jump {
                CLIMB_SPEED
            } else if input.descend {
                -CLIMB_SPEED
            } else {
                state.velocity.y.max(-CLIMB_SPEED * 0.5)
            };
        }
    }

    // Move one axis at a time, Y first so landing is resolved before sliding.
    let was_on_ground = state.on_ground;
    let mut hit_wall = false;
    state.on_ground = false;
    for axis in [1, 0, 2] {
        let delta = state.velocity[axis] * dt;
        if delta == 0.0 {
            continue;
        }
        let mut next = state.position;
        next[axis] += delta;
        let before = Aabb::player(state.position);
        let aabb = Aabb::player(next);
        let mut hit = false;
        for b in aabb.blocks() {
            for solid in solid_boxes(world, reg, b) {
                // Boxes we already overlap (say, a block placed inside us, or
                // an unloaded chunk) don't block movement, so we can't get
                // stuck.
                if !solid.intersects(&aabb) || before.intersects(&solid) {
                    continue;
                }
                hit = true;
                // Push back to just touching the box's face.
                let half = if axis == 1 { 0.0 } else { PLAYER_WIDTH / 2.0 };
                if delta > 0.0 {
                    let limit =
                        solid.min[axis] - SKIN - if axis == 1 { PLAYER_HEIGHT } else { half };
                    next[axis] = next[axis].min(limit);
                } else {
                    let limit = solid.max[axis] + SKIN + half;
                    next[axis] = next[axis].max(limit);
                }
            }
        }
        // Walking into something low (a slab, a stair step): step up onto
        // it instead of stopping.
        if hit && axis != 1 && was_on_ground && !state.flying {
            let mut up = state.position;
            up[axis] += delta;
            up.y += STEP_HEIGHT;
            if !collides(world, reg, up) {
                // Settle down onto the step.
                let mut settled = up;
                for _ in 0..12 {
                    let lower = settled - Vec3::Y * (STEP_HEIGHT / 12.0);
                    if collides(world, reg, lower) {
                        break;
                    }
                    settled = lower;
                }
                state.position = settled;
                state.on_ground = true;
                continue;
            }
        }
        if hit {
            if axis == 1 && delta < 0.0 {
                state.on_ground = true;
            }
            if axis != 1 {
                hit_wall = true;
            }
            state.velocity[axis] = 0.0;
        }
        state.position = next;
    }

    // Auto-jump: walked into a wall that's only one block high.
    if input.auto_jump && hit_wall && was_on_ground && !state.flying && wish != Vec3::ZERO {
        let ahead = state.position + wish.normalize() * 0.4;
        let step_up = ahead + Vec3::Y * 1.05;
        if collides(world, reg, ahead) && !collides(world, reg, step_up) {
            state.velocity.y = JUMP_SPEED;
        }
    }

    // Landing while flying ends flight, like in creative mode.
    if state.flying && state.on_ground && input.descend {
        state.flying = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks;
    use crate::chunk::Chunk;
    use crate::world::{ColumnPos, VoxelWorld, WORLD_HEIGHT_CHUNKS};

    /// A world with a stone floor whose top is at y = 10, around the origin.
    fn flat_world() -> VoxelWorld {
        let mut w = VoxelWorld::new();
        for cx in -2..2 {
            for cz in -2..2 {
                let col = (0..WORLD_HEIGHT_CHUNKS).map(|_| Chunk::empty()).collect();
                w.insert_column(ColumnPos::new(cx, cz), col);
            }
        }
        for x in -32..32 {
            for z in -32..32 {
                for y in 0..10 {
                    w.set_block(x, y, z, blocks::STONE);
                }
            }
        }
        w
    }

    fn run(state: &mut PlayerState, input: PlayerInput, world: &VoxelWorld, ticks: usize) {
        let reg = BlockRegistry::with_builtin_blocks();
        for _ in 0..ticks {
            step(state, &input, world, &reg);
        }
    }

    #[test]
    fn falls_and_lands_on_the_floor() {
        let w = flat_world();
        let mut s = PlayerState::new(Vec3::new(0.5, 20.0, 0.5));
        run(&mut s, PlayerInput::default(), &w, 120);
        assert!(s.on_ground);
        assert!((s.position.y - 10.0).abs() < 0.01, "y = {}", s.position.y);
        assert_eq!(s.velocity.y, 0.0);
    }

    #[test]
    fn jumps_about_one_block() {
        let w = flat_world();
        let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        run(&mut s, PlayerInput::default(), &w, 2);
        let mut peak: f32 = 0.0;
        let reg = BlockRegistry::with_builtin_blocks();
        let jump = PlayerInput {
            jump: true,
            ..default_input()
        };
        step(&mut s, &jump, &w, &reg);
        for _ in 0..60 {
            step(&mut s, &PlayerInput::default(), &w, &reg);
            peak = peak.max(s.position.y - 10.0);
        }
        assert!(peak > 1.1 && peak < 1.6, "jump height {peak}");
        assert!(s.on_ground);
    }

    #[test]
    fn hop_potion_jumps_higher() {
        let w = flat_world();
        let reg = BlockRegistry::with_builtin_blocks();
        let peak = |effects: u8| {
            let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
            run(&mut s, PlayerInput::default(), &w, 2);
            let jump = PlayerInput {
                jump: true,
                effects,
                ..default_input()
            };
            step(&mut s, &jump, &w, &reg);
            let mut top: f32 = 0.0;
            for _ in 0..90 {
                step(&mut s, &PlayerInput::default(), &w, &reg);
                top = top.max(s.position.y - 10.0);
            }
            top
        };
        let (plain, hop) = (peak(0), peak(crate::magic::Effect::Hop.bit()));
        assert!(plain < 1.6 && hop > 2.2, "plain {plain}, hop {hop}");
    }

    fn default_input() -> PlayerInput {
        PlayerInput::default()
    }

    #[test]
    fn walls_stop_movement() {
        let mut w = flat_world();
        // A wall at x = 3, two blocks high.
        for z in -5..5 {
            w.set_block(3, 10, z, blocks::STONE);
            w.set_block(3, 11, z, blocks::STONE);
        }
        let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        // Yaw of -90° faces +X.
        let input = PlayerInput {
            movement: Vec2::new(0.0, 1.0),
            yaw: -std::f32::consts::FRAC_PI_2,
            ..default_input()
        };
        run(&mut s, input, &w, 120);
        let max_x = 3.0 - PLAYER_WIDTH / 2.0;
        assert!(
            s.position.x <= max_x && s.position.x > max_x - 0.01,
            "x = {}",
            s.position.x
        );
    }

    fn east() -> PlayerInput {
        PlayerInput {
            movement: Vec2::new(0.0, 1.0),
            yaw: -std::f32::consts::FRAC_PI_2,
            ..default_input()
        }
    }

    #[test]
    fn walks_up_slabs_and_stairs() {
        let reg = BlockRegistry::with_builtin_blocks();
        let mut w = flat_world();
        let slab = reg.by_name("core:stone_slab").unwrap();
        for z in -5..5 {
            w.set_block(3, 10, z, slab);
            for x in 4..12 {
                w.set_block(x, 10, z, blocks::STONE);
            }
        }
        let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        run(&mut s, east(), &w, 150);
        // Up the slab (half a block), then up onto the full block.
        assert!(s.position.x > 5.0, "x = {}", s.position.x);
        assert!((s.position.y - 11.0).abs() < 0.05, "y = {}", s.position.y);
    }

    #[test]
    fn full_blocks_still_need_a_jump() {
        let mut w = flat_world();
        for z in -5..5 {
            w.set_block(3, 10, z, blocks::STONE);
        }
        let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        run(&mut s, east(), &w, 120);
        assert!(s.position.x < 3.0);
    }

    #[test]
    fn closed_doors_stop_you_and_open_ones_dont() {
        let reg = BlockRegistry::with_builtin_blocks();
        let door = reg.by_name("core:door").unwrap();
        // A closed door facing east has its panel on the east side.
        let closed = reg
            .placement(door, [0, 1, 0], [1.0, 0.0, 0.0])
            .unwrap()
            .block;
        let open = reg.toggled(closed).unwrap()[0].1;
        for (block, passes) in [(closed, false), (open, true)] {
            let mut w = flat_world();
            for z in -5..5 {
                w.set_block(3, 10, z, block);
                w.set_block(3, 11, z, block);
            }
            let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
            run(&mut s, east(), &w, 150);
            assert_eq!(s.position.x > 4.5, passes, "x = {}", s.position.x);
        }
    }

    #[test]
    fn ladders_are_climbed_by_jumping() {
        let reg = BlockRegistry::with_builtin_blocks();
        let mut w = flat_world();
        let ladder = reg.by_name("core:ladder").unwrap();
        for y in 10..20 {
            w.set_block(1, y, 0, blocks::STONE); // the wall
            w.set_block(0, y, 0, ladder);
        }
        let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        let climb = PlayerInput {
            jump: true,
            ..default_input()
        };
        run(&mut s, climb, &w, 120);
        assert!(s.position.y > 14.0, "y = {}", s.position.y);
    }

    #[test]
    fn walking_moves_the_expected_direction() {
        let w = flat_world();
        let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        // Yaw 0 faces -Z.
        let input = PlayerInput {
            movement: Vec2::new(0.0, 1.0),
            ..default_input()
        };
        run(&mut s, input, &w, 60);
        assert!(s.position.z < -3.0, "z = {}", s.position.z);
        assert!((s.position.x - 0.5).abs() < 1e-4);
    }

    #[test]
    fn flying_ignores_gravity() {
        let w = flat_world();
        let mut s = PlayerState::new(Vec3::new(0.5, 20.0, 0.5));
        s.flying = true;
        run(&mut s, PlayerInput::default(), &w, 60);
        assert!((s.position.y - 20.0).abs() < 1e-3);
        let up = PlayerInput {
            jump: true,
            ..default_input()
        };
        run(&mut s, up, &w, 30);
        assert!(s.position.y > 23.0);
    }

    #[test]
    fn unloaded_chunks_are_solid() {
        let w = VoxelWorld::new();
        let mut s = PlayerState::new(Vec3::new(0.5, 50.0, 0.5));
        let reg = BlockRegistry::with_builtin_blocks();
        assert!(collides(&w, &reg, s.position));
        run(&mut s, PlayerInput::default(), &w, 10);
        assert!((s.position.y - 50.0).abs() < 0.01, "y = {}", s.position.y);
    }

    #[test]
    fn boats_float_and_row_faster_than_swimming() {
        let mut w = flat_world();
        for y in 10..20 {
            for x in -30..30 {
                for z in -3..3 {
                    w.set_block(x, y, z, BlockId(blocks::WATER.0));
                }
            }
        }
        // Row forward (facing +x) for two seconds, in a boat and swimming.
        let go = |boat: bool| {
            let mut s = PlayerState::new(Vec3::new(0.5, 19.5, 0.5));
            let input = PlayerInput {
                movement: Vec2::new(0.0, 1.0),
                yaw: -std::f32::consts::FRAC_PI_2,
                boat,
                ..PlayerInput::default()
            };
            run(&mut s, input, &w, 120);
            s
        };
        let (boat, swim) = (go(true), go(false));
        assert!(
            boat.position.x > swim.position.x + 3.0,
            "{} vs {}",
            boat.position.x,
            swim.position.x
        );
        // The boat stays at the surface (the top of the water is y = 20).
        assert!(
            (19.6..20.6).contains(&boat.position.y),
            "y = {}",
            boat.position.y
        );
        assert!(swim.position.y < 19.6, "a swimmer sinks a bit");
    }

    #[test]
    fn water_slows_falling() {
        let mut w = flat_world();
        for y in 10..20 {
            for x in -3..3 {
                for z in -3..3 {
                    w.set_block(x, y, z, BlockId(blocks::WATER.0));
                }
            }
        }
        let mut s = PlayerState::new(Vec3::new(0.5, 19.0, 0.5));
        run(&mut s, PlayerInput::default(), &w, 30);
        assert!(s.in_water);
        assert!(s.velocity.y > -2.0, "sinking too fast: {}", s.velocity.y);
    }

    #[test]
    fn auto_jump_climbs_single_steps_only() {
        let mut w = flat_world();
        // A one-block step at x = 2, and a two-block wall at x = 6.
        for z in -5..5 {
            for x in 2..10 {
                w.set_block(x, 10, z, blocks::STONE);
            }
            w.set_block(6, 11, z, blocks::STONE);
            w.set_block(6, 12, z, blocks::STONE);
        }
        let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        let input = PlayerInput {
            movement: Vec2::new(0.0, 1.0),
            yaw: -std::f32::consts::FRAC_PI_2, // face +X
            auto_jump: true,
            ..default_input()
        };
        run(&mut s, input, &w, 180);
        // Climbed onto the step (y = 11) but stopped at the tall wall.
        assert!((s.position.y - 11.0).abs() < 0.01, "y = {}", s.position.y);
        assert!(
            s.position.x < 6.0 && s.position.x > 5.0,
            "x = {}",
            s.position.x
        );
    }

    #[test]
    fn speed_multiplier_scales_and_freezes() {
        let w = flat_world();
        let walk = |speed| {
            let mut s = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
            let input = PlayerInput {
                movement: Vec2::new(0.0, 1.0),
                jump: true,
                speed,
                ..default_input()
            };
            run(&mut s, input, &w, 60);
            s
        };
        let normal = walk(1.0);
        let fast = walk(1.5);
        let frozen = walk(0.0);
        assert!(
            fast.position.z < normal.position.z * 1.3,
            "fast moves further"
        );
        assert!(
            (frozen.position.z - 0.5).abs() < 1e-4,
            "frozen doesn't move"
        );
        assert!(frozen.on_ground, "frozen can't jump");
    }

    #[test]
    fn ray_hits_boxes() {
        let b = Aabb::block(IVec3::new(5, 0, 0));
        let t = b.ray_hit(Vec3::new(0.0, 0.5, 0.5), Vec3::X, 10.0).unwrap();
        assert!((t - 5.0).abs() < 1e-5);
        assert!(b.ray_hit(Vec3::new(0.0, 0.5, 0.5), Vec3::X, 4.0).is_none());
        assert!(b.ray_hit(Vec3::new(0.0, 2.5, 0.5), Vec3::X, 10.0).is_none());
        assert!(
            b.ray_hit(Vec3::new(0.0, 0.5, 0.5), Vec3::NEG_X, 10.0)
                .is_none()
        );
    }

    #[test]
    fn aabb_block_iteration() {
        let b: Vec<IVec3> = Aabb::player(Vec3::new(0.5, 10.0, 0.5)).blocks().collect();
        // 0.2..0.8 in x and z, 10..11.8 in y: 1 × 2 × 1 blocks.
        assert_eq!(b, vec![IVec3::new(0, 10, 0), IVec3::new(0, 11, 0)]);
    }
}

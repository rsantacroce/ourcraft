//! Cloud portals: a standing frame of sunstone, lit with a feather, opens
//! the way to the Cloud Isles (and back).
//!
//! **The frame** is a flat ring of frame blocks round an empty rectangle,
//! at least 2 wide and 3 tall inside (a door-sized portal), at most 21 by
//! 21. The corners don't matter. To find it from the block that was
//! touched, we try each empty spot next to that block, in both upright
//! directions (along x and along z), and "flood" outwards through empty
//! spots in that flat plane. If the flood stays small, fills a perfect
//! rectangle and is closed in by frame blocks on every side, that
//! rectangle is the inside of a frame.
//!
//! **The far side.** When nobody has built a portal near where you come
//! out, a small one is made for you ([`structure`]), with a little floor
//! and some room to step out.

use glam::{IVec3, Vec3};

/// Smallest inside of a frame (width, height).
pub const MIN_INSIDE: (i32, i32) = (2, 3);
/// Largest inside of a frame, either way.
pub const MAX_INSIDE: i32 = 21;
/// Seconds to stand in a portal before it takes you.
pub const PORTAL_SECS: f32 = 2.5;

/// Which way a portal's flat face runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Along x (you walk through it going north or south).
    X,
    /// Along z.
    Z,
}

impl Axis {
    pub fn unit(self) -> IVec3 {
        match self {
            Axis::X => IVec3::X,
            Axis::Z => IVec3::Z,
        }
    }
}

/// The inside of a portal frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortalShape {
    /// The bottom corner with the lowest x and z.
    pub min: IVec3,
    pub axis: Axis,
    pub width: i32,
    pub height: i32,
}

impl PortalShape {
    /// Every spot inside the frame.
    pub fn cells(&self) -> impl Iterator<Item = IVec3> + '_ {
        let along = self.axis.unit();
        (0..self.height)
            .flat_map(move |y| (0..self.width).map(move |a| self.min + along * a + IVec3::Y * y))
    }

    pub fn contains(&self, p: IVec3) -> bool {
        self.cells().any(|c| c == p)
    }
}

/// Find the inside of a frame that `start` is in. `is_frame` says which
/// spots are frame blocks, `is_empty` which could hold the portal (air, or
/// an old portal block).
pub fn find_inside(
    start: IVec3,
    axis: Axis,
    is_frame: impl Fn(IVec3) -> bool,
    is_empty: impl Fn(IVec3) -> bool,
) -> Option<PortalShape> {
    if !is_empty(start) {
        return None;
    }
    let along = axis.unit();
    let steps = [along, -along, IVec3::Y, IVec3::NEG_Y];
    let mut seen = vec![start];
    let mut todo = vec![start];
    while let Some(p) = todo.pop() {
        for s in steps {
            let n = p + s;
            if seen.contains(&n) {
                continue;
            }
            if is_empty(n) {
                // Spilling out of any frame we'd allow: not a frame.
                if seen.len() >= (MAX_INSIDE * MAX_INSIDE) as usize {
                    return None;
                }
                seen.push(n);
                todo.push(n);
            } else if !is_frame(n) {
                return None; // a gap in the ring
            }
        }
    }
    // The flood must have filled a whole rectangle.
    let coord = |p: IVec3| p.dot(along);
    let (min_a, max_a) = seen.iter().fold((i32::MAX, i32::MIN), |(lo, hi), p| {
        (lo.min(coord(*p)), hi.max(coord(*p)))
    });
    let (min_y, max_y) = seen.iter().fold((i32::MAX, i32::MIN), |(lo, hi), p| {
        (lo.min(p.y), hi.max(p.y))
    });
    let (width, height) = (max_a - min_a + 1, max_y - min_y + 1);
    let fits = (MIN_INSIDE.0..=MAX_INSIDE).contains(&width)
        && (MIN_INSIDE.1..=MAX_INSIDE).contains(&height);
    if !fits || seen.len() != (width * height) as usize {
        return None;
    }
    // The corner with the lowest coordinates.
    let mut min = start;
    match axis {
        Axis::X => min.x = min_a,
        Axis::Z => min.z = min_a,
    }
    min.y = min_y;
    Some(PortalShape {
        min,
        axis,
        width,
        height,
    })
}

/// Touching the frame block at `clicked` lights the frame it's part of,
/// if there is one: the inside that should fill with portal.
pub fn light_from(
    clicked: IVec3,
    is_frame: impl Fn(IVec3) -> bool,
    is_empty: impl Fn(IVec3) -> bool,
) -> Option<PortalShape> {
    if !is_frame(clicked) {
        return None;
    }
    for axis in [Axis::X, Axis::Z] {
        let along = axis.unit();
        for side in [IVec3::Y, IVec3::NEG_Y, along, -along] {
            let start = clicked + side;
            if is_empty(start)
                && let Some(shape) = find_inside(start, axis, &is_frame, &is_empty)
            {
                return Some(shape);
            }
        }
    }
    None
}

/// What goes where in a portal built for someone arriving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Frame,
    Portal,
    /// Room to step out (cleared).
    Air,
    /// Something to step out onto (only where there's nothing solid).
    Floor,
}

/// A small portal (2 by 3 inside) whose bottom frame row starts at `base`,
/// running along x, with a floor and room to step out on both sides.
pub fn structure(base: IVec3) -> Vec<(IVec3, Part)> {
    let mut out = Vec::new();
    for x in 0..4 {
        for y in 0..5 {
            let ring = x == 0 || x == 3 || y == 0 || y == 4;
            let part = if ring { Part::Frame } else { Part::Portal };
            out.push((base + IVec3::new(x, y, 0), part));
        }
        for z in [-1, 1] {
            out.push((base + IVec3::new(x, 0, z), Part::Floor));
            for y in 1..4 {
                out.push((base + IVec3::new(x, y, z), Part::Air));
            }
        }
    }
    out
}

/// Where someone arriving through a [`structure`] at `base` stands (feet).
pub fn arrival(base: IVec3) -> Vec3 {
    base.as_vec3() + Vec3::new(1.5, 1.0, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A frame of `w`×`h` inside, along x at the origin, optionally with
    /// corners and with one block left out.
    fn frame(w: i32, h: i32, corners: bool, missing: Option<IVec3>) -> HashSet<IVec3> {
        let mut f = HashSet::new();
        for x in -1..=w {
            for y in -1..=h {
                let edge_x = x == -1 || x == w;
                let edge_y = y == -1 || y == h;
                if (edge_x && edge_y && !corners) || !(edge_x || edge_y) {
                    continue;
                }
                f.insert(IVec3::new(x, y, 0));
            }
        }
        if let Some(m) = missing {
            f.remove(&m);
        }
        f
    }

    fn light(f: &HashSet<IVec3>, clicked: IVec3) -> Option<PortalShape> {
        light_from(clicked, |p| f.contains(&p), |p| !f.contains(&p))
    }

    #[test]
    fn finds_a_door_sized_frame() {
        for corners in [true, false] {
            let f = frame(2, 3, corners, None);
            let shape = light(&f, IVec3::new(0, -1, 0)).expect("frame not found");
            assert_eq!(shape.min, IVec3::ZERO);
            assert_eq!((shape.axis, shape.width, shape.height), (Axis::X, 2, 3));
            assert_eq!(shape.cells().count(), 6);
            // Touching the side or the top works too.
            assert_eq!(light(&f, IVec3::new(2, 1, 0)), Some(shape));
            assert_eq!(light(&f, IVec3::new(1, 3, 0)), Some(shape));
        }
    }

    #[test]
    fn finds_big_frames_and_ones_along_z() {
        let f = frame(5, 7, false, None);
        let s = light(&f, IVec3::new(-1, 3, 0)).unwrap();
        assert_eq!((s.width, s.height), (5, 7));
        // The same frame turned round to run along z.
        let turned: HashSet<IVec3> = f.iter().map(|p| IVec3::new(4, p.y, p.x)).collect();
        let s = light(&turned, IVec3::new(4, -1, 2)).unwrap();
        assert_eq!(s.axis, Axis::Z);
        assert_eq!(s.min, IVec3::new(4, 0, 0));
    }

    #[test]
    fn broken_or_odd_frames_do_nothing() {
        // A gap in the side.
        let f = frame(2, 3, true, Some(IVec3::new(-1, 1, 0)));
        assert_eq!(light(&f, IVec3::new(0, -1, 0)), None);
        // Too small inside.
        let f = frame(1, 2, true, None);
        assert_eq!(light(&f, IVec3::new(0, -1, 0)), None);
        // Not a frame block at all.
        let f = frame(2, 3, true, None);
        assert_eq!(light(&f, IVec3::new(9, 9, 9)), None);
        // An L-shaped inside isn't a rectangle.
        let mut f = frame(3, 3, true, None);
        f.insert(IVec3::new(2, 0, 0));
        assert_eq!(light(&f, IVec3::new(0, -1, 0)), None);
        // A lone block on open ground: the flood runs off forever.
        let one: HashSet<IVec3> = [IVec3::ZERO].into();
        assert_eq!(light(&one, IVec3::ZERO), None);
    }

    #[test]
    fn built_portals_are_real_frames() {
        let base = IVec3::new(10, 64, -3);
        let parts = structure(base);
        let frames: HashSet<IVec3> = parts
            .iter()
            .filter(|(_, p)| *p == Part::Frame)
            .map(|(p, _)| *p)
            .collect();
        // Touching the bottom of the frame (not a corner) lights it.
        let touch = base + IVec3::X;
        let shape = light_from(touch, |p| frames.contains(&p), |p| !frames.contains(&p)).unwrap();
        let portal: HashSet<IVec3> = parts
            .iter()
            .filter(|(_, p)| *p == Part::Portal)
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(shape.cells().collect::<HashSet<_>>(), portal);
        // You arrive inside it, on its bottom row.
        let feet = arrival(base).floor().as_ivec3();
        assert!(shape.contains(feet));
        assert!(frames.contains(&(feet - IVec3::Y)));
    }
}

//! Dimensions: the overworld, and the Cloud Isles, a dreamy sky world of
//! floating islands reached through a cloud portal.
//!
//! **Stacked in one space.** The server keeps every dimension in one big
//! block space, one above the other: the overworld is where it always was
//! (`y` 0 to 255), and the Cloud Isles sit 512 blocks below it (`y` -512 to
//! -257). Nothing can cross the gap between them (the overworld has a
//! bedrock floor, and anything falling out of the Cloud Isles falls *away*
//! from the overworld). Because of that, everything on the server just
//! works in both: water flows, plants grow, carts roll, items drop, and
//! "which dimension is this in?" is simply a question about its height.
//!
//! **Clients only see their own.** A player's game never knows about the
//! stacking: it always gets its own dimension's blocks at the usual heights
//! (0 to 255), and never hears about anything in another dimension (blocks,
//! animals, other players). The server's network code moves positions up
//! and down on the way in and out (see `localize` in the server).
//!
//! Why not keep two separate worlds? Every part of the server (water,
//! farming, circuits, animals…) would need to be told which world to work
//! in. Stacking keeps them all unchanged, and the heights involved stay
//! small enough for exact physics (positions are 32-bit floats).

use serde::{Deserialize, Serialize};

use crate::world::WORLD_HEIGHT;

/// The places a player can be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dimension {
    /// Where everyone starts: hills, seas, caves.
    #[default]
    Overworld,
    /// Floating islands over an endless sky.
    CloudIsles,
}

/// Where the Cloud Isles start, in the server's stacked space.
const CLOUD_ISLES_BASE: i32 = -512;

/// Heights between the two dimensions belong to the nearer one: above
/// this, the overworld.
const SPLIT: f32 = -128.0;

impl Dimension {
    pub const ALL: [Dimension; 2] = [Dimension::Overworld, Dimension::CloudIsles];

    /// The server-space height of this dimension's `y = 0`.
    pub const fn base_y(self) -> i32 {
        match self {
            Dimension::Overworld => 0,
            Dimension::CloudIsles => CLOUD_ISLES_BASE,
        }
    }

    /// The server-space chunk height of this dimension's bottom chunk.
    pub const fn base_chunk_y(self) -> i32 {
        self.base_y() / crate::chunk::CHUNK_SIZE_I32
    }

    /// The dimension a server-space block height is in, if it's inside
    /// one (and not above, below or between them).
    pub fn of_block_y(y: i32) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|d| (d.base_y()..d.base_y() + WORLD_HEIGHT).contains(&y))
    }

    /// The dimension a server-space position is in (or nearest to: things
    /// flying high above or falling below a dimension still belong to it).
    pub fn of_y(y: f32) -> Self {
        if y >= SPLIT {
            Dimension::Overworld
        } else {
            Dimension::CloudIsles
        }
    }

    /// From this dimension's own heights (what players see) to the
    /// server's stacked space.
    pub fn to_server(self, local: [f32; 3]) -> [f32; 3] {
        [local[0], local[1] + self.base_y() as f32, local[2]]
    }

    /// From the server's stacked space to this dimension's own heights.
    pub fn to_local(self, server: [f32; 3]) -> [f32; 3] {
        [server[0], server[1] - self.base_y() as f32, server[2]]
    }

    /// [`Dimension::to_server`] for block positions.
    pub fn block_to_server(self, local: [i32; 3]) -> [i32; 3] {
        [local[0], local[1] + self.base_y(), local[2]]
    }

    /// [`Dimension::to_local`] for block positions.
    pub fn block_to_local(self, server: [i32; 3]) -> [i32; 3] {
        [server[0], server[1] - self.base_y(), server[2]]
    }

    /// The lowest and highest server-space heights a player can be at in
    /// this dimension (a little room above and below the blocks).
    pub fn player_range(self) -> (f32, f32) {
        let base = self.base_y() as f32;
        (base - 64.0, base + WORLD_HEIGHT as f32 + 64.0)
    }

    /// The portal leads here from this dimension.
    pub fn other(self) -> Self {
        match self {
            Dimension::Overworld => Dimension::CloudIsles,
            Dimension::CloudIsles => Dimension::Overworld,
        }
    }

    /// What players call it.
    pub fn display_name(self) -> &'static str {
        match self {
            Dimension::Overworld => "the overworld",
            Dimension::CloudIsles => "the Cloud Isles",
        }
    }

    /// Its name in save files (and its folder under `dimensions/`).
    pub fn save_name(self) -> &'static str {
        match self {
            Dimension::Overworld => "overworld",
            Dimension::CloudIsles => "cloud_isles",
        }
    }

    pub fn from_save_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.save_name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_do_not_overlap() {
        for y in -1000..1000 {
            let inside: Vec<_> = Dimension::ALL
                .iter()
                .filter(|d| (d.base_y()..d.base_y() + WORLD_HEIGHT).contains(&y))
                .collect();
            assert!(inside.len() <= 1, "two dimensions at y {y}");
        }
        // Each one's player range stays on its own side of the split.
        for d in Dimension::ALL {
            let (low, high) = d.player_range();
            assert_eq!(Dimension::of_y(low), d);
            assert_eq!(Dimension::of_y(high), d);
        }
    }

    #[test]
    fn heights_go_there_and_back() {
        let d = Dimension::CloudIsles;
        let local = [3.5, 100.25, -7.0];
        let server = d.to_server(local);
        assert_eq!(server[1], 100.25 - 512.0);
        assert_eq!(d.to_local(server), local);
        assert_eq!(Dimension::of_y(server[1]), d);
        assert_eq!(Dimension::of_block_y(server[1].floor() as i32), Some(d));
        assert_eq!(d.block_to_local(d.block_to_server([1, 2, 3])), [1, 2, 3]);
        // The overworld doesn't move.
        assert_eq!(Dimension::Overworld.to_server(local), local);
        assert_eq!(Dimension::of_block_y(64), Some(Dimension::Overworld));
        assert_eq!(Dimension::of_block_y(-100), None);
    }

    #[test]
    fn names_round_trip() {
        for d in Dimension::ALL {
            assert_eq!(Dimension::from_save_name(d.save_name()), Some(d));
            assert_eq!(d.other().other(), d);
        }
        assert_eq!(Dimension::from_save_name("moon"), None);
    }
}

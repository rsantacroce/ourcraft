//! Types shared by every part of Ourcraft.

/// This build's version: the release tag (`v0.2.0`), or for builds in
/// between, `git describe` (`v0.1.2-10-gac539ac`). See `build.rs`.
pub const VERSION: &str = env!("OURCRAFT_VERSION");

pub mod achievements;
pub mod adventure;
pub mod block;
pub mod chunk;
pub mod circuits;
pub mod cloud_isles;
pub mod colours;
pub mod container;
pub mod decoration;
pub mod dimension;
pub mod farming;
pub mod fluids;
pub mod inventory;
pub mod item;
pub mod magic;
pub mod mobs;
pub mod physics;
pub mod portal;
pub mod prediction;
pub mod protocol;
pub mod raycast;
pub mod recipes;
pub mod save;
pub mod shapes;
pub mod skin;
pub mod stones;
pub mod villages;
pub mod woods;
pub mod world;
pub mod worldgen;

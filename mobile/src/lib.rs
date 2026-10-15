//! Entry point for the iOS and Android builds.

use bevy::prelude::*;

/// `bevy_main` generates the boilerplate Android needs to start the app
/// (an `android_main` function). On iOS, `main.rs` calls this directly.
#[bevy_main]
pub fn main() {
    ourcraft_client::run();
}

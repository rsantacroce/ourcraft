//! Player-adjustable settings. (A settings screen arrives in M10; for now
//! render distance is changed with keys.)

use bevy::prelude::*;

use crate::render::{VoxelMaterial, VoxelMaterialHandle};

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GraphicsSettings>()
            .add_systems(Update, (adjust_render_distance, apply_fog).chain());
    }
}

#[derive(Resource, Debug, Clone)]
pub struct GraphicsSettings {
    /// How many columns of chunks to load in each direction.
    pub render_distance: i32,
}

impl GraphicsSettings {
    pub const MIN_RENDER_DISTANCE: i32 = 2;
    pub const MAX_RENDER_DISTANCE: i32 = 24;
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        // Tablets and phones get a shorter view distance by default.
        let mobile = cfg!(any(target_os = "ios", target_os = "android"));
        Self {
            render_distance: if mobile { 5 } else { 8 },
        }
    }
}

fn adjust_render_distance(keys: Res<ButtonInput<KeyCode>>, mut settings: ResMut<GraphicsSettings>) {
    let mut rd = settings.render_distance;
    if keys.just_pressed(KeyCode::BracketRight) {
        rd += 1;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        rd -= 1;
    }
    rd = rd.clamp(
        GraphicsSettings::MIN_RENDER_DISTANCE,
        GraphicsSettings::MAX_RENDER_DISTANCE,
    );
    if rd != settings.render_distance {
        settings.render_distance = rd;
        info!("Render distance: {rd} chunks");
    }
}

/// Fog ends just before the edge of the loaded area.
fn apply_fog(
    settings: Res<GraphicsSettings>,
    handle: Option<Res<VoxelMaterialHandle>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
) {
    let Some(handle) = handle else { return };
    if !settings.is_changed() && !handle.is_added() {
        return;
    }
    let end = (settings.render_distance as f32 - 0.5) * 16.0;
    for h in handle.both() {
        if let Some(mut mat) = materials.get_mut(h) {
            mat.params.fog_end = end;
            mat.params.fog_start = end * 0.55;
        }
    }
}

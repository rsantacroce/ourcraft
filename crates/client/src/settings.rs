//! Player settings, changed in the Settings screen and saved to a small
//! text file (`settings.txt` next to the worlds folder) so they stick.
//!
//! [`ClientSettings`] is the one source of truth; when it changes, the
//! resources other systems read ([`GraphicsSettings`], input sensitivity,
//! touch controls) are updated from it.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::input::InputSettings;
use crate::touch::TouchControls;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClientSettings::load())
            .init_resource::<GraphicsSettings>()
            .add_systems(
                Update,
                (adjust_render_distance, apply_settings, save_settings).chain(),
            );
    }
}

/// Everything the player can change, as saved on disk.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct ClientSettings {
    /// Your name in multiplayer.
    pub name: String,
    pub render_distance: i32,
    /// 1.0 = default.
    pub mouse_sensitivity: f32,
    /// Sound volume, 0..1.
    pub volume: f32,
    /// Play the calm music now and then.
    pub music: bool,
    /// `None` = automatic (on for touchscreens).
    pub touch_controls: Option<bool>,
    /// Hide rude words in chat.
    pub chat_filter: bool,
    /// Show the FPS/position text in the corner.
    pub show_debug: bool,
    /// How you look: a built-in skin's name, or `file:<name>.png` from the
    /// skins folder (see [`crate::skins`]).
    pub skin: String,
    /// Middle click picks the block you're looking at (switching the slot).
    pub middle_click_pick: bool,
}

impl GraphicsSettings {
    pub const MIN_RENDER_DISTANCE: i32 = 2;
    pub const MAX_RENDER_DISTANCE: i32 = 24;
}

impl Default for ClientSettings {
    fn default() -> Self {
        // Tablets and phones get a shorter view distance by default.
        let mobile = cfg!(any(target_os = "ios", target_os = "android"));
        Self {
            name: "Player".into(),
            render_distance: if mobile { 5 } else { 8 },
            mouse_sensitivity: 1.0,
            volume: 0.7,
            music: true,
            touch_controls: None,
            chat_filter: true,
            show_debug: !mobile,
            skin: "explorer".into(),
            middle_click_pick: true,
        }
    }
}

pub(crate) fn settings_path() -> PathBuf {
    let saves = crate::paths::saves_dir();
    saves
        .parent()
        .map_or_else(|| PathBuf::from("settings.txt"), |p| p.join("settings.txt"))
}

impl ClientSettings {
    /// Read `key = value` lines; anything missing or unreadable keeps its
    /// default.
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "name" if !value.is_empty() => {
                    s.name = ourcraft_common::protocol::sanitize_name(value)
                }
                "render_distance" => {
                    if let Ok(v) = value.parse::<i32>() {
                        s.render_distance = v.clamp(
                            GraphicsSettings::MIN_RENDER_DISTANCE,
                            GraphicsSettings::MAX_RENDER_DISTANCE,
                        );
                    }
                }
                "mouse_sensitivity" => {
                    if let Ok(v) = value.parse::<f32>() {
                        s.mouse_sensitivity = v.clamp(0.1, 5.0);
                    }
                }
                "volume" => {
                    if let Ok(v) = value.parse::<f32>() {
                        s.volume = v.clamp(0.0, 1.0);
                    }
                }
                "touch_controls" => {
                    s.touch_controls = match value {
                        "on" => Some(true),
                        "off" => Some(false),
                        _ => None,
                    }
                }
                "chat_filter" => s.chat_filter = value != "off",
                "music" => s.music = value != "off",
                "show_debug" => s.show_debug = value == "on",
                "skin" => s.skin = crate::skins::sanitize_skin_setting(value),
                "middle_click_pick" => s.middle_click_pick = value != "off",
                _ => {}
            }
        }
        s
    }

    pub fn to_text(&self) -> String {
        let onoff = |b: bool| if b { "on" } else { "off" };
        format!(
            "# Ourcraft settings (you can edit this file)\n\
             name = {}\nrender_distance = {}\nmouse_sensitivity = {:.2}\nvolume = {:.2}\n\
             music = {}\ntouch_controls = {}\nchat_filter = {}\nshow_debug = {}\n\
             skin = {}\nmiddle_click_pick = {}\n",
            self.name,
            self.render_distance,
            self.mouse_sensitivity,
            self.volume,
            onoff(self.music),
            match self.touch_controls {
                Some(true) => "on",
                Some(false) => "off",
                None => "auto",
            },
            onoff(self.chat_filter),
            onoff(self.show_debug),
            self.skin,
            onoff(self.middle_click_pick),
        )
    }

    pub fn load() -> Self {
        std::fs::read_to_string(settings_path())
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }
}

/// View distance, read by world streaming and networking.
#[derive(Resource, Debug, Clone)]
pub struct GraphicsSettings {
    /// How many columns of chunks to load in each direction.
    pub render_distance: i32,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            render_distance: ClientSettings::default().render_distance,
        }
    }
}

/// `[` and `]` change the render distance in game.
fn adjust_render_distance(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<crate::input::UiFocus>,
    mut settings: ResMut<ClientSettings>,
) {
    if focus.chat_open || focus.menu_open {
        return;
    }
    let mut rd = settings.render_distance;
    if keys.just_pressed(KeyCode::BracketRight) {
        rd += 1;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        rd -= 1;
    }
    let rd = rd.clamp(
        GraphicsSettings::MIN_RENDER_DISTANCE,
        GraphicsSettings::MAX_RENDER_DISTANCE,
    );
    if rd != settings.render_distance {
        settings.render_distance = rd;
        info!("Render distance: {rd} chunks");
    }
}

fn apply_settings(
    settings: Res<ClientSettings>,
    mut graphics: ResMut<GraphicsSettings>,
    mut input: ResMut<InputSettings>,
    mut touch: ResMut<TouchControls>,
) {
    if !settings.is_changed() {
        return;
    }
    if graphics.render_distance != settings.render_distance {
        graphics.render_distance = settings.render_distance;
    }
    input.mouse_sensitivity =
        InputSettings::default().mouse_sensitivity * settings.mouse_sensitivity;
    input.middle_click_pick = settings.middle_click_pick;
    if let Some(on) = settings.touch_controls {
        touch.enabled = on;
    }
}

/// Save a second after the last change (not on every click of a slider).
fn save_settings(time: Res<Time>, settings: Res<ClientSettings>, mut pending: Local<Option<f32>>) {
    if settings.is_changed() && !settings.is_added() {
        *pending = Some(1.0);
    }
    if let Some(t) = pending.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            *pending = None;
            let path = settings_path();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = std::fs::write(&path, settings.to_text()) {
                warn!("Couldn't save settings to {}: {e}", path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_text() {
        let s = ClientSettings {
            name: "Robin".into(),
            render_distance: 12,
            mouse_sensitivity: 1.5,
            volume: 0.25,
            music: false,
            touch_controls: Some(false),
            chat_filter: false,
            show_debug: true,
            skin: "file:My Skin.png".into(),
            middle_click_pick: false,
        };
        assert_eq!(ClientSettings::parse(&s.to_text()), s);
        let knight = ClientSettings {
            skin: "knight".into(),
            ..s
        };
        assert_eq!(ClientSettings::parse(&knight.to_text()), knight);
    }

    #[test]
    fn bad_values_fall_back_to_defaults() {
        let s = ClientSettings::parse("render_distance = 999\nvolume = loud\nname = \n");
        assert_eq!(s.render_distance, GraphicsSettings::MAX_RENDER_DISTANCE);
        assert_eq!(s.volume, ClientSettings::default().volume);
        assert_eq!(s.name, "Player");
        // An unknown skin (or a path out of the skins folder) is replaced.
        let s = ClientSettings::parse("skin = dragon\n");
        assert_eq!(s.skin, "explorer");
        let s = ClientSettings::parse("skin = file:../../x.png\n");
        assert_eq!(s.skin, "explorer");
    }
}

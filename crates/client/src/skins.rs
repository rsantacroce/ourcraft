//! Choosing how you look.
//!
//! The Settings screen offers the game's own skins plus any PNG files in
//! the `skins` folder next to the settings file (64×64 classic-layout
//! skins, or the older 64×32 ones). The choice is saved in settings as a
//! skin name (`explorer`) or `file:<name>.png`.
//!
//! Your skin is sent to the server when you join (and again if you change
//! it), and the server passes it on so everyone else sees it. A custom skin
//! travels as its 16 KB of pixels.

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use ourcraft_common::protocol::{Channel, ClientMessage};
use ourcraft_common::skin::{
    NetSkin, SKIN_BYTES, SKIN_SIZE, SkinError, check_skin_file, skin_from_rgba,
};

use crate::net::Connection;
use crate::render::skin_textures::{BUILTIN_SKINS, builtin_skin};
use crate::settings::ClientSettings;

/// Saved-setting prefix for a skin file from the skins folder.
const FILE_PREFIX: &str = "file:";

pub struct SkinsPlugin;

impl Plugin for SkinsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MySkin>()
            .init_resource::<SkinImages>()
            .add_systems(Startup, make_skins_folder)
            .add_systems(Update, (choose_skin, send_skin).chain());
    }
}

/// The skin you've chosen, ready to show and send.
#[derive(Resource, Default, Debug, Clone)]
pub struct MySkin {
    pub skin: NetSkin,
    /// Why the chosen skin file couldn't be used (we fall back to the
    /// default skin), to show in Settings.
    pub problem: Option<String>,
}

/// The `skins` folder, next to the settings file.
pub fn skins_dir() -> PathBuf {
    crate::settings::settings_path()
        .parent()
        .map_or_else(|| PathBuf::from("skins"), |p| p.join("skins"))
}

/// Every skin you can pick, as saved-setting values: the built-in ones,
/// then the PNG files in the skins folder (in name order).
pub fn skin_choices() -> Vec<String> {
    let mut choices: Vec<String> = BUILTIN_SKINS.iter().map(|(n, _)| n.to_string()).collect();
    choices.extend(
        skin_files(&skins_dir())
            .into_iter()
            .map(|f| format!("{FILE_PREFIX}{f}")),
    );
    choices
}

/// PNG file names in a folder.
fn skin_files(dir: &Path) -> Vec<String> {
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.to_lowercase().ends_with(".png") && !n.starts_with('.'))
        .collect();
    files.sort();
    files
}

/// The next (or previous) skin after `current`.
pub fn cycle_skin(current: &str, step: i32) -> String {
    let choices = skin_choices();
    let i = choices.iter().position(|c| c == current).unwrap_or(0) as i32;
    let n = choices.len() as i32;
    choices[(i + step).rem_euclid(n) as usize].clone()
}

/// How a skin setting is shown in Settings.
pub fn skin_label(value: &str) -> String {
    if let Some(file) = value.strip_prefix(FILE_PREFIX) {
        return file
            .trim_end_matches(".png")
            .trim_end_matches(".PNG")
            .to_owned();
    }
    BUILTIN_SKINS
        .iter()
        .find(|(n, _)| *n == value)
        .map_or_else(|| value.to_owned(), |(_, label)| label.to_string())
}

/// Clean up a saved skin setting: a built-in name, or a plain file name
/// (no folders, so it can only ever read from the skins folder).
pub fn sanitize_skin_setting(value: &str) -> String {
    let value = value.trim();
    if let Some(file) = value.strip_prefix(FILE_PREFIX) {
        let ok = !file.is_empty()
            && !file.contains(['/', '\\'])
            && !file.starts_with('.')
            && file.to_lowercase().ends_with(".png");
        if ok {
            return value.to_owned();
        }
    } else if BUILTIN_SKINS.iter().any(|(n, _)| *n == value) {
        return value.to_owned();
    }
    BUILTIN_SKINS[0].0.to_owned()
}

/// Read a skin file: check its header, decode it and check its size.
pub fn load_skin_file(path: &Path) -> Result<Vec<u8>, SkinError> {
    let bytes = std::fs::read(path).map_err(|e| SkinError::Unreadable(e.to_string()))?;
    check_skin_file(&bytes)?;
    let image = Image::from_buffer(
        &bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::nearest(),
        RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|e| SkinError::Unreadable(e.to_string()))?;
    let (width, height) = (image.width(), image.height());
    let rgba = image
        .convert(TextureFormat::Rgba8UnormSrgb)
        .and_then(|i| i.data)
        .ok_or(SkinError::BadData)?;
    skin_from_rgba(width, height, &rgba)
}

/// What a skin setting means: the skin to wear, and a problem to report
/// if a file couldn't be used.
pub fn resolve_skin(value: &str) -> (NetSkin, Option<String>) {
    let value = sanitize_skin_setting(value);
    match value.strip_prefix(FILE_PREFIX) {
        Some(file) => match load_skin_file(&skins_dir().join(file)) {
            Ok(pixels) => (NetSkin::Custom(pixels), None),
            Err(e) => {
                warn!("Skin {file} {e}; using the default skin");
                (NetSkin::default(), Some(format!("{file} {e}")))
            }
        },
        None => (NetSkin::Builtin(value), None),
    }
}

/// The pixels to draw for a skin (from the network or our settings).
/// Anything unknown or broken gets the default skin.
pub fn skin_pixels(skin: &NetSkin) -> Vec<u8> {
    match skin {
        NetSkin::Custom(pixels) if pixels.len() == SKIN_BYTES => Some(pixels.clone()),
        NetSkin::Builtin(name) => builtin_skin(name),
        NetSkin::Custom(_) => None,
    }
    .or_else(|| builtin_skin(BUILTIN_SKINS[0].0))
    .unwrap_or_else(|| vec![255; SKIN_BYTES])
}

/// Skin pictures already turned into textures, so ten players in the same
/// skin share one.
#[derive(Resource, Default)]
pub struct SkinImages(std::collections::HashMap<NetSkin, Handle<Image>>);

impl SkinImages {
    pub fn get(&mut self, skin: &NetSkin, images: &mut Assets<Image>) -> Handle<Image> {
        if let Some(h) = self.0.get(skin) {
            return h.clone();
        }
        // Custom skins come and go with players; don't keep them forever.
        if self.0.len() > 64 {
            self.0.retain(|k, _| matches!(k, NetSkin::Builtin(_)));
        }
        let mut image = Image::new(
            Extent3d {
                width: SKIN_SIZE,
                height: SKIN_SIZE,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            skin_pixels(skin),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::nearest();
        let handle = images.add(image);
        self.0.insert(skin.clone(), handle.clone());
        handle
    }
}

/// Make the skins folder, so it's there to drop files into.
fn make_skins_folder() {
    let _ = std::fs::create_dir_all(skins_dir());
}

/// Load the chosen skin when the setting changes.
fn choose_skin(settings: Res<ClientSettings>, mut mine: ResMut<MySkin>, mut chosen: Local<String>) {
    if !chosen.is_empty() && *chosen == settings.skin {
        return;
    }
    *chosen = settings.skin.clone();
    let (skin, problem) = resolve_skin(&settings.skin);
    *mine = MySkin { skin, problem };
}

/// Tell the server how we look: as soon as we've joined, and a second
/// after we last changed skin (so clicking through skins in Settings
/// doesn't send a picture per click).
fn send_skin(
    time: Res<Time>,
    mine: Res<MySkin>,
    conn: Option<ResMut<Connection>>,
    mut sent: Local<Option<NetSkin>>,
    mut since_change: Local<f32>,
) {
    let Some(mut conn) = conn.filter(|c| c.is_welcomed()) else {
        *sent = None; // a new game: send it again
        return;
    };
    if mine.is_changed() {
        *since_change = 0.0;
    }
    *since_change += time.delta_secs();
    if sent.as_ref() != Some(&mine.skin) && (sent.is_none() || *since_change > 1.0) {
        conn.send(Channel::Reliable, &ClientMessage::Skin(mine.skin.clone()));
        *sent = Some(mine.skin.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skin_settings_are_cleaned_up() {
        assert_eq!(sanitize_skin_setting("miner"), "miner");
        assert_eq!(sanitize_skin_setting("file:Sam.png"), "file:Sam.png");
        // No sneaking out of the skins folder, and no unknown skins.
        assert_eq!(sanitize_skin_setting("file:../secret.png"), "explorer");
        assert_eq!(sanitize_skin_setting("file:notes.txt"), "explorer");
        assert_eq!(sanitize_skin_setting("dragon"), "explorer");
        assert_eq!(skin_label("file:Sam.png"), "Sam");
        assert_eq!(skin_label("knight"), "Knight");
    }

    #[test]
    fn broken_skins_fall_back_to_the_default() {
        let default = skin_pixels(&NetSkin::default());
        assert_eq!(skin_pixels(&NetSkin::Custom(vec![1, 2, 3])), default);
        assert_eq!(skin_pixels(&NetSkin::Builtin("dragon".into())), default);
        let mine = vec![9; SKIN_BYTES];
        assert_eq!(skin_pixels(&NetSkin::Custom(mine.clone())), mine);
    }

    #[test]
    fn skin_files_are_loaded_and_checked() {
        let dir = std::env::temp_dir().join(format!("ourcraft-skins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A real (tiny) PNG, made with Bevy's own encoder.
        let good = Image::new(
            Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![200; SKIN_BYTES],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::all(),
        );
        let png = |img: Image, path: &Path| {
            img.try_into_dynamic().unwrap().save(path).unwrap();
        };
        png(good, &dir.join("good.png"));
        let small = Image::new(
            Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![200; 16 * 16 * 4],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::all(),
        );
        png(small, &dir.join("small.png"));
        std::fs::write(dir.join("junk.png"), b"not a picture").unwrap();

        assert_eq!(
            load_skin_file(&dir.join("good.png")).unwrap(),
            vec![200; SKIN_BYTES]
        );
        assert_eq!(
            load_skin_file(&dir.join("small.png")),
            Err(SkinError::WrongSize {
                width: 16,
                height: 16
            })
        );
        assert!(load_skin_file(&dir.join("junk.png")).is_err());
        assert!(load_skin_file(&dir.join("missing.png")).is_err());
        assert_eq!(skin_files(&dir), ["good.png", "junk.png", "small.png"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

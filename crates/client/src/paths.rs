//! Where things are stored on each platform.

use std::path::PathBuf;

/// Where worlds are saved on this device.
///
/// - macOS: `~/Library/Application Support/Ourcraft/worlds`
/// - Windows: `%APPDATA%\Ourcraft\worlds`
/// - Linux: `~/.local/share/ourcraft/worlds`
/// - iOS: the app's private `Documents/worlds`
/// - Android: the app's private internal storage
pub fn saves_dir() -> PathBuf {
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    #[cfg(target_os = "android")]
    {
        if let Some(path) = bevy::android::ANDROID_APP
            .get()
            .and_then(|app| app.internal_data_path())
        {
            return path.join("worlds");
        }
        PathBuf::from("worlds")
    }
    #[cfg(target_os = "ios")]
    {
        home().join("Documents").join("worlds")
    }
    #[cfg(target_os = "macos")]
    {
        home().join("Library/Application Support/Ourcraft/worlds")
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home)
            .join("Ourcraft")
            .join("worlds")
    }
    #[cfg(not(any(
        target_os = "android",
        target_os = "ios",
        target_os = "macos",
        target_os = "windows"
    )))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("ourcraft")
            .join("worlds")
    }
}

/// Turn a world name into a safe folder name.
pub fn folder_name(world_name: &str) -> String {
    let s: String = world_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    if s.is_empty() { "world".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(folder_name("My World"), "My_World");
        assert_eq!(folder_name("../../etc"), "______etc");
        assert_eq!(folder_name(""), "world");
    }
}

//! What mods keep with a world ([`HostCall::SaveData`]): a little text under
//! names, for each mod, in `mod_data/<mod>.dat` in the world's folder.
//! Written as soon as it changes (it's small), so nothing is lost if the
//! game stops.
//!
//! [`HostCall::SaveData`]: ourcraft_mod_api::HostCall::SaveData

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use ourcraft_mod_api::{MAX_DATA_KEY, MAX_DATA_KEYS, MAX_DATA_VALUE};

#[derive(Resource, Default)]
pub struct ModData {
    /// The world's `mod_data` folder (none: kept in memory only, for tests).
    dir: Option<PathBuf>,
    /// Each mod's data, once it has been read.
    stores: HashMap<String, BTreeMap<String, String>>,
}

impl ModData {
    pub fn new(world_dir: Option<&Path>) -> Self {
        Self {
            dir: world_dir.map(|d| d.join("mod_data")),
            stores: HashMap::new(),
        }
    }

    /// The file a mod's data lives in. Mod names come from file names, so
    /// anything odd in them is replaced.
    fn file(&self, mod_name: &str) -> Option<PathBuf> {
        let safe: String = mod_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Some(self.dir.as_ref()?.join(format!("{safe}.dat")))
    }

    fn store(&mut self, mod_name: &str) -> &mut BTreeMap<String, String> {
        if !self.stores.contains_key(mod_name) {
            let saved = self
                .file(mod_name)
                .and_then(|f| std::fs::read(f).ok())
                .and_then(|b| postcard::from_bytes::<Vec<(String, String)>>(&b).ok())
                .unwrap_or_default();
            self.stores
                .insert(mod_name.to_owned(), saved.into_iter().collect());
        }
        self.stores.get_mut(mod_name).expect("just inserted")
    }

    pub fn load(&mut self, mod_name: &str, key: &str) -> Option<String> {
        self.store(mod_name).get(key).cloned()
    }

    /// Keep `value` under `key` (or forget it), and write the mod's file.
    pub fn save(&mut self, mod_name: &str, key: &str, value: Option<String>) -> Result<(), String> {
        if key.is_empty() || key.len() > MAX_DATA_KEY {
            return Err(format!(
                "a name for saved data is 1 to {MAX_DATA_KEY} letters long"
            ));
        }
        if value.as_ref().is_some_and(|v| v.len() > MAX_DATA_VALUE) {
            return Err(format!(
                "that's too much to save under one name (at most {MAX_DATA_VALUE} bytes)"
            ));
        }
        let store = self.store(mod_name);
        match value {
            Some(v) => {
                if !store.contains_key(key) && store.len() >= MAX_DATA_KEYS {
                    return Err(format!(
                        "a mod can save under at most {MAX_DATA_KEYS} names"
                    ));
                }
                store.insert(key.to_owned(), v);
            }
            None => {
                store.remove(key);
            }
        }
        let entries: Vec<(String, String)> =
            store.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        if let Some(file) = self.file(mod_name)
            && let Err(e) = write(&file, &postcard::to_stdvec(&entries).unwrap_or_default())
        {
            warn!("Couldn't save {mod_name}'s data: {e}");
        }
        Ok(())
    }
}

/// Write a file safely: a temporary file first, then renamed over the old.
fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_is_kept_in_the_world_and_comes_back() {
        let dir = std::env::temp_dir().join(format!("ourcraft-moddata-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut data = ModData::new(Some(&dir));
        data.save("fishing", "journal", Some("[\"perch\"]".into()))
            .unwrap();
        data.save("fishing", "gone", Some("x".into())).unwrap();
        data.save("fishing", "gone", None).unwrap();
        data.save("other mod!", "journal", Some("theirs".into()))
            .unwrap();
        assert!(data.save("fishing", "", Some("x".into())).is_err());
        assert!(
            data.save("fishing", "big", Some("x".repeat(MAX_DATA_VALUE + 1)))
                .is_err()
        );

        // Next time the world opens.
        let mut again = ModData::new(Some(&dir));
        assert_eq!(
            again.load("fishing", "journal").as_deref(),
            Some("[\"perch\"]")
        );
        assert_eq!(again.load("fishing", "gone"), None);
        assert_eq!(
            again.load("other mod!", "journal").as_deref(),
            Some("theirs")
        );
        assert!(dir.join("mod_data/fishing.dat").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

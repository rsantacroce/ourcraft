//! Saving and loading the world (single player).
//!
//! The file formats live in `ourcraft_common::save`; this module decides
//! *when* to save:
//! - a changed column is queued for saving as soon as it changes,
//! - queued columns, the player and the world info are written every 30
//!   seconds (autosave),
//! - and once more when the game closes or a phone/tablet sends it to the
//!   background (mobile apps can be killed without warning after that).
//!
//! Writing to disk happens on a background thread so the game never stutters.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, futures::check_ready};
use bevy::window::AppLifecycle;
use ourcraft_common::block::BlockRegistry;
use ourcraft_common::save::{BlockRemap, META_VERSION, PlayerSave, WorldMeta, WorldStorage};
use ourcraft_common::world::ColumnPos;

use crate::interaction::PlayerInventory;
use crate::player::{GameMode, Player};
use crate::sky::TimeOfDay;
use crate::world::{ClientWorld, DEFAULT_SEED};

/// Seconds between autosaves.
const AUTOSAVE_SECS: f32 = 30.0;
/// The single-player player's save name.
pub const LOCAL_PLAYER: &str = "local";

pub struct PersistencePlugin;

impl Plugin for PersistencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LatestSnapshot>()
            .init_resource::<SaveTask>()
            .add_systems(Startup, install_save_guard)
            .add_systems(Update, (autosave, finish_save_task))
            // After Bevy's own exit check, which sends `AppExit` when the
            // window closes (also in `Last`).
            .add_systems(
                Last,
                save_on_exit_or_suspend.after(bevy::window::ExitSystems),
            );
    }
}

/// Which world to play, from the command line: `--world NAME --seed N`.
#[derive(Debug, Clone)]
pub struct WorldChoice {
    pub name: String,
    pub seed: Option<u64>,
}

impl WorldChoice {
    pub fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        Self {
            name: value("--world").unwrap_or_else(|| "My World".into()),
            seed: value("--seed").and_then(|s| s.parse().ok()),
        }
    }
}

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

/// The open world: its folder and metadata.
#[derive(Resource, Clone)]
pub struct Storage {
    pub storage: Arc<WorldStorage>,
    pub meta: WorldMeta,
    /// The local player's saved state, if this isn't a new world.
    pub player: Option<PlayerSave>,
    pub remap: Option<BlockRemap>,
}

/// Open (or create) a world. Falls back to an unsaved temporary folder if
/// the saves folder can't be written, so the game still starts.
pub fn open_world(choice: &WorldChoice, registry: &BlockRegistry) -> Storage {
    let dir = saves_dir().join(folder_name(&choice.name));
    let storage = WorldStorage::open(&dir).unwrap_or_else(|e| {
        error!(
            "Can't open world folder {}: {e}. Saving to a temp folder.",
            dir.display()
        );
        WorldStorage::open(std::env::temp_dir().join("ourcraft-unsaved"))
            .expect("no writable folder at all")
    });
    let existing = storage.load_meta().unwrap_or_else(|e| {
        error!("Can't read world info: {e}");
        None
    });
    let (meta, remap) = match existing {
        Some(mut meta) => {
            let remap = BlockRemap::new(&meta.block_names, registry);
            // From now on the world is saved with the current block ids.
            meta.block_names = registry.iter().map(|(_, d)| d.name.clone()).collect();
            info!("Loaded world '{}' (seed {})", meta.name, meta.seed);
            (meta, Some(remap))
        }
        None => {
            let seed = choice.seed.unwrap_or(DEFAULT_SEED);
            let meta = WorldMeta::new(&choice.name, seed, registry);
            if let Err(e) = storage.save_meta(&meta) {
                error!("Can't save world info: {e}");
            }
            info!(
                "Created world '{}' (seed {seed}) in {}",
                meta.name,
                dir.display()
            );
            (meta, None)
        }
    };
    storage.set_remap(remap.clone());
    let player = storage.load_player(LOCAL_PLAYER).unwrap_or_else(|e| {
        error!("Can't read player save: {e}");
        None
    });
    Storage {
        storage: Arc::new(storage),
        meta,
        player,
        remap,
    }
}

/// The latest world info and player state, refreshed every second so that
/// [`SaveOnDrop`] always has something recent to write.
#[derive(Resource, Clone, Default)]
pub struct LatestSnapshot(Arc<Mutex<Option<(WorldMeta, PlayerSave)>>>);

/// Writes everything to disk when dropped.
///
/// On macOS, Cmd+Q ends the app without running another frame; Bevy just
/// drops every resource. Doing the final save in `Drop` catches that case
/// as well as normal exits.
#[derive(Resource)]
struct SaveOnDrop {
    storage: Arc<WorldStorage>,
    snapshot: LatestSnapshot,
}

impl Drop for SaveOnDrop {
    fn drop(&mut self) {
        save_now(&self.storage, &self.snapshot);
    }
}

/// Synchronously write queued columns, world info and the player.
fn save_now(storage: &WorldStorage, snapshot: &LatestSnapshot) {
    if let Some((meta, player)) = snapshot.0.lock().unwrap().clone() {
        if let Err(e) = storage.save_meta(&meta) {
            error!("Saving world info failed: {e}");
        }
        if let Err(e) = storage.save_player(LOCAL_PLAYER, &player) {
            error!("Saving player failed: {e}");
        }
    }
    match storage.flush() {
        Ok(n) => info!("Saved world ({n} column(s))"),
        Err(e) => error!("Saving failed: {e}"),
    }
}

/// Remember a changed column for saving. Called right after every edit, so
/// nothing is lost however the game ends.
pub fn mark_column_changed(storage: &Storage, world: &ClientWorld, col: ColumnPos) {
    if let Some(chunks) = world.column(col) {
        storage.storage.queue_save(col, &chunks);
    }
}

/// The background write currently running, if any.
#[derive(Resource, Default)]
struct SaveTask(Option<Task<()>>);

/// Snapshot the player for saving.
fn player_save(player: &Player, inventory: &PlayerInventory) -> PlayerSave {
    let p = player.state.position;
    let mut save = PlayerSave {
        version: META_VERSION,
        position: [p.x, p.y, p.z],
        yaw: player.yaw,
        pitch: player.pitch,
        flying: player.state.flying,
        inventory: Vec::new(),
        selected: 0,
    };
    save.store_inventory(inventory);
    save
}

fn install_save_guard(
    mut commands: Commands,
    storage: Res<Storage>,
    snapshot: Res<LatestSnapshot>,
) {
    commands.insert_resource(SaveOnDrop {
        storage: storage.storage.clone(),
        snapshot: snapshot.clone(),
    });
}

/// Refresh the snapshot (every second) and write to disk (every 30 s).
fn autosave(
    time: Res<Time>,
    storage: Res<Storage>,
    snapshot: Res<LatestSnapshot>,
    tod: Res<TimeOfDay>,
    mode: Res<GameMode>,
    player: Single<&Player>,
    inventory: Res<PlayerInventory>,
    mut task: ResMut<SaveTask>,
    mut timers: Local<(f32, f32)>,
) {
    timers.0 += time.delta_secs();
    timers.1 += time.delta_secs();
    if timers.0 >= 1.0 {
        timers.0 = 0.0;
        let mut meta = storage.meta.clone();
        meta.time_of_day = tod.fraction;
        meta.creative = *mode == GameMode::Creative;
        *snapshot.0.lock().unwrap() = Some((meta, player_save(&player, &inventory)));
    }
    if timers.1 >= AUTOSAVE_SECS && task.0.is_none() {
        timers.1 = 0.0;
        let (s, snap) = (storage.storage.clone(), snapshot.clone());
        task.0 = Some(IoTaskPool::get().spawn(async move { save_now(&s, &snap) }));
    }
}

fn finish_save_task(mut task: ResMut<SaveTask>) {
    if let Some(t) = task.0.as_mut()
        && check_ready(t).is_some()
    {
        task.0 = None;
    }
}

/// Save right away when the window closes or the app is suspended (mobile
/// apps may be killed without warning once in the background).
fn save_on_exit_or_suspend(
    mut exit: MessageReader<AppExit>,
    mut lifecycle: MessageReader<AppLifecycle>,
    storage: Res<Storage>,
    snapshot: Res<LatestSnapshot>,
    tod: Res<TimeOfDay>,
    mode: Res<GameMode>,
    player: Single<&Player>,
    inventory: Res<PlayerInventory>,
) {
    let exiting = exit.read().count() > 0;
    let suspending = lifecycle
        .read()
        .any(|e| matches!(e, AppLifecycle::WillSuspend));
    if !exiting && !suspending {
        return;
    }
    let mut meta = storage.meta.clone();
    meta.time_of_day = tod.fraction;
    meta.creative = *mode == GameMode::Creative;
    *snapshot.0.lock().unwrap() = Some((meta, player_save(&player, &inventory)));
    save_now(&storage.storage, &snapshot);
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

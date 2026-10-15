//! The server's world: loading and generating columns players need,
//! applying edits, saving, and the time of day.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool, Task, futures::check_ready};
use ourcraft_common::block::{BlockId, BlockRegistry};
use ourcraft_common::chunk::Chunk;
use ourcraft_common::container::Container;
use ourcraft_common::item::ItemRegistry;
use ourcraft_common::save::{BlockRemap, SavedContainer, WorldMeta, WorldStorage, encode_column};
use ourcraft_common::world::{ColumnPos, DAY_LENGTH_SECS, VoxelWorld};
use ourcraft_common::worldgen::WorldGenerator;

use crate::ServerConfig;

/// Seconds between autosaves.
const AUTOSAVE_SECS: f32 = 30.0;
/// Max columns loading/generating at once.
const MAX_LOADING: usize = 32;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        let config = app.world().resource::<ServerConfig>().clone();
        let (world, mods, registry, content) = open_world(&config);
        app.insert_resource(world)
            .insert_resource(mods)
            .insert_resource(content)
            .insert_resource(Registry(Arc::new(registry)))
            .init_resource::<ServerWorld>()
            .init_resource::<ColumnCache>()
            .init_resource::<Loading>()
            .init_resource::<WantedColumns>()
            .init_resource::<SaveTask>()
            .add_systems(Startup, install_save_guard)
            .add_systems(
                Update,
                (
                    load_wanted_columns,
                    receive_loaded_columns,
                    unload_unwanted_columns,
                    advance_time,
                    autosave,
                )
                    .chain()
                    .in_set(WorldSet),
            );
    }
}

/// World systems: run after players have said which columns they want.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldSet;

#[derive(Resource, Clone)]
pub struct Registry(pub Arc<BlockRegistry>);

/// All loaded columns.
#[derive(Resource, Default, Deref, DerefMut)]
pub struct ServerWorld(pub VoxelWorld);

/// The world on disk, its info, and everything derived from it.
#[derive(Resource)]
pub struct OpenWorld {
    pub storage: Arc<WorldStorage>,
    pub meta: WorldMeta,
    pub generator: Arc<WorldGenerator>,
    /// Where new players appear (block position to stand on).
    pub spawn: IVec3,
    /// Translation for block ids in old saves.
    pub remap: Option<BlockRemap>,
    /// Every item (blocks first), for inventories and drops.
    pub items: Arc<ItemRegistry>,
    /// Furnaces (and other blocks holding items), by position.
    pub containers: HashMap<IVec3, Container>,
    /// Every kind of animal.
    pub mobs: Arc<Vec<ourcraft_common::mobs::MobDef>>,
    /// What signs say, by position.
    pub signs: HashMap<IVec3, Vec<String>>,
    /// Where each player last slept (their respawn point).
    pub beds: HashMap<String, IVec3>,
    /// No monsters and no hunger.
    pub peaceful: bool,
    /// Shared with the save-on-exit guard.
    snapshot: Arc<Mutex<Snapshot>>,
}

/// What gets written on save, copied from the live world.
#[derive(Clone)]
struct Snapshot {
    meta: WorldMeta,
    containers: Vec<SavedContainer>,
    signs: Vec<([i32; 3], Vec<String>)>,
    beds: Vec<(String, [i32; 3])>,
    rules: Vec<(String, bool)>,
}

/// Columns encoded for sending, so several players near the same place
/// don't each cost an encode. Cleared for a column when it changes.
#[derive(Resource, Default)]
pub struct ColumnCache(HashMap<ColumnPos, Arc<Vec<u8>>>);

#[derive(Resource, Default)]
struct Loading(HashMap<ColumnPos, Task<Vec<Chunk>>>);

/// Columns some player needs, nearest-first priority (lower = sooner).
/// Filled by the players module every tick.
#[derive(Resource, Default)]
pub struct WantedColumns {
    pub columns: HashMap<ColumnPos, i32>,
    /// Loaded columns to keep even if nobody needs them right now (a margin
    /// so walking back and forth doesn't reload them).
    pub keep: HashSet<ColumnPos>,
}

#[derive(Resource, Default)]
struct SaveTask(Option<Task<()>>);

/// Open the world folder, load mods (they may add blocks), and work out
/// the block registry.
fn open_world(
    config: &ServerConfig,
) -> (
    OpenWorld,
    crate::mods::Mods,
    BlockRegistry,
    crate::mods::Content,
) {
    let storage = WorldStorage::open(&config.world_dir).unwrap_or_else(|e| {
        panic!(
            "can't open world folder {}: {e}",
            config.world_dir.display()
        )
    });
    let existing = storage.load_meta().unwrap_or_else(|e| {
        error!("Can't read world info ({e}); starting a new world");
        None
    });
    // An existing world keeps its game mode; a new one uses the config's.
    let game_mode = match &existing {
        Some(meta) => meta.game_mode.clone(),
        None => config.game_mode.clone(),
    };
    let (mods, registry, content) = crate::mods::load_mods(
        config.mods_dir.as_deref(),
        config.bundled_mods,
        game_mode.clone(),
    );
    let registry_ref = &registry;
    let (meta, remap) = match existing {
        Some(mut meta) => {
            let registry = registry_ref;
            upgrade_players(&storage, &meta.block_names);
            let remap = BlockRemap::new(&meta.block_names, registry);
            meta.block_names = registry.iter().map(|(_, d)| d.name.clone()).collect();
            info!("Loaded world '{}' (seed {})", meta.name, meta.seed);
            (meta, Some(remap))
        }
        None => {
            let seed = config.seed.unwrap_or_else(random_seed);
            let mut meta = WorldMeta::new(&config.world_name, seed, registry_ref);
            meta.creative = config.creative;
            meta.game_mode = game_mode.clone();
            if let Err(e) = storage.save_meta(&meta) {
                error!("Can't save world info: {e}");
            }
            info!(
                "Created world '{}' (seed {seed}) in {}",
                meta.name,
                config.world_dir.display()
            );
            (meta, None)
        }
    };
    storage.set_remap(remap.clone());
    let generator = Arc::new(WorldGenerator::new(meta.seed));
    let (x, y, z) = generator.find_spawn();
    let items = Arc::new(ItemRegistry::from_defs(content.items.clone()));
    let containers = storage
        .load_containers()
        .unwrap_or_else(|e| {
            error!("Can't read furnaces ({e})");
            Vec::new()
        })
        .iter()
        .map(|c| (IVec3::from_array(c.pos), c.restore(&items)))
        .collect();
    let signs = storage
        .load_signs()
        .unwrap_or_else(|e| {
            error!("Can't read signs ({e})");
            Vec::new()
        })
        .into_iter()
        .map(|(p, l)| (IVec3::from_array(p), l))
        .collect();
    let beds = storage
        .load_beds()
        .unwrap_or_else(|e| {
            error!("Can't read beds ({e})");
            Vec::new()
        })
        .into_iter()
        .map(|(n, p)| (n, IVec3::from_array(p)))
        .collect();
    let snapshot = Snapshot {
        meta: meta.clone(),
        containers: Vec::new(),
        signs: Vec::new(),
        beds: Vec::new(),
        rules: Vec::new(),
    };
    let peaceful = storage
        .load_rules()
        .unwrap_or_default()
        .iter()
        .any(|(k, v)| k == "peaceful" && *v);
    let open = OpenWorld {
        storage: Arc::new(storage),
        snapshot: Arc::new(Mutex::new(snapshot)),
        meta,
        generator,
        spawn: IVec3::new(x, y, z),
        remap,
        items,
        containers,
        mobs: Arc::new(content.mobs.clone()),
        signs,
        beds,
        peaceful,
    };
    // The save guard must never write an empty furnace list over real ones.
    open.update_snapshot();
    (open, mods, registry, crate::mods::Content(content))
}

fn random_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    ourcraft_common::worldgen::splitmix64(t)
}

/// Player files from before milestone 11 hold block ids, which only make
/// sense with the block names the world was saved with. Convert them all
/// now, before those names are replaced by the current ones.
fn upgrade_players(storage: &WorldStorage, saved_block_names: &[String]) {
    for name in storage.player_names() {
        match storage.load_player(&name, saved_block_names) {
            Ok(Some(p)) => {
                if let Err(e) = storage.save_player(&name, &p) {
                    error!("Can't upgrade player {name}: {e}");
                }
            }
            Ok(None) => {}
            Err(e) => error!("Can't read player {name}: {e}"),
        }
    }
}

impl OpenWorld {
    pub fn time_of_day(&self) -> f32 {
        self.meta.time_of_day
    }

    /// Copy what needs saving for the background saver.
    fn update_snapshot(&self) {
        let containers = self
            .containers
            .iter()
            .map(|(pos, c)| SavedContainer::new(pos.to_array(), c, &self.items))
            .collect();
        *self.snapshot.lock().unwrap() = Snapshot {
            meta: self.meta.clone(),
            containers,
            signs: self
                .signs
                .iter()
                .map(|(p, l)| (p.to_array(), l.clone()))
                .collect(),
            beds: self
                .beds
                .iter()
                .map(|(n, p)| (n.clone(), p.to_array()))
                .collect(),
            rules: vec![("peaceful".into(), self.peaceful)],
        };
    }
}

/// The encoded column, from the cache or freshly encoded.
pub fn encoded_column(
    cache: &mut ColumnCache,
    world: &ServerWorld,
    pos: ColumnPos,
) -> Option<Arc<Vec<u8>>> {
    if let Some(bytes) = cache.0.get(&pos) {
        return Some(bytes.clone());
    }
    let chunks = world.column(pos)?;
    let bytes = Arc::new(encode_column(&chunks));
    cache.0.insert(pos, bytes.clone());
    Some(bytes)
}

/// Change a block, invalidate caches and queue the column for saving.
/// Returns the old block, or `None` if its column isn't loaded.
pub fn set_block(
    world: &mut ServerWorld,
    cache: &mut ColumnCache,
    open: &OpenWorld,
    pos: IVec3,
    block: BlockId,
) -> Option<BlockId> {
    let old = world.set_block(pos.x, pos.y, pos.z, block)?;
    let col = ColumnPos::from_block(pos.x, pos.z);
    cache.0.remove(&col);
    if let Some(chunks) = world.column(col) {
        open.storage.queue_save(col, &chunks);
    }
    Some(old)
}

fn load_wanted_columns(
    world: Res<ServerWorld>,
    open: Res<OpenWorld>,
    wanted: Res<WantedColumns>,
    mut loading: ResMut<Loading>,
) {
    let budget = MAX_LOADING.saturating_sub(loading.0.len());
    if budget == 0 {
        return;
    }
    let mut missing: Vec<(ColumnPos, i32)> = wanted
        .columns
        .iter()
        .filter(|(p, _)| !world.has_column(**p) && !loading.0.contains_key(p))
        .map(|(p, d)| (*p, *d))
        .collect();
    missing.sort_by_key(|(_, d)| *d);
    let pool = AsyncComputeTaskPool::get();
    for (pos, _) in missing.into_iter().take(budget) {
        let storage = open.storage.clone();
        let generator = open.generator.clone();
        // A saved column if there is one, otherwise fresh terrain.
        let task = pool.spawn(async move {
            match storage.load_column(pos) {
                Ok(Some(chunks)) => chunks,
                Ok(None) => generator.generate_column(pos),
                Err(e) => {
                    error!("Column {pos:?} is unreadable ({e}); regenerating it");
                    generator.generate_column(pos)
                }
            }
        });
        loading.0.insert(pos, task);
    }
}

fn receive_loaded_columns(mut world: ResMut<ServerWorld>, mut loading: ResMut<Loading>) {
    let mut done = Vec::new();
    loading.0.retain(|pos, task| match check_ready(task) {
        Some(chunks) => {
            done.push((*pos, chunks));
            false
        }
        None => true,
    });
    for (pos, chunks) in done {
        world.insert_column(pos, chunks);
    }
}

fn unload_unwanted_columns(
    mut world: ResMut<ServerWorld>,
    mut cache: ResMut<ColumnCache>,
    wanted: Res<WantedColumns>,
    mut loading: ResMut<Loading>,
) {
    let unwanted: Vec<ColumnPos> = world
        .columns()
        .filter(|p| !wanted.columns.contains_key(p) && !wanted.keep.contains(p))
        .collect();
    for pos in unwanted {
        // Changed columns were queued for saving when they changed.
        world.remove_column(pos);
        cache.0.remove(&pos);
    }
    loading
        .0
        .retain(|p, _| wanted.columns.contains_key(p) || wanted.keep.contains(p));
}

fn advance_time(time: Res<Time>, mut open: ResMut<OpenWorld>) {
    open.meta.time_of_day = (open.meta.time_of_day + time.delta_secs() / DAY_LENGTH_SECS).fract();
}

/// Writes everything when dropped, so even an abrupt shutdown saves.
#[derive(Resource)]
struct SaveOnDrop {
    storage: Arc<WorldStorage>,
    snapshot: Arc<Mutex<Snapshot>>,
}

impl Drop for SaveOnDrop {
    fn drop(&mut self) {
        save_now(&self.storage, &self.snapshot);
    }
}

fn save_now(storage: &WorldStorage, snapshot: &Mutex<Snapshot>) {
    let snapshot = snapshot.lock().unwrap().clone();
    if let Err(e) = storage.save_meta(&snapshot.meta) {
        error!("Saving world info failed: {e}");
    }
    if let Err(e) = storage.save_containers(snapshot.containers) {
        error!("Saving furnaces failed: {e}");
    }
    if let Err(e) = storage.save_signs(snapshot.signs) {
        error!("Saving signs failed: {e}");
    }
    if let Err(e) = storage.save_beds(snapshot.beds) {
        error!("Saving beds failed: {e}");
    }
    if let Err(e) = storage.save_rules(snapshot.rules) {
        error!("Saving world rules failed: {e}");
    }
    match storage.flush() {
        Ok(n) if n > 0 => info!("Saved {n} column(s)"),
        Ok(_) => {}
        Err(e) => error!("Saving failed: {e}"),
    }
}

fn install_save_guard(mut commands: Commands, open: Res<OpenWorld>) {
    commands.insert_resource(SaveOnDrop {
        storage: open.storage.clone(),
        snapshot: open.snapshot.clone(),
    });
}

fn autosave(
    time: Res<Time>,
    open: Res<OpenWorld>,
    mut task: ResMut<SaveTask>,
    mut timer: Local<f32>,
) {
    if let Some(t) = task.0.as_mut()
        && check_ready(t).is_some()
    {
        task.0 = None;
    }
    *timer += time.delta_secs();
    if *timer >= AUTOSAVE_SECS && task.0.is_none() {
        *timer = 0.0;
        open.update_snapshot();
        let (storage, snapshot) = (open.storage.clone(), open.snapshot.clone());
        task.0 = Some(IoTaskPool::get().spawn(async move { save_now(&storage, &snapshot) }));
    }
}

/// Save everything now (on shutdown).
pub fn save_all(open: &OpenWorld) {
    open.update_snapshot();
    save_now(&open.storage, &open.snapshot);
}

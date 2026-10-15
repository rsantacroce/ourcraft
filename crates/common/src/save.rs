//! Saving worlds to disk.
//!
//! **What gets saved.** The terrain is generated from the seed, so only
//! columns that players have *changed* need saving; everything else is
//! regenerated identically when needed. That keeps saves tiny.
//!
//! **Region files.** Saving every column as its own file would mean
//! thousands of tiny files. Instead, columns are grouped 32×32 into *region
//! files* (`region/r.X.Z.ocr`). Each file starts with a table saying where
//! each column's data lives inside it. Data is stored in 4 KiB *sectors*; a
//! column that grows beyond its sectors moves to the end of the file.
//!
//! **Compression.** Each column is encoded with `postcard` (a compact binary
//! format) and compressed with LZ4, which is very fast to decompress.
//!
//! **Block ids.** Mods can add blocks, so a block's numeric id may differ
//! between runs. The world metadata stores the *names* of all blocks at save
//! time; on load, ids are translated to the current registry. Thanks to
//! palette compression this only touches each chunk's small palette.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::block::{BlockId, BlockRegistry};
use crate::chunk::Chunk;
use crate::inventory::{INVENTORY_SLOTS, Inventory, ItemStack};
use crate::item::ItemRegistry;
use crate::world::{ColumnPos, WORLD_HEIGHT_CHUNKS};

/// Columns per region side.
pub const REGION_SIZE: i32 = 32;
const COLUMNS_PER_REGION: usize = (REGION_SIZE * REGION_SIZE) as usize;
const SECTOR: u64 = 4096;
const MAGIC: &[u8; 4] = b"OCRG";
const REGION_VERSION: u32 = 1;
/// Magic + version + one (sector offset, byte length) pair per column.
const HEADER_BYTES: u64 = 8 + COLUMNS_PER_REGION as u64 * 8;
const HEADER_SECTORS: u64 = HEADER_BYTES.div_ceil(SECTOR);

/// Version of the column encoding inside region files.
const COLUMN_FORMAT: u8 = 1;
/// Version of `world.dat`.
pub const META_VERSION: u32 = 2;
/// Version of player files. 3: inventories by item name (milestone 11).
pub const PLAYER_VERSION: u32 = 3;

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// Which region file a column lives in.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RegionPos {
    pub x: i32,
    pub z: i32,
}

impl RegionPos {
    pub fn of(col: ColumnPos) -> Self {
        Self {
            x: col.x.div_euclid(REGION_SIZE),
            z: col.z.div_euclid(REGION_SIZE),
        }
    }

    /// Index of a column within its region's table.
    fn local_index(col: ColumnPos) -> usize {
        (col.x.rem_euclid(REGION_SIZE) + col.z.rem_euclid(REGION_SIZE) * REGION_SIZE) as usize
    }

    fn file_name(self) -> String {
        format!("r.{}.{}.ocr", self.x, self.z)
    }
}

/// One open region file.
pub struct RegionFile {
    file: File,
    /// (first sector, length in bytes) per column; (0, 0) = not saved.
    entries: Vec<(u32, u32)>,
    /// First sector past the end of the data.
    end_sector: u64,
}

impl RegionFile {
    /// Open a region file, creating it if it doesn't exist.
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let len = file.metadata()?.len();
        let mut entries = vec![(0u32, 0u32); COLUMNS_PER_REGION];
        if len == 0 {
            // New file: write an empty header.
            let mut header = Vec::with_capacity((HEADER_SECTORS * SECTOR) as usize);
            header.extend_from_slice(MAGIC);
            header.extend_from_slice(&REGION_VERSION.to_le_bytes());
            header.resize((HEADER_SECTORS * SECTOR) as usize, 0);
            file.write_all(&header)?;
            file.sync_data()?;
        } else {
            let mut header = vec![0u8; HEADER_BYTES as usize];
            file.read_exact(&mut header)?;
            if &header[0..4] != MAGIC {
                return Err(invalid("not an Ourcraft region file"));
            }
            let version = u32::from_le_bytes(header[4..8].try_into().unwrap());
            if version != REGION_VERSION {
                return Err(invalid(format!("unsupported region version {version}")));
            }
            for (i, e) in entries.iter_mut().enumerate() {
                let o = 8 + i * 8;
                let sector = u32::from_le_bytes(header[o..o + 4].try_into().unwrap());
                let bytes = u32::from_le_bytes(header[o + 4..o + 8].try_into().unwrap());
                *e = (sector, bytes);
            }
        }
        let end_sector = entries
            .iter()
            .map(|&(s, b)| s as u64 + (b as u64).div_ceil(SECTOR))
            .max()
            .unwrap_or(0)
            .max(HEADER_SECTORS);
        Ok(Self {
            file,
            entries,
            end_sector,
        })
    }

    /// Read a column's raw (compressed) bytes, if it was saved.
    pub fn read(&mut self, col: ColumnPos) -> io::Result<Option<Vec<u8>>> {
        let (sector, bytes) = self.entries[RegionPos::local_index(col)];
        if bytes == 0 {
            return Ok(None);
        }
        let mut data = vec![0u8; bytes as usize];
        self.file.seek(SeekFrom::Start(sector as u64 * SECTOR))?;
        self.file.read_exact(&mut data)?;
        Ok(Some(data))
    }

    /// Write a column's raw (compressed) bytes.
    pub fn write(&mut self, col: ColumnPos, data: &[u8]) -> io::Result<()> {
        let i = RegionPos::local_index(col);
        let (old_sector, old_bytes) = self.entries[i];
        let needed = (data.len() as u64).div_ceil(SECTOR);
        let available = (old_bytes as u64).div_ceil(SECTOR);
        // Reuse the old space if it's big enough, otherwise append.
        let sector = if old_bytes > 0 && needed <= available {
            old_sector as u64
        } else {
            let s = self.end_sector;
            self.end_sector += needed;
            s
        };
        // Data first, then the table entry, so a crash mid-write leaves the
        // old version intact.
        self.file.seek(SeekFrom::Start(sector * SECTOR))?;
        self.file.write_all(data)?;
        let pad = (needed * SECTOR) as usize - data.len();
        self.file.write_all(&vec![0u8; pad])?;
        self.file.seek(SeekFrom::Start(8 + i as u64 * 8))?;
        self.file.write_all(&(sector as u32).to_le_bytes())?;
        self.file.write_all(&(data.len() as u32).to_le_bytes())?;
        self.entries[i] = (sector as u32, data.len() as u32);
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct ChunkData {
    palette: Vec<u16>,
    bits: u8,
    data: Vec<u64>,
}

#[derive(Serialize, Deserialize)]
struct ColumnData {
    format: u8,
    chunks: Vec<ChunkData>,
}

/// Encode and compress a column (bottom chunk first).
pub fn encode_column(chunks: &[Chunk]) -> Vec<u8> {
    let column = ColumnData {
        format: COLUMN_FORMAT,
        chunks: chunks
            .iter()
            .map(|c| {
                let mut c = c.clone();
                c.compact();
                let (bits, data) = c.packed();
                ChunkData {
                    palette: c.palette().iter().map(|b| b.0).collect(),
                    bits,
                    data: data.to_vec(),
                }
            })
            .collect(),
    };
    let bytes = postcard::to_allocvec(&column).expect("serializing a column can't fail");
    lz4_flex::compress_prepend_size(&bytes)
}

/// Decompress and decode a column. `remap` translates saved block ids to
/// current ones (see [`BlockRemap`]).
pub fn decode_column(bytes: &[u8], remap: Option<&BlockRemap>) -> io::Result<Vec<Chunk>> {
    let raw = lz4_flex::decompress_size_prepended(bytes).map_err(|e| invalid(e.to_string()))?;
    let column: ColumnData = postcard::from_bytes(&raw).map_err(|e| invalid(e.to_string()))?;
    if column.format != COLUMN_FORMAT {
        return Err(invalid(format!(
            "unsupported column format {}",
            column.format
        )));
    }
    if column.chunks.len() != WORLD_HEIGHT_CHUNKS as usize {
        return Err(invalid("wrong number of chunks in column"));
    }
    column
        .chunks
        .into_iter()
        .map(|c| {
            let palette = c
                .palette
                .into_iter()
                .map(|id| remap.map_or(BlockId(id), |r| r.map(BlockId(id))))
                .collect();
            Chunk::from_packed(palette, c.bits, c.data).ok_or_else(|| invalid("corrupt chunk"))
        })
        .collect()
}

/// Translates block ids from a save file to the current registry by name.
#[derive(Debug, Clone)]
pub struct BlockRemap {
    table: Vec<BlockId>,
}

impl BlockRemap {
    /// `saved_names[i]` is the name block id `i` had when the world was saved.
    /// Blocks that no longer exist (say, from a removed mod) become air.
    pub fn new(saved_names: &[String], registry: &BlockRegistry) -> Self {
        Self {
            table: saved_names
                .iter()
                .map(|n| registry.by_name(n).unwrap_or(BlockId::AIR))
                .collect(),
        }
    }

    pub fn map(&self, id: BlockId) -> BlockId {
        self.table
            .get(id.0 as usize)
            .copied()
            .unwrap_or(BlockId::AIR)
    }

    /// True if every id maps to itself (no translation needed).
    pub fn is_identity(&self) -> bool {
        self.table
            .iter()
            .enumerate()
            .all(|(i, b)| b.0 as usize == i)
    }
}

/// Everything about a world that isn't terrain.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WorldMeta {
    pub version: u32,
    pub name: String,
    pub seed: u64,
    /// Time of day, 0..1 (see the client's sky module).
    pub time_of_day: f32,
    /// Block names by id when saved, for [`BlockRemap`].
    pub block_names: Vec<String>,
    /// True for creative worlds.
    pub creative: bool,
    /// A game mode provided by a mod (e.g. `"team_shooter"`), if any.
    pub game_mode: Option<String>,
}

/// `world.dat` as saved by version 1 (before game modes).
#[derive(Deserialize)]
struct WorldMetaV1 {
    version: u32,
    name: String,
    seed: u64,
    time_of_day: f32,
    block_names: Vec<String>,
    creative: bool,
}

impl WorldMeta {
    pub fn new(name: &str, seed: u64, registry: &BlockRegistry) -> Self {
        Self {
            version: META_VERSION,
            name: name.to_owned(),
            seed,
            time_of_day: 0.3,
            block_names: registry.iter().map(|(_, d)| d.name.clone()).collect(),
            creative: true,
            game_mode: None,
        }
    }
}

/// One saved inventory slot. Items are saved by name, like blocks.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SavedStack {
    pub item: String,
    pub count: u16,
    pub wear: u16,
}

/// A player's saved state.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PlayerSave {
    pub version: u32,
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub flying: bool,
    pub inventory: Vec<Option<SavedStack>>,
    pub selected: usize,
    /// Playing in creative mode (players can switch in any world).
    pub creative: bool,
}

/// A player file as saved by version 2 (block ids, no items yet).
#[derive(Deserialize)]
struct PlayerSaveV2 {
    version: u32,
    position: [f32; 3],
    yaw: f32,
    pitch: f32,
    flying: bool,
    inventory: Vec<Option<(u16, u16)>>,
    selected: usize,
}

impl PlayerSave {
    pub fn inventory(&self, items: &ItemRegistry) -> Inventory {
        let mut inv = Inventory::default();
        for (slot, saved) in inv.slots.iter_mut().zip(&self.inventory) {
            // Items that no longer exist (a removed mod) are dropped.
            *slot = saved.as_ref().and_then(|s| {
                Some(ItemStack {
                    item: items.by_name(&s.item)?,
                    count: s.count,
                    wear: s.wear,
                })
            });
        }
        inv.select(self.selected);
        inv
    }

    pub fn store_inventory(&mut self, inv: &Inventory, items: &ItemRegistry) {
        self.inventory = inv
            .slots
            .iter()
            .map(|s| {
                s.map(|s| SavedStack {
                    item: items.name(s.item).to_owned(),
                    count: s.count,
                    wear: s.wear,
                })
            })
            .collect();
        self.inventory.resize(INVENTORY_SLOTS, None);
        self.selected = inv.selected;
    }
}

/// A saved furnace (or other container) and where it is.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SavedContainer {
    pub pos: [i32; 3],
    pub kind: crate::container::ContainerKind,
    pub slots: Vec<Option<SavedStack>>,
    pub burn_left: f32,
    pub burn_total: f32,
    pub progress: f32,
}

#[derive(Serialize, Deserialize)]
struct ContainersFile {
    version: u32,
    containers: Vec<SavedContainer>,
}

impl SavedContainer {
    pub fn new(pos: [i32; 3], c: &crate::container::Container, items: &ItemRegistry) -> Self {
        Self {
            pos,
            kind: c.kind,
            slots: c
                .slots
                .iter()
                .map(|s| {
                    s.map(|s| SavedStack {
                        item: items.name(s.item).to_owned(),
                        count: s.count,
                        wear: s.wear,
                    })
                })
                .collect(),
            burn_left: c.burn_left,
            burn_total: c.burn_total,
            progress: c.progress,
        }
    }

    pub fn restore(&self, items: &ItemRegistry) -> crate::container::Container {
        let mut c = crate::container::Container::new(self.kind);
        for (slot, saved) in c.slots.iter_mut().zip(&self.slots) {
            *slot = saved.as_ref().and_then(|s| {
                Some(ItemStack {
                    item: items.by_name(&s.item)?,
                    count: s.count,
                    wear: s.wear,
                })
            });
        }
        c.burn_left = self.burn_left;
        c.burn_total = self.burn_total;
        c.progress = self.progress;
        c
    }
}

/// Write `bytes` to `path` safely: write a temporary file, then rename it
/// over the old one, so a crash never leaves a half-written file.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

/// A world's folder on disk. Safe to share between threads: loading and
/// saving happen on background threads so they never stall a frame.
pub struct WorldStorage {
    dir: PathBuf,
    regions: Mutex<HashMap<RegionPos, RegionFile>>,
    /// Columns handed to [`WorldStorage::queue_save`] but not yet written.
    /// Loads check here first, so a column unloaded and quickly reloaded
    /// never comes back stale.
    pending: Mutex<HashMap<ColumnPos, Arc<Vec<u8>>>>,
    remap: Mutex<Option<BlockRemap>>,
}

impl WorldStorage {
    /// Open (or create) a world folder.
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(dir.join("region"))?;
        fs::create_dir_all(dir.join("players"))?;
        Ok(Self {
            dir,
            regions: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            remap: Mutex::new(None),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Set the id translation used when loading columns (from the saved
    /// world's block names). `None` or an identity map means no translation.
    pub fn set_remap(&self, remap: Option<BlockRemap>) {
        *self.remap.lock().unwrap() = remap.filter(|r| !r.is_identity());
    }

    fn with_region<T>(
        &self,
        col: ColumnPos,
        f: impl FnOnce(&mut RegionFile) -> io::Result<T>,
    ) -> io::Result<T> {
        let rpos = RegionPos::of(col);
        let mut regions = self.regions.lock().unwrap();
        let region = match regions.entry(rpos) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let path = self.dir.join("region").join(rpos.file_name());
                e.insert(RegionFile::open(&path)?)
            }
        };
        f(region)
    }

    /// Load a saved column, or `None` if it was never saved (generate it).
    pub fn load_column(&self, col: ColumnPos) -> io::Result<Option<Vec<Chunk>>> {
        let pending = self.pending.lock().unwrap().get(&col).cloned();
        let bytes = match pending {
            Some(b) => Some((*b).clone()),
            None => self.with_region(col, |r| r.read(col))?,
        };
        let remap = self.remap.lock().unwrap().clone();
        bytes.map(|b| decode_column(&b, remap.as_ref())).transpose()
    }

    /// Remember a column to save, encoding it now. Call
    /// [`WorldStorage::flush`] (typically on a background thread) to write it.
    pub fn queue_save(&self, col: ColumnPos, chunks: &[Chunk]) {
        let bytes = Arc::new(encode_column(chunks));
        self.pending.lock().unwrap().insert(col, bytes);
    }

    /// Write all queued columns to disk.
    pub fn flush(&self) -> io::Result<usize> {
        let queued: Vec<(ColumnPos, Arc<Vec<u8>>)> = self
            .pending
            .lock()
            .unwrap()
            .iter()
            .map(|(c, b)| (*c, b.clone()))
            .collect();
        for (col, bytes) in &queued {
            self.with_region(*col, |r| r.write(*col, bytes))?;
            // Only forget it if it wasn't re-queued with newer data meanwhile.
            let mut pending = self.pending.lock().unwrap();
            if pending.get(col).is_some_and(|b| Arc::ptr_eq(b, bytes)) {
                pending.remove(col);
            }
        }
        for region in self.regions.lock().unwrap().values_mut() {
            region.file.sync_data()?;
        }
        Ok(queued.len())
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.lock().unwrap().is_empty()
    }

    pub fn load_meta(&self) -> io::Result<Option<WorldMeta>> {
        let path = self.dir.join("world.dat");
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        if let Ok(meta) = postcard::from_bytes::<WorldMeta>(&bytes)
            && meta.version == META_VERSION
        {
            return Ok(Some(meta));
        }
        // Older worlds: upgrade on the fly.
        let old: WorldMetaV1 = postcard::from_bytes(&bytes).map_err(|e| invalid(e.to_string()))?;
        if old.version != 1 {
            return Err(invalid(format!("unknown world version {}", old.version)));
        }
        Ok(Some(WorldMeta {
            version: META_VERSION,
            name: old.name,
            seed: old.seed,
            time_of_day: old.time_of_day,
            block_names: old.block_names,
            creative: old.creative,
            game_mode: None,
        }))
    }

    pub fn save_meta(&self, meta: &WorldMeta) -> io::Result<()> {
        let bytes = postcard::to_allocvec(meta).map_err(|e| invalid(e.to_string()))?;
        write_atomic(&self.dir.join("world.dat"), &bytes)
    }

    fn player_path(&self, name: &str) -> PathBuf {
        // Keep only safe characters so a player name can't escape the folder.
        let safe: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(32)
            .collect();
        let safe = if safe.is_empty() {
            "player".into()
        } else {
            safe
        };
        self.dir.join("players").join(format!("{safe}.dat"))
    }

    /// Load a player. `block_names` are the world's saved block names
    /// (from [`WorldMeta`]), needed to upgrade older player files whose
    /// inventories held block ids.
    pub fn load_player(
        &self,
        name: &str,
        block_names: &[String],
    ) -> io::Result<Option<PlayerSave>> {
        let path = self.player_path(name);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        if let Ok(p) = postcard::from_bytes::<PlayerSave>(&bytes)
            && p.version == PLAYER_VERSION
        {
            return Ok(Some(p));
        }
        let old: PlayerSaveV2 = postcard::from_bytes(&bytes).map_err(|e| invalid(e.to_string()))?;
        if old.version != 2 {
            return Err(invalid(format!("unknown player version {}", old.version)));
        }
        Ok(Some(PlayerSave {
            version: PLAYER_VERSION,
            position: old.position,
            yaw: old.yaw,
            pitch: old.pitch,
            flying: old.flying,
            inventory: old
                .inventory
                .iter()
                .map(|s| {
                    let (id, count) = (*s)?;
                    Some(SavedStack {
                        item: block_names.get(id as usize)?.clone(),
                        count,
                        wear: 0,
                    })
                })
                .collect(),
            selected: old.selected,
            creative: true,
        }))
    }

    /// Names of all saved players.
    pub fn player_names(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.dir.join("players")) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                (p.extension()? == "dat").then(|| p.file_stem()?.to_str().map(String::from))?
            })
            .collect()
    }

    /// Furnaces and other blocks holding items (`containers.dat`).
    pub fn load_containers(&self) -> io::Result<Vec<SavedContainer>> {
        let path = self.dir.join("containers.dat");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file: ContainersFile =
            postcard::from_bytes(&fs::read(path)?).map_err(|e| invalid(e.to_string()))?;
        Ok(file.containers)
    }

    pub fn save_containers(&self, containers: Vec<SavedContainer>) -> io::Result<()> {
        let file = ContainersFile {
            version: 1,
            containers,
        };
        let bytes = postcard::to_allocvec(&file).map_err(|e| invalid(e.to_string()))?;
        write_atomic(&self.dir.join("containers.dat"), &bytes)
    }

    /// What signs say (`signs.dat`).
    pub fn load_signs(&self) -> io::Result<Vec<([i32; 3], Vec<String>)>> {
        self.load_list("signs.dat")
    }

    pub fn save_signs(&self, signs: Vec<([i32; 3], Vec<String>)>) -> io::Result<()> {
        self.save_list("signs.dat", signs)
    }

    /// Where players respawn: the bed they last slept in (`beds.dat`).
    pub fn load_beds(&self) -> io::Result<Vec<(String, [i32; 3])>> {
        self.load_list("beds.dat")
    }

    pub fn save_beds(&self, beds: Vec<(String, [i32; 3])>) -> io::Result<()> {
        self.save_list("beds.dat", beds)
    }

    fn load_list<T: serde::de::DeserializeOwned>(&self, file: &str) -> io::Result<Vec<T>> {
        let path = self.dir.join(file);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let (version, list): (u32, Vec<T>) =
            postcard::from_bytes(&fs::read(path)?).map_err(|e| invalid(e.to_string()))?;
        if version != 1 {
            return Err(invalid(format!("unknown {file} version {version}")));
        }
        Ok(list)
    }

    fn save_list<T: Serialize>(&self, file: &str, list: Vec<T>) -> io::Result<()> {
        let bytes = postcard::to_allocvec(&(1u32, list)).map_err(|e| invalid(e.to_string()))?;
        write_atomic(&self.dir.join(file), &bytes)
    }

    pub fn save_player(&self, name: &str, player: &PlayerSave) -> io::Result<()> {
        let bytes = postcard::to_allocvec(player).map_err(|e| invalid(e.to_string()))?;
        write_atomic(&self.player_path(name), &bytes)
    }
}

/// List the world folders inside a saves directory, by name.
pub fn list_worlds(saves_dir: &Path) -> Vec<(String, WorldMeta)> {
    let Ok(entries) = fs::read_dir(saves_dir) else {
        return Vec::new();
    };
    let mut worlds: Vec<(String, WorldMeta)> = entries
        .flatten()
        .filter_map(|e| {
            let bytes = fs::read(e.path().join("world.dat")).ok()?;
            let meta: WorldMeta = postcard::from_bytes(&bytes).ok()?;
            Some((e.file_name().to_string_lossy().into_owned(), meta))
        })
        .collect();
    worlds.sort_by(|a, b| a.0.cmp(&b.0));
    worlds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks;
    use crate::worldgen::WorldGenerator;

    /// A fresh, empty temporary directory for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ourcraft-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn column_encoding_round_trips() {
        let col = WorldGenerator::new(1).generate_column(ColumnPos::new(2, 3));
        let bytes = encode_column(&col);
        let back = decode_column(&bytes, None).unwrap();
        assert_eq!(back, col);
        // Generated terrain compresses very well.
        assert!(bytes.len() < 8 * 1024, "column is {} bytes", bytes.len());
    }

    #[test]
    fn corrupt_data_is_an_error_not_a_crash() {
        assert!(decode_column(b"garbage", None).is_err());
        let mut bytes =
            encode_column(&WorldGenerator::new(1).generate_column(ColumnPos::new(0, 0)));
        let n = bytes.len();
        bytes[n / 2] ^= 0xFF;
        bytes.truncate(n - 3);
        assert!(decode_column(&bytes, None).is_err());
    }

    #[test]
    fn region_file_stores_columns_and_survives_reopen() {
        let dir = temp_dir("region");
        let path = dir.join("r.0.0.ocr");
        let small = vec![1u8; 100];
        let big = vec![2u8; 10_000];
        {
            let mut r = RegionFile::open(&path).unwrap();
            assert_eq!(r.read(ColumnPos::new(0, 0)).unwrap(), None);
            r.write(ColumnPos::new(0, 0), &small).unwrap();
            r.write(ColumnPos::new(31, 31), &big).unwrap();
            // Growing a column moves it; shrinking reuses its space.
            r.write(ColumnPos::new(0, 0), &big).unwrap();
            r.write(ColumnPos::new(0, 0), &small).unwrap();
        }
        let mut r = RegionFile::open(&path).unwrap();
        assert_eq!(r.read(ColumnPos::new(0, 0)).unwrap(), Some(small));
        assert_eq!(r.read(ColumnPos::new(31, 31)).unwrap(), Some(big));
        assert_eq!(r.read(ColumnPos::new(5, 5)).unwrap(), None);
        // Negative columns map into the region's table too.
        assert_eq!(
            RegionPos::of(ColumnPos::new(-1, -32)),
            RegionPos { x: -1, z: -1 }
        );
        assert_eq!(RegionPos::local_index(ColumnPos::new(-1, -32)), 31);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_files_that_are_not_regions() {
        let dir = temp_dir("badregion");
        let path = dir.join("r.0.0.ocr");
        fs::write(&path, vec![7u8; 9000]).unwrap();
        assert!(RegionFile::open(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn storage_saves_loads_and_serves_pending_columns() {
        let dir = temp_dir("storage");
        let generator = WorldGenerator::new(9);
        let pos = ColumnPos::new(-40, 70);
        let mut col = generator.generate_column(pos);
        col[5].set(1, 2, 3, blocks::GLASS);
        {
            let storage = WorldStorage::open(&dir).unwrap();
            assert!(storage.load_column(pos).unwrap().is_none());
            storage.queue_save(pos, &col);
            // Readable before it's flushed…
            assert_eq!(storage.load_column(pos).unwrap().unwrap(), col);
            assert_eq!(storage.flush().unwrap(), 1);
            assert!(!storage.has_pending());
        }
        // …and after reopening.
        let storage = WorldStorage::open(&dir).unwrap();
        assert_eq!(storage.load_column(pos).unwrap().unwrap(), col);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn meta_and_players_round_trip() {
        let dir = temp_dir("meta");
        let reg = BlockRegistry::with_builtin_blocks();
        let storage = WorldStorage::open(&dir).unwrap();
        assert!(storage.load_meta().unwrap().is_none());
        let meta = WorldMeta::new("Test World", 1234, &reg);
        storage.save_meta(&meta).unwrap();
        assert_eq!(storage.load_meta().unwrap().unwrap(), meta);

        let items = ItemRegistry::build(&reg, &[]);
        let mut inv = Inventory::default();
        inv.add(&items, crate::item::ItemId::from_block(blocks::BRICKS), 10);
        inv.add(&items, items.by_name("core:stone_axe").unwrap(), 1);
        inv.slots[1].as_mut().unwrap().wear = 7;
        let mut player = PlayerSave {
            version: PLAYER_VERSION,
            position: [1.0, 70.0, -3.5],
            yaw: 0.5,
            pitch: -0.2,
            flying: true,
            inventory: vec![],
            selected: 0,
            creative: false,
        };
        player.store_inventory(&inv, &items);
        storage.save_player("Alex/../evil", &player).unwrap();
        assert_eq!(
            storage.load_player("Alex/../evil", &[]).unwrap().unwrap(),
            player
        );
        assert!(dir.join("players/Alexevil.dat").exists());
        assert_eq!(player.inventory(&items), inv);

        let worlds = list_worlds(dir.parent().unwrap());
        assert!(worlds.iter().any(|(_, m)| m.name == "Test World"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn signs_and_beds_round_trip() {
        let dir = temp_dir("signs");
        let storage = WorldStorage::open(&dir).unwrap();
        assert!(storage.load_signs().unwrap().is_empty());
        let signs = vec![([1, 64, -2], vec!["Hello".to_string(), "Sam!".to_string()])];
        storage.save_signs(signs.clone()).unwrap();
        assert_eq!(storage.load_signs().unwrap(), signs);
        let beds = vec![("Kim".to_string(), [5, 70, 5])];
        storage.save_beds(beds.clone()).unwrap();
        assert_eq!(storage.load_beds().unwrap(), beds);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn containers_round_trip() {
        use crate::container::{Container, ContainerKind};
        let dir = temp_dir("containers");
        let storage = WorldStorage::open(&dir).unwrap();
        assert!(storage.load_containers().unwrap().is_empty());
        let items = ItemRegistry::build(&BlockRegistry::with_builtin_blocks(), &[]);
        let mut f = Container::new(ContainerKind::Furnace);
        f.slots[0] = Some(ItemStack::new(
            crate::item::ItemId::from_block(blocks::SAND),
            5,
        ));
        f.burn_left = 3.0;
        storage
            .save_containers(vec![SavedContainer::new([1, 2, 3], &f, &items)])
            .unwrap();
        let back = storage.load_containers().unwrap();
        assert_eq!(back[0].pos, [1, 2, 3]);
        assert_eq!(back[0].restore(&items), f);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_2_players_get_item_names() {
        #[derive(Serialize)]
        struct V2 {
            version: u32,
            position: [f32; 3],
            yaw: f32,
            pitch: f32,
            flying: bool,
            inventory: Vec<Option<(u16, u16)>>,
            selected: usize,
        }
        let dir = temp_dir("player-v2");
        let storage = WorldStorage::open(&dir).unwrap();
        let old = V2 {
            version: 2,
            position: [0.0, 64.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            flying: false,
            inventory: vec![Some((1, 5)), None, Some((99, 1))],
            selected: 0,
        };
        let path = dir.join("players/Sam.dat");
        fs::write(&path, postcard::to_allocvec(&old).unwrap()).unwrap();
        let names = vec!["core:air".to_string(), "paint:red".to_string()];
        let p = storage.load_player("Sam", &names).unwrap().unwrap();
        assert_eq!(p.version, PLAYER_VERSION);
        assert_eq!(p.inventory[0].as_ref().unwrap().item, "paint:red");
        assert_eq!(p.inventory[0].as_ref().unwrap().count, 5);
        assert_eq!(p.inventory[2], None, "unknown ids are dropped");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn version_1_worlds_still_load() {
        #[derive(Serialize)]
        struct V1 {
            version: u32,
            name: String,
            seed: u64,
            time_of_day: f32,
            block_names: Vec<String>,
            creative: bool,
        }
        let dir = temp_dir("v1meta");
        let storage = WorldStorage::open(&dir).unwrap();
        let old = V1 {
            version: 1,
            name: "Old".into(),
            seed: 5,
            time_of_day: 0.4,
            block_names: vec!["core:air".into()],
            creative: false,
        };
        fs::write(dir.join("world.dat"), postcard::to_allocvec(&old).unwrap()).unwrap();
        let meta = storage.load_meta().unwrap().unwrap();
        assert_eq!(meta.name, "Old");
        assert_eq!(meta.seed, 5);
        assert!(!meta.creative);
        assert_eq!(meta.game_mode, None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn block_ids_are_remapped_by_name() {
        // Pretend the world was saved when stone and dirt had swapped ids,
        // plus a block from a mod that's no longer installed.
        let reg = BlockRegistry::with_builtin_blocks();
        let saved = vec![
            "core:air".to_string(),
            "core:dirt".to_string(),
            "core:stone".to_string(),
            "some_mod:gone".to_string(),
        ];
        let remap = BlockRemap::new(&saved, &reg);
        assert!(!remap.is_identity());
        let mut chunk = Chunk::empty();
        chunk.set(0, 0, 0, BlockId(1)); // saved dirt
        chunk.set(1, 0, 0, BlockId(2)); // saved stone
        chunk.set(2, 0, 0, BlockId(3)); // removed block
        let col: Vec<Chunk> = std::iter::once(chunk)
            .chain((1..WORLD_HEIGHT_CHUNKS).map(|_| Chunk::empty()))
            .collect();
        let back = decode_column(&encode_column(&col), Some(&remap)).unwrap();
        assert_eq!(back[0].get(0, 0, 0), blocks::DIRT);
        assert_eq!(back[0].get(1, 0, 0), blocks::STONE);
        assert_eq!(back[0].get(2, 0, 0), BlockId::AIR);
        let full: Vec<String> = reg.iter().map(|(_, d)| d.name.clone()).collect();
        assert!(BlockRemap::new(&full, &reg).is_identity());
    }
}

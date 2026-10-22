use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use bridge::{Backend, GameMode};
use launcher::local_worlds::world::{Difficulty, Generator, NewWorld, Prefs, World, WorldUpdate};
use serde::{Deserialize, Serialize};

use super::{
    create_private_dir,
    level_dat::LevelData,
    lock,
    provision::unix_now,
    world::{WorldError, failed, validate_name},
};

const META_FILE: &str = "world.json";
const PREFS_FILE: &str = "prefs.json";
/// The Bedrock world folder (`level.dat`, `db/`) inside a world's directory.
pub(crate) const LEVEL_DIR: &str = "db";
/// A world's menu thumbnail, beside its metadata, named as vanilla names its world icon.
const THUMBNAIL_FILE: &str = "world_icon.jpeg";
/// Scratch folders of imports in progress; ids never start with it, so listings skip them.
const STAGING_PREFIX: &str = ".import-";
/// Beside each scratch folder, a file its importer holds locked until the folder is gone.
const STAGING_LOCK_SUFFIX: &str = ".lock";
/// Where an archive unpacks inside its scratch folder, beside the world being assembled.
const UNPACKED_DIR: &str = "unpacked";

/// An import's scratch folder, removed when dropped. Its lock file stays locked until then, so
/// other clients sharing the worlds folder leave it alone.
pub(crate) struct Staging {
    dir: PathBuf,
    lock_path: PathBuf,
    lock: fs::File,
}

impl Staging {
    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.dir
    }

    /// Where the archive unpacks.
    pub(crate) fn unpacked(&self) -> PathBuf {
        self.dir.join(UNPACKED_DIR)
    }
}

impl Drop for Staging {
    /// Removes the folder while still locked, then the lock file; the lock ends with the handle.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
        let _ = fs::remove_file(&self.lock_path);
    }
}
/// Largest thumbnail read back; one is a few tens of kilobytes.
const MAX_THUMBNAIL_BYTES: u64 = 1024 * 1024;

/// One directory per world under a per-user root: `world.json` beside the server's data folders.
pub(crate) struct Store {
    root: PathBuf,
    state: Mutex<StoreState>,
}

#[derive(Default)]
struct StoreState {
    /// Backend of worlds created without one; Dragonfly until set.
    default_backend: Option<Backend>,
    /// Pinned Unix time for tests; `None` reads the clock.
    now: Option<i64>,
}

impl StoreState {
    fn now(&self) -> i64 {
        self.now.unwrap_or_else(unix_now)
    }

    fn default_backend(&self) -> Backend {
        self.default_backend.unwrap_or(Backend::Dragonfly)
    }
}

/// `world.json` as written; field order matches earlier releases.
#[derive(Serialize)]
struct Record<'a> {
    id: &'a str,
    name: &'a str,
    game_mode: GameMode,
    generator: Generator,
    difficulty: Difficulty,
    backend: Backend,
    seed: i64,
    options: &'a launcher::local_worlds::options::WorldOptions,
    pending_options: &'a launcher::local_worlds::options::OptionChanges,
    created_unix: i64,
    last_played_unix: i64,
}

/// `world.json` as read: worlds saved before backends or generators existed leave them empty.
#[derive(Deserialize)]
struct Saved {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    game_mode: GameMode,
    #[serde(default)]
    generator: String,
    #[serde(default)]
    difficulty: Difficulty,
    #[serde(default)]
    backend: String,
    #[serde(default)]
    seed: i64,
    #[serde(default)]
    options: Option<launcher::local_worlds::options::WorldOptions>,
    #[serde(default)]
    pending_options: launcher::local_worlds::options::OptionChanges,
    #[serde(default)]
    created_unix: i64,
    #[serde(default)]
    last_played_unix: i64,
}

#[derive(Default, Deserialize, Serialize)]
struct SavedPrefs {
    #[serde(default)]
    docker_prompt_dismissed: bool,
}

impl Store {
    /// Opens the store at `root`, creating it if needed.
    pub(crate) fn open(root: PathBuf) -> Result<Self, WorldError> {
        create_private_dir(&root)
            .map_err(|error| failed(format!("create worlds directory: {error}")))?;
        Ok(Self {
            root,
            state: Mutex::default(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, StoreState> {
        lock(&self.state)
    }

    /// Sets the backend of worlds created without one.
    pub(crate) fn set_default_backend(&self, backend: Backend) {
        self.lock().default_backend = Some(backend);
    }

    pub(crate) fn default_backend(&self) -> Backend {
        self.lock().default_backend()
    }

    #[cfg(test)]
    pub(crate) fn pin_clock(&self, unix: i64) {
        self.lock().now = Some(unix);
    }

    /// The data directory of a world id, or `NotFound` for a malformed id.
    pub(crate) fn dir(&self, id: &str) -> Result<PathBuf, WorldError> {
        if !valid_id(id) {
            return Err(WorldError::NotFound);
        }
        Ok(self.root.join(id))
    }

    /// World `id`'s saved thumbnail, if one was captured.
    pub(crate) fn thumbnail(&self, id: &str) -> Option<Vec<u8>> {
        let path = self.dir(id).ok()?.join(THUMBNAIL_FILE);
        let file = fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(file, MAX_THUMBNAIL_BYTES + 1),
            &mut bytes,
        )
        .ok()?;
        (bytes.len() as u64 <= MAX_THUMBNAIL_BYTES).then_some(bytes)
    }

    /// Replaces world `id`'s thumbnail with `image`, whole or not at all.
    pub(crate) fn save_thumbnail(&self, id: &str, image: &[u8]) -> Result<(), WorldError> {
        let _state = self.lock();
        let dir = self.dir(id)?;
        if fs::metadata(dir.join(META_FILE)).is_err() {
            return Err(WorldError::NotFound);
        }
        let partial = dir.join(format!("{THUMBNAIL_FILE}.partial"));
        fs::write(&partial, image)
            .and_then(|()| fs::rename(&partial, dir.join(THUMBNAIL_FILE)))
            .map_err(|error| failed(format!("save world thumbnail: {error}")))
    }

    /// Reads and validates the saved metadata record.
    fn saved(&self, id: &str) -> Result<Saved, WorldError> {
        let path = self.dir(id)?.join(META_FILE);
        let raw = match fs::read(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(WorldError::NotFound);
            }
            Err(error) => return Err(failed(format!("read world metadata: {error}"))),
        };
        let corrupt = || failed(format!("corrupt world metadata for {id}"));
        let saved: Saved = serde_json::from_slice(&raw).map_err(|_| corrupt())?;
        if saved.id != id {
            return Err(corrupt());
        }
        Ok(saved)
    }

    /// Combines metadata edits with the latest settings saved by BDS.
    fn read(&self, id: &str) -> Result<World, WorldError> {
        let saved = self.saved(id)?;
        let corrupt = || failed(format!("corrupt world metadata for {id}"));
        let backend = match saved.backend.as_str() {
            "" | "dragonfly" => Backend::Dragonfly,
            "bds" => Backend::Bds,
            _ => return Err(corrupt()),
        };
        let generator = match saved.generator.as_str() {
            // The original local server created flat worlds; BDS always generated natural ones.
            "" if backend == Backend::Bds => Generator::Normal,
            "" | "flat" => Generator::Flat,
            "normal" => Generator::Normal,
            _ => return Err(corrupt()),
        };
        let level = super::level_dat::read(&self.root.join(id).join(LEVEL_DIR).join("level.dat"));
        let options_unavailable = matches!(level, Ok(LevelData::Oversized));
        let current = match &level {
            Ok(LevelData::Read(bytes)) => super::level_dat::options_from(bytes),
            _ => None,
        };
        let mut options = match (saved.options, current) {
            (Some(mut options), Some(current)) => {
                options.merge_server_rules(&current);
                options
            }
            (Some(options), None) => options,
            (None, current) => current.unwrap_or_default(),
        };
        options.apply_changes(&saved.pending_options);
        Ok(World {
            id: saved.id,
            name: saved.name,
            game_mode: saved.game_mode,
            generator,
            difficulty: saved.difficulty,
            backend,
            seed: saved.seed,
            options,
            created_unix: saved.created_unix,
            last_played_unix: saved.last_played_unix,
            options_unavailable,
            size_bytes: 0,
        })
    }

    fn write(
        &self,
        world: &World,
        pending_options: &launcher::local_worlds::options::OptionChanges,
    ) -> Result<(), WorldError> {
        let dir = self.dir(&world.id)?;
        create_private_dir(&dir)
            .map_err(|error| failed(format!("create world directory: {error}")))?;
        write_record(&dir, world, pending_options)
    }

    /// Validates `world` and saves it with a fresh id.
    pub(crate) fn create(&self, world: &NewWorld) -> Result<World, WorldError> {
        let id = format!("{:016x}", u64::from_be_bytes(random_bytes()?));
        let state = self.lock();
        let world = new_record(&state, id, world)?;
        self.write(&world, &Default::default())?;
        Ok(world)
    }

    /// A scratch folder beside the worlds, so publishing from it is a rename. Its lock is taken
    /// before the folder exists, so no other client ever sees it unlocked.
    pub(crate) fn staging(&self) -> Result<Staging, WorldError> {
        let error = |error: io::Error| failed(format!("create import folder: {error}"));
        let name = format!(
            "{STAGING_PREFIX}{:016x}",
            u64::from_be_bytes(random_bytes()?)
        );
        let lock_path = self.root.join(format!("{name}{STAGING_LOCK_SUFFIX}"));
        let lock = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(error)?;
        let staging = Staging {
            dir: self.root.join(name),
            lock_path,
            lock,
        };
        staging
            .lock
            .try_lock()
            .map_err(|locked| error(io::Error::other(locked)))?;
        create_private_dir(&staging.unpacked()).map_err(error)?;
        Ok(staging)
    }

    /// Deletes abandoned imports, which may be whole worlds; blocking, so never on a frame.
    pub(crate) fn reclaim_abandoned_imports(&self) {
        reclaim_abandoned_imports(&self.root);
    }

    /// Publishes the Bedrock world folder `level`, unpacked inside `staging`, as a new world.
    /// Its metadata and thumbnail are written inside the staging folder, which then becomes the
    /// world in one rename, so no world folder ever lacks its metadata. A cancelled import never
    /// publishes; its staging folder is removed with the [`Staging`].
    pub(crate) fn publish(
        &self,
        staging: &Staging,
        level: &Path,
        world: &NewWorld,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<World, WorldError> {
        let prepare = |error: io::Error| failed(format!("prepare imported world: {error}"));
        fs::rename(level, staging.dir.join(LEVEL_DIR)).map_err(prepare)?;
        if let Err(error) = fs::remove_dir_all(staging.unpacked())
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(prepare(error));
        }
        let icon = staging.dir.join(LEVEL_DIR).join(THUMBNAIL_FILE);
        if fs::metadata(&icon).is_ok_and(|icon| icon.is_file() && icon.len() <= MAX_THUMBNAIL_BYTES)
        {
            let _ = fs::copy(icon, staging.dir.join(THUMBNAIL_FILE));
        }
        // Listings take the same lock, so once a detach cancels, no later listing can miss a
        // world this import still publishes.
        let state = self.lock();
        if cancelled() {
            return Err(WorldError::Cancelled);
        }
        let mut id = format!("{:016x}", u64::from_be_bytes(random_bytes()?));
        while fs::exists(self.root.join(&id)).map_err(prepare)? {
            id = format!("{:016x}", u64::from_be_bytes(random_bytes()?));
        }
        let mut world = new_record(&state, id, world)?;
        write_record(&staging.dir, &world, &Default::default())?;
        let dir = self.root.join(&world.id);
        fs::rename(&staging.dir, &dir)
            .map_err(|error| failed(format!("publish imported world: {error}")))?;
        drop(state);
        world.size_bytes = dir_size(&dir);
        Ok(world)
    }

    pub(crate) fn get(&self, id: &str) -> Result<World, WorldError> {
        let _state = self.lock();
        self.read(id)
    }

    /// Every readable world, most recently played first; unreadable directories are skipped.
    pub(crate) fn list(&self) -> Result<Vec<World>, WorldError> {
        let _state = self.lock();
        let entries =
            fs::read_dir(&self.root).map_err(|error| failed(format!("list worlds: {error}")))?;
        let mut worlds = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(id) = name.to_str() else { continue };
            if !valid_id(id) || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            if let Ok(mut world) = self.read(id) {
                world.size_bytes = dir_size(&entry.path());
                worlds.push(world);
            }
        }
        worlds.sort_by(|a, b| {
            b.last_played_unix
                .cmp(&a.last_played_unix)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(worlds)
    }

    /// Applies a settings change; game mode and difficulty take effect on the next open.
    pub(crate) fn update(&self, id: &str, update: &WorldUpdate) -> Result<World, WorldError> {
        let _state = self.lock();
        let mut world = self.read(id)?;
        if let Some(name) = &update.name {
            world.name = validate_name(name)?;
        }
        if let Some(game_mode) = update.game_mode {
            world.game_mode = game_mode;
        }
        if let Some(difficulty) = update.difficulty {
            world.difficulty = difficulty;
        }
        if let Some(options) = &update.options {
            world.options.apply_changes(options);
        }
        let mut pending = self.saved(id)?.pending_options;
        if let Some(changes) = &update.options {
            pending.extend(changes);
        }
        self.write(&world, &pending)?;
        Ok(world)
    }

    /// Records that a world was just played.
    pub(crate) fn touch(&self, id: &str) -> Result<(), WorldError> {
        let state = self.lock();
        let mut world = self.read(id)?;
        world.last_played_unix = state.now();
        self.write(&world, &Default::default())
    }

    /// Removes a world and all of its saved data.
    pub(crate) fn delete(&self, id: &str) -> Result<(), WorldError> {
        let _state = self.lock();
        let dir = self.dir(id)?;
        if fs::metadata(&dir).is_err() {
            return Err(WorldError::NotFound);
        }
        fs::remove_dir_all(dir).map_err(|error| failed(format!("delete world: {error}")))
    }

    /// The saved preferences; a missing or unreadable file yields defaults.
    pub(crate) fn prefs(&self) -> Prefs {
        let _state = self.lock();
        self.read_prefs()
    }

    fn read_prefs(&self) -> Prefs {
        let saved = fs::read(self.root.join(PREFS_FILE))
            .ok()
            .and_then(|raw| serde_json::from_slice::<SavedPrefs>(&raw).ok())
            .unwrap_or_default();
        Prefs {
            docker_prompt_dismissed: saved.docker_prompt_dismissed,
        }
    }

    /// Saves the Docker prompt dismissal and returns the resulting preferences.
    pub(crate) fn update_prefs(&self, docker_prompt_dismissed: bool) -> Result<Prefs, WorldError> {
        let _state = self.lock();
        let mut prefs = self.read_prefs();
        prefs.docker_prompt_dismissed = docker_prompt_dismissed;
        let raw = serde_json::to_vec(&SavedPrefs {
            docker_prompt_dismissed,
        })
        .map_err(|error| failed(error.to_string()))?;
        write_atomic(&self.root, PREFS_FILE, &raw)
            .map_err(|error| failed(format!("write preferences: {error}")))?;
        Ok(prefs)
    }
}

/// World ids are 16 lowercase hex digits, so no id can name a path outside the store.
fn valid_id(id: &str) -> bool {
    id.len() == 16
        && id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Writes `world.json` for `world` into `dir`.
fn write_record(
    dir: &Path,
    world: &World,
    pending_options: &launcher::local_worlds::options::OptionChanges,
) -> Result<(), WorldError> {
    let raw = serde_json::to_vec_pretty(&Record {
        id: &world.id,
        name: &world.name,
        game_mode: world.game_mode,
        generator: world.generator,
        difficulty: world.difficulty,
        backend: world.backend,
        seed: world.seed,
        options: &world.options,
        pending_options,
        created_unix: world.created_unix,
        last_played_unix: world.last_played_unix,
    })
    .map_err(|error| failed(error.to_string()))?;
    write_atomic(dir, META_FILE, &raw)
        .map_err(|error| failed(format!("write world metadata: {error}")))
}

/// A new world's record, stamped now; a missing seed or backend gets a random seed or the default.
fn new_record(state: &StoreState, id: String, world: &NewWorld) -> Result<World, WorldError> {
    let seed = match world.seed {
        Some(seed) => seed,
        None => i64::from_be_bytes(random_bytes()?),
    };
    let now = state.now();
    Ok(World {
        id,
        name: validate_name(&world.name)?,
        game_mode: world.game_mode,
        generator: world.generator,
        difficulty: world.difficulty,
        backend: world.backend.unwrap_or_else(|| state.default_backend()),
        seed,
        options: world.options.clone(),
        created_unix: now,
        last_played_unix: now,
        options_unavailable: false,
        size_bytes: 0,
    })
}

/// Removes import folders a killed client left behind: those whose lock file is gone or can be
/// locked. A folder whose lock another client holds is a live import and stays.
fn reclaim_abandoned_imports(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(STAGING_PREFIX)
            || name.ends_with(STAGING_LOCK_SUFFIX)
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        let lock_path = root.join(format!("{name}{STAGING_LOCK_SUFFIX}"));
        // Held until both are removed, so a racing cleanup skips this folder too.
        let lock = match fs::OpenOptions::new().write(true).open(&lock_path) {
            Ok(lock) if lock.try_lock().is_ok() => Some(lock),
            Ok(_) => continue,
            // Importers lock before creating their folder, so a folder without one is abandoned.
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => continue,
        };
        let _ = fs::remove_dir_all(entry.path());
        if lock.is_some() {
            let _ = fs::remove_file(&lock_path);
        }
    }
}

fn random_bytes() -> Result<[u8; 8], WorldError> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).map_err(|error| failed(error.to_string()))?;
    Ok(bytes)
}

/// Total bytes of the regular files under `dir`; unreadable entries count as empty.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |metadata| metadata.len()),
            _ => 0,
        })
        .sum()
}

/// Replaces `dir/name` through a temporary sibling, so readers never see a partial file.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let mut file = tempfile::Builder::new()
        .prefix(&format!("{name}."))
        .suffix(".tmp")
        .tempfile_in(dir)?;
    file.write_all(bytes)?;
    file.persist(dir.join(name)).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

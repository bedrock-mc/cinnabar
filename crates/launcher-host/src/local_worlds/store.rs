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
    create_private_dir, lock,
    provision::unix_now,
    world::{WorldError, failed, validate_name},
};

const META_FILE: &str = "world.json";
const PREFS_FILE: &str = "prefs.json";

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

    fn read(&self, id: &str) -> Result<World, WorldError> {
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
        Ok(World {
            id: saved.id,
            name: saved.name,
            game_mode: saved.game_mode,
            generator,
            difficulty: saved.difficulty,
            backend,
            seed: saved.seed,
            created_unix: saved.created_unix,
            last_played_unix: saved.last_played_unix,
            size_bytes: 0,
        })
    }

    fn write(&self, world: &World) -> Result<(), WorldError> {
        let dir = self.dir(&world.id)?;
        create_private_dir(&dir)
            .map_err(|error| failed(format!("create world directory: {error}")))?;
        let raw = serde_json::to_vec_pretty(&Record {
            id: &world.id,
            name: &world.name,
            game_mode: world.game_mode,
            generator: world.generator,
            difficulty: world.difficulty,
            backend: world.backend,
            seed: world.seed,
            created_unix: world.created_unix,
            last_played_unix: world.last_played_unix,
        })
        .map_err(|error| failed(error.to_string()))?;
        write_atomic(&dir, META_FILE, &raw)
            .map_err(|error| failed(format!("write world metadata: {error}")))
    }

    /// Validates `world` and saves it with a fresh id.
    pub(crate) fn create(&self, world: &NewWorld) -> Result<World, WorldError> {
        let name = validate_name(&world.name)?;
        let id = format!("{:016x}", u64::from_be_bytes(random_bytes()?));
        let seed = match world.seed {
            Some(seed) => seed,
            None => i64::from_be_bytes(random_bytes()?),
        };
        let state = self.lock();
        let now = state.now();
        let world = World {
            id,
            name,
            game_mode: world.game_mode,
            generator: world.generator,
            difficulty: world.difficulty,
            backend: world.backend.unwrap_or_else(|| state.default_backend()),
            seed,
            created_unix: now,
            last_played_unix: now,
            size_bytes: 0,
        };
        self.write(&world)?;
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
        self.write(&world)?;
        Ok(world)
    }

    /// Records that a world was just played.
    pub(crate) fn touch(&self, id: &str) -> Result<(), WorldError> {
        let state = self.lock();
        let mut world = self.read(id)?;
        world.last_played_unix = state.now();
        self.write(&world)
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

//! `.mcworld` import: a zipped Bedrock world folder becomes a new saved world.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use bridge::{Backend, GameMode};
use launcher::local_worlds::world::{Difficulty, Generator, NewWorld, World};

use super::{
    store::Store,
    unpack::{UnpackLimits, extract_archive},
    world::{MAX_NAME_CHARS, WorldError, failed, validate_name},
};

/// The archive extension the Worlds tab imports.
pub const WORLD_ARCHIVE_EXTENSION: &str = "mcworld";

/// Whether `path` names a world archive, by its extension in any case.
#[must_use]
pub fn is_world_archive(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(WORLD_ARCHIVE_EXTENSION))
}

/// Bounds an imported world before anything is written; long-played worlds are a few GiB.
pub(super) const WORLD_LIMITS: UnpackLimits = UnpackLimits {
    label: "world file",
    refuse_links: true,
    max_entries: 100_000,
    max_file_bytes: 8 << 30,
    max_total_bytes: 8 << 30,
    min_ratio_sample: 4096,
    max_entry_ratio: 500.0,
    max_archive_ratio: 100.0,
};

const LEVEL_DAT: &str = "level.dat";
const LEVEL_NAME_FILE: &str = "levelname.txt";
/// Finder's resource-fork folder beside the world in archives zipped on macOS.
const MAC_METADATA_DIR: &str = "__MACOSX";
const FALLBACK_NAME: &str = "Imported world";

/// Unpacks `archive` into a new world on `backend`. A refused or cancelled import leaves nothing
/// behind; `cancelled` is checked between entries and before publishing.
pub(super) fn import(
    store: &Store,
    archive: &Path,
    backend: Backend,
    limits: &UnpackLimits,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<World, WorldError> {
    let staging = store.staging()?;
    let unpacked = staging.unpacked();
    extract_archive(&unpacked, archive, limits, cancelled)?;
    let level = world_root(&unpacked)?;
    let settings = read_level_dat(&level.join(LEVEL_DAT));
    let name = [
        read_level_name(&level),
        settings.name.clone(),
        archive
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned()),
    ]
    .into_iter()
    .flatten()
    .find_map(|name| clean_name(&name))
    .unwrap_or_else(|| FALLBACK_NAME.to_owned());
    store.publish(
        &staging,
        &level,
        &NewWorld {
            options: super::level_dat::read_options(&level.join(LEVEL_DAT)),
            name,
            game_mode: settings.game_mode,
            generator: settings.generator,
            difficulty: settings.difficulty,
            backend: Some(backend),
            seed: settings.seed,
        },
        cancelled,
    )
}

/// The extracted folder holding `level.dat`: the archive root or its one top-level folder.
fn world_root(root: &Path) -> Result<PathBuf, WorldError> {
    if root.join(LEVEL_DAT).is_file() {
        return Ok(root.to_path_buf());
    }
    let entries = fs::read_dir(root).map_err(|error| failed(error.to_string()))?;
    let mut folders = entries.flatten().filter(|entry| {
        entry.file_type().is_ok_and(|kind| kind.is_dir()) && entry.file_name() != MAC_METADATA_DIR
    });
    match (folders.next(), folders.next()) {
        (Some(folder), None) if folder.path().join(LEVEL_DAT).is_file() => Ok(folder.path()),
        _ => Err(WorldError::Rejected(format!(
            "the world file has no {LEVEL_DAT}"
        ))),
    }
}

/// The first line of `levelname.txt`, if the world has one.
fn read_level_name(level: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(level.join(LEVEL_NAME_FILE))
        .ok()?
        .take(4096)
        .read_to_end(&mut bytes)
        .ok()?;
    String::from_utf8_lossy(&bytes)
        .lines()
        .next()
        .map(str::to_owned)
}

/// A saveable world name from `raw`: trimmed, without control characters, at most the limit.
fn clean_name(raw: &str) -> Option<String> {
    let name: String = raw
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    validate_name(&name).ok()
}

/// The settings an imported world keeps; tags missing from `level.dat` take the defaults.
#[derive(Debug, Default, PartialEq)]
struct LevelSettings {
    name: Option<String>,
    game_mode: GameMode,
    difficulty: Difficulty,
    generator: Generator,
    seed: Option<i64>,
}

/// Reads `level.dat`: an 8-byte header, then a little-endian NBT compound. An unreadable file
/// yields what was read before the damage.
fn read_level_dat(path: &Path) -> LevelSettings {
    let mut settings = LevelSettings {
        generator: Generator::Normal,
        ..LevelSettings::default()
    };
    let mut bytes = Vec::new();
    let read = fs::File::open(path).and_then(|file| {
        file.take(super::level_dat::MAX_LEVEL_DAT_BYTES)
            .read_to_end(&mut bytes)
    });
    if read.is_ok() {
        let _ = Nbt(bytes.get(8..).unwrap_or_default()).root(&mut settings);
    }
    settings
}

const TAG_END: u8 = 0;
const TAG_INT: u8 = 3;
const TAG_LONG: u8 = 4;
const TAG_STRING: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;
/// Deeper nesting than any real `level.dat` is treated as damage.
const MAX_NBT_DEPTH: u32 = 64;

/// A cursor over little-endian NBT; every read is `None` past the end.
struct Nbt<'a>(&'a [u8]);

impl<'a> Nbt<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(len)?;
        self.0 = rest;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.array::<1>()?[0])
    }

    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.array()?))
    }

    fn len(&mut self) -> Option<usize> {
        usize::try_from(self.i32()?).ok()
    }

    fn string(&mut self) -> Option<String> {
        let len = u16::from_le_bytes(self.array()?);
        Some(String::from_utf8_lossy(self.take(usize::from(len))?).into_owned())
    }

    /// Reads the root compound's settings tags and skips the rest.
    fn root(&mut self, settings: &mut LevelSettings) -> Option<()> {
        if self.u8()? != TAG_COMPOUND {
            return None;
        }
        self.string()?;
        loop {
            let tag = self.u8()?;
            if tag == TAG_END {
                return Some(());
            }
            match (tag, self.string()?.as_str()) {
                (TAG_INT, "GameType") => {
                    settings.game_mode = match self.i32()? {
                        1 => GameMode::Creative,
                        2 => GameMode::Adventure,
                        _ => GameMode::Survival,
                    };
                }
                (TAG_INT, "Difficulty") => {
                    settings.difficulty = match self.i32()? {
                        0 => Difficulty::Peaceful,
                        1 => Difficulty::Easy,
                        3 => Difficulty::Hard,
                        _ => Difficulty::Normal,
                    };
                }
                (TAG_INT, "Generator") => {
                    settings.generator = if self.i32()? == 2 {
                        Generator::Flat
                    } else {
                        Generator::Normal
                    };
                }
                (TAG_LONG, "RandomSeed") => {
                    settings.seed = Some(i64::from_le_bytes(self.array()?));
                }
                (TAG_STRING, "LevelName") => settings.name = Some(self.string()?),
                (tag, _) => self.skip(tag, 0)?,
            }
        }
    }

    /// Skips one payload of `tag`.
    fn skip(&mut self, tag: u8, depth: u32) -> Option<()> {
        if depth > MAX_NBT_DEPTH {
            return None;
        }
        let len = match tag {
            1 => 1,
            2 => 2,
            TAG_INT | 5 => 4,
            TAG_LONG | 6 => 8,
            7 => self.len()?,
            TAG_STRING => usize::from(u16::from_le_bytes(self.array()?)),
            TAG_LIST => {
                let item = self.u8()?;
                for _ in 0..self.len()? {
                    self.skip(item, depth + 1)?;
                }
                return Some(());
            }
            TAG_COMPOUND => loop {
                match self.u8()? {
                    TAG_END => return Some(()),
                    tag => {
                        self.string()?;
                        self.skip(tag, depth + 1)?;
                    }
                }
            },
            11 => self.len()?.checked_mul(4)?,
            12 => self.len()?.checked_mul(8)?,
            _ => return None,
        };
        self.take(len).map(drop)
    }
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;

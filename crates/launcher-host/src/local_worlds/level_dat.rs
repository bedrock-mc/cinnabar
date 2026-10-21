//! Applies saved world options before BDS opens its little-endian level data.

use super::{
    world::{WorldError, failed},
    write_private,
};
use launcher::local_worlds::{
    options::{Control, Storage, WorldOption as O},
    world::{Generator, World},
};
use std::{fs, io, path::Path};

/// Prepares new level data or replaces only owned fields in an existing world.
pub(super) fn configure(world: &World, dir: &Path) -> Result<(), WorldError> {
    let path = dir.join("level.dat");
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(failed(format!("read world settings: {error}"))),
    };
    let data = configured(world, &bytes).ok_or_else(|| failed("invalid world level data"))?;
    let temporary = dir.join("level.dat.cinnabar");
    write_private(&temporary, &data)
        .and_then(|()| fs::rename(&temporary, &path))
        .map_err(|error| failed(format!("write world settings: {error}")))
}

/// Produces a complete level-data file while preserving unowned tags and the file version.
fn configured(world: &World, bytes: &[u8]) -> Option<Vec<u8>> {
    let new = bytes.is_empty();
    let (version, mut fields) = if new {
        (10u32.to_le_bytes(), Vec::new())
    } else {
        let version = bytes.get(..4)?.try_into().ok()?;
        let size = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
        let body = bytes.get(8..)?;
        if body.len() != size {
            return None;
        }
        (version, read_fields(body)?)
    };
    if new {
        put(&mut fields, "StorageVersion", 3, &10i32.to_le_bytes());
        put(&mut fields, "RandomSeed", 4, &world.seed.to_le_bytes());
        put(
            &mut fields,
            "Generator",
            3,
            &(if world.generator == Generator::Flat {
                2i32
            } else {
                1
            })
            .to_le_bytes(),
        );
        put(
            &mut fields,
            "GameType",
            3,
            &(world.game_mode as i32).to_le_bytes(),
        );
        put(
            &mut fields,
            "Difficulty",
            3,
            &(world.difficulty as i32).to_le_bytes(),
        );
    }
    put(&mut fields, "LevelName", 8, &string(&world.name));
    let options = &world.options;
    for &id in O::ALL {
        let definition = id.definition();
        if definition.creation_only && !new {
            continue;
        }
        let key = match definition.storage {
            Storage::Rule(key) | Storage::Level(key) | Storage::Property(_, key) => key,
            Storage::Combined => continue,
        };
        let value = if id == O::SleepPercentage && !options.enabled(O::SleepSkipNight) {
            101
        } else if id == O::LanVisible {
            i32::from(options.lan_visible())
        } else {
            options.get(id)
        };
        match definition.control {
            Control::Toggle => put(&mut fields, key, 1, &[value as u8]),
            Control::Number { .. } => put(&mut fields, key, 3, &value.to_le_bytes()),
        }
    }
    // Once used, cheats remain recorded even after the switch is turned off.
    if options.enabled(O::ActivateCheats) {
        put(&mut fields, "hasBeenLoadedInCreative", 1, &[1]);
    }
    put(
        &mut fields,
        "MultiplayerGameIntent",
        1,
        &[u8::from(options.enabled(O::Multiplayer))],
    );
    put(
        &mut fields,
        "LANBroadcastIntent",
        1,
        &[u8::from(options.enabled(O::LanVisible))],
    );
    put(
        &mut fields,
        "daylightCycle",
        3,
        &(if options.enabled(O::AlwaysDay) {
            1i32
        } else if options.enabled(O::DaylightCycle) {
            0
        } else {
            2
        })
        .to_le_bytes(),
    );
    if options.enabled(O::AlwaysDay) {
        put(&mut fields, "Time", 4, &6000i64.to_le_bytes());
    }
    let mut body = vec![10, 0, 0];
    for (_, tag) in fields {
        body.extend(tag);
    }
    body.push(0);
    let mut output = version.to_vec();
    output.extend(u32::try_from(body.len()).ok()?.to_le_bytes());
    output.extend(body);
    Some(output)
}

type Fields = Vec<(Vec<u8>, Vec<u8>)>;

/// Replaces an owned tag without keeping duplicate values of the same name.
fn put(fields: &mut Fields, key: &str, kind: u8, payload: &[u8]) {
    fields.retain(|(name, _)| name != key.as_bytes());
    let mut tag = vec![kind];
    tag.extend(string(key));
    tag.extend(payload);
    fields.push((key.as_bytes().to_vec(), tag));
}

/// Encodes a length-prefixed NBT string.
fn string(value: &str) -> Vec<u8> {
    let mut bytes = (value.len() as u16).to_le_bytes().to_vec();
    bytes.extend(value.as_bytes());
    bytes
}

/// Reads raw root fields, retaining every byte of unknown payloads.
fn read_fields(bytes: &[u8]) -> Option<Fields> {
    let mut reader = Reader(bytes);
    if reader.take(1)? != [10] {
        return None;
    }
    reader.name()?;
    let mut fields = Vec::new();
    loop {
        let start = reader.0;
        let kind = reader.take(1)?[0];
        if kind == 0 {
            return reader.0.is_empty().then_some(fields);
        }
        let name = reader.name()?.to_vec();
        reader.skip(kind)?;
        fields.push((name, start[..start.len() - reader.0.len()].to_vec()));
    }
}

/// A checked cursor that walks nested payloads without a recursion limit.
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    /// Takes a checked prefix and advances the cursor.
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let (head, tail) = self.0.split_at_checked(len)?;
        self.0 = tail;
        Some(head)
    }
    /// Reads an NBT name or string without changing its encoding.
    fn name(&mut self) -> Option<&'a [u8]> {
        let len = u16::from_le_bytes(self.take(2)?.try_into().ok()?);
        self.take(usize::from(len))
    }
    /// Reads a nonnegative collection length.
    fn len(&mut self) -> Option<usize> {
        usize::try_from(i32::from_le_bytes(self.take(4)?.try_into().ok()?)).ok()
    }
    /// Skips one payload, using a stack for nested lists and compounds.
    fn skip(&mut self, kind: u8) -> Option<()> {
        let mut pending = vec![(kind, 1usize)];
        while let Some((kind, count)) = pending.pop() {
            if count == 0 {
                continue;
            }
            if count > 1 {
                pending.push((kind, count - 1));
            }
            let size = match kind {
                1 => 1,
                2 => 2,
                3 | 5 => 4,
                4 | 6 => 8,
                7 => self.len()?,
                8 => {
                    self.name()?;
                    continue;
                }
                9 => {
                    let item = self.take(1)?[0];
                    let count = self.len()?;
                    if count > 0 && !(1..=12).contains(&item) {
                        return None;
                    }
                    pending.push((item, count));
                    continue;
                }
                10 => {
                    let next = self.take(1)?[0];
                    if next != 0 {
                        self.name()?;
                        pending.push((10, 1));
                        pending.push((next, 1));
                    }
                    continue;
                }
                11 => self.len()?.checked_mul(4)?,
                12 => self.len()?.checked_mul(8)?,
                _ => return None,
            };
            self.take(size)?;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests;

/// Recovers options from worlds saved before the option catalog was introduced.
pub(super) fn read_options(path: &Path) -> launcher::local_worlds::options::WorldOptions {
    let mut options = launcher::local_worlds::options::WorldOptions::default();
    let Some(fields) = fs::read(path)
        .ok()
        .and_then(|bytes| read_fields(bytes.get(8..)?))
    else {
        return options;
    };
    for &id in O::ALL {
        let key = match id.definition().storage {
            Storage::Rule(key) | Storage::Level(key) | Storage::Property(_, key) => key,
            Storage::Combined => continue,
        };
        if let Some((_, tag)) = fields.iter().find(|(name, _)| name == key.as_bytes()) {
            let data = &tag[3 + key.len()..];
            let value = match tag[0] {
                1 => data.first().map(|value| i32::from(*value)),
                3 => data.try_into().ok().map(i32::from_le_bytes),
                _ => None,
            };
            if let Some(value) = value {
                if id == O::SleepPercentage && value > 100 {
                    options.set(O::SleepSkipNight, 0);
                } else {
                    options.set(id, value);
                }
            }
        }
    }
    // Reading existing rules must preserve the saved cheats switch, not simulate UI activation.
    if fields.iter().any(|(name, tag)| {
        name == b"daylightCycle"
            && tag
                .last_chunk::<4>()
                .is_some_and(|bytes| i32::from_le_bytes(*bytes) == 1)
    }) {
        options.set(O::AlwaysDay, 1);
    }
    let enabled = fields
        .iter()
        .find(|(name, _)| name == b"cheatsEnabled")
        .and_then(|(_, tag)| tag.last())
        .copied()
        .unwrap_or(0)
        != 0;
    options.restore_cheats(enabled);
    options
}

//! Small whole-file tables: a checked envelope, a bounded body reader and atomic
//! replacement.
//!
//! An envelope is a 4-byte magic, a little-endian `u16` format version, the body, then a
//! CRC32 of everything before it. Files are replaced by writing a temporary sibling and
//! renaming it over the old file, so a reader sees the old or the new table, or after a
//! power loss, unless the replacement was synced, possibly a torn one that fails its
//! checksum.

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use crate::{frame::crc32, region_file::TEMP_EXTENSION};

const PREAMBLE_LEN: usize = 6;
const CRC_LEN: usize = 4;

/// Why a table file was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TableError {
    #[error("table has {0} bytes, less than its envelope")]
    Truncated(usize),
    #[error("table file has {len} bytes; at most {max} are allowed")]
    TooLarge { len: u64, max: usize },
    #[error("table starts with {found:02x?}, not {expected:02x?}")]
    Magic { found: [u8; 4], expected: [u8; 4] },
    #[error("table format version {0} is not supported")]
    Version(u16),
    #[error("table checksum {stored:#010x} does not match {computed:#010x}")]
    Checksum { stored: u32, computed: u32 },
    #[error("table body is malformed: {0}")]
    Body(&'static str),
}

/// Whether a replacement is flushed to disk before it takes the old file's place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Durability {
    /// Rebuildable data: a torn file after power loss is rejected and rebuilt.
    Cached,
    /// Data other files depend on, such as the material registry.
    Synced,
}

/// Wraps `body` in the envelope.
pub(crate) fn seal(magic: [u8; 4], version: u16, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(PREAMBLE_LEN + body.len() + CRC_LEN);
    out.extend_from_slice(&magic);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(body);
    let crc = crc32(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

/// The body of a sealed table, after checking its magic, version and checksum.
pub(crate) fn unseal(magic: [u8; 4], version: u16, bytes: &[u8]) -> Result<&[u8], TableError> {
    if bytes.len() < PREAMBLE_LEN + CRC_LEN {
        return Err(TableError::Truncated(bytes.len()));
    }
    let found: [u8; 4] = bytes[..4].try_into().expect("four bytes");
    if found != magic {
        return Err(TableError::Magic {
            found,
            expected: magic,
        });
    }
    let (checked, stored) = bytes.split_at(bytes.len() - CRC_LEN);
    let stored = u32::from_le_bytes(stored.try_into().expect("four bytes"));
    let computed = crc32(checked);
    if stored != computed {
        return Err(TableError::Checksum { stored, computed });
    }
    let found_version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if found_version != version {
        return Err(TableError::Version(found_version));
    }
    Ok(&checked[PREAMBLE_LEN..])
}

/// Reads the file at `path`, or `None` when it does not exist. Files over `max` bytes
/// are rejected before anything is allocated.
pub(crate) fn read_bounded(
    path: &Path,
    max: usize,
) -> io::Result<Option<Result<Vec<u8>, TableError>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let len = file.metadata()?.len();
    if len > max as u64 {
        return Ok(Some(Err(TableError::TooLarge { len, max })));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Ok(Some(Err(TableError::TooLarge {
            len: bytes.len() as u64,
            max,
        })));
    }
    Ok(Some(Ok(bytes)))
}

/// Replaces the file at `path` with `bytes` through a temporary sibling.
pub(crate) fn replace(path: &Path, bytes: &[u8], durability: Durability) -> io::Result<()> {
    let temp = temp_path(path);
    let written = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        if durability == Durability::Synced {
            file.sync_all()?;
        }
        drop(file);
        fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// Removes the file at `path`; a missing file is not an error.
pub(crate) fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// The temporary sibling [`replace`] writes first.
pub(crate) fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".");
    name.push(TEMP_EXTENSION);
    path.with_file_name(name)
}

/// Bounds-checked little-endian reads over a table body.
pub(crate) struct BodyReader<'a> {
    bytes: &'a [u8],
}

impl<'a> BodyReader<'a> {
    /// A reader over `bytes`.
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// The next `len` bytes; fails when fewer remain.
    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], TableError> {
        if len > self.bytes.len() {
            return Err(TableError::Body("body ends inside a field"));
        }
        let (taken, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        Ok(taken)
    }

    /// The next `N` bytes as an array.
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], TableError> {
        Ok(self.take(N)?.try_into().expect("took exactly N bytes"))
    }

    /// The next byte; fails at the end of the body.
    pub(crate) fn u8(&mut self) -> Result<u8, TableError> {
        Ok(self.array::<1>()?[0])
    }

    /// The next little-endian `u16`; fails when fewer bytes remain.
    pub(crate) fn u16(&mut self) -> Result<u16, TableError> {
        self.array().map(u16::from_le_bytes)
    }

    /// The next little-endian `u32`; fails when fewer bytes remain.
    pub(crate) fn u32(&mut self) -> Result<u32, TableError> {
        self.array().map(u32::from_le_bytes)
    }

    /// The next little-endian `i32`; fails when fewer bytes remain.
    pub(crate) fn i32(&mut self) -> Result<i32, TableError> {
        self.array().map(i32::from_le_bytes)
    }

    /// A count of entries of at least `entry_len` bytes each, rejected when the remaining
    /// bytes cannot hold that many, so a corrupt count cannot force a large allocation.
    pub(crate) fn count(&mut self, entry_len: usize) -> Result<usize, TableError> {
        let count = self.u32()? as usize;
        if count.saturating_mul(entry_len) > self.bytes.len() {
            return Err(TableError::Body("entry count exceeds the body"));
        }
        Ok(count)
    }

    /// Fails unless every byte was read.
    pub(crate) fn finish(self) -> Result<(), TableError> {
        if self.bytes.is_empty() {
            Ok(())
        } else {
            Err(TableError::Body("bytes follow the last entry"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAGIC: [u8; 4] = *b"TEST";

    #[test]
    fn sealed_tables_round_trip_and_reject_damage() {
        let sealed = seal(MAGIC, 3, b"body");
        assert_eq!(unseal(MAGIC, 3, &sealed), Ok(&b"body"[..]));
        assert_eq!(
            unseal(MAGIC, 4, &sealed),
            Err(TableError::Version(3)),
            "a version change is not a checksum error"
        );
        assert!(matches!(
            unseal(*b"NOPE", 3, &sealed),
            Err(TableError::Magic { .. })
        ));
        let mut flipped = sealed.clone();
        flipped[7] ^= 1;
        assert!(matches!(
            unseal(MAGIC, 3, &flipped),
            Err(TableError::Checksum { .. })
        ));
        assert_eq!(
            unseal(MAGIC, 3, &sealed[..5]),
            Err(TableError::Truncated(5))
        );
    }

    #[test]
    fn replacement_is_whole_and_reads_are_bounded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("table");
        assert_eq!(read_bounded(&path, 16).expect("read"), None);
        replace(&path, b"first", Durability::Cached).expect("write");
        replace(&path, b"second", Durability::Synced).expect("rewrite");
        assert_eq!(
            read_bounded(&path, 16).expect("read"),
            Some(Ok(b"second".to_vec()))
        );
        assert!(!temp_path(&path).exists());
        assert_eq!(
            read_bounded(&path, 3).expect("read"),
            Some(Err(TableError::TooLarge { len: 6, max: 3 }))
        );
    }

    #[test]
    fn body_counts_cannot_exceed_the_body() {
        let mut body = u32::MAX.to_le_bytes().to_vec();
        body.extend_from_slice(&[0; 8]);
        assert!(BodyReader::new(&body).count(4).is_err());
        let mut body = 2u32.to_le_bytes().to_vec();
        body.extend_from_slice(&[0; 8]);
        let mut reader = BodyReader::new(&body);
        assert_eq!(reader.count(4), Ok(2));
        assert!(reader.take(9).is_err());
    }
}

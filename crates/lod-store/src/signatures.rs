//! Persisted surface signatures of one epoch, for world-epoch voting.
//!
//! Signatures of the [`lod::epoch`] regions inside one level-0 storage region share one
//! sealed file named after that region. Files are rewritten whole through a temporary
//! sibling; a damaged file reads as empty, so its regions abstain until re-recorded.

use std::{
    collections::{BTreeMap, HashMap},
    io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use foldhash::fast::FixedState;
use lod::epoch::{self, ChunkSurface, MAX_ENCODED_SIGNATURE_BYTES, RegionPos, RegionSignature};

use crate::{
    region_file::Access,
    region_id::{self, RegionId},
    table::{self, BodyReader, Durability, TableError, seal, unseal},
};

/// Leading bytes of a signature file.
pub const SIGNATURE_MAGIC: [u8; 4] = *b"CLSG";
/// Version of the signature file layout.
pub const SIGNATURE_FORMAT_VERSION: u16 = 1;
/// Extension of signature file names.
pub const SIGNATURE_FILE_EXTENSION: &str = "lods";
/// Signature regions along each side of one storage region.
pub const SIGNATURE_SIDE: i32 = region_id::REGION_SIDE_CHUNKS / epoch::REGION_SIDE_CHUNKS;
/// Signature regions one file holds.
pub const SIGNATURES_PER_FILE: usize = (SIGNATURE_SIDE * SIGNATURE_SIDE) as usize;
const ENTRY_HEADER_LEN: usize = 6;
/// Largest signature file read: every region fully recorded, plus framing.
pub const MAX_SIGNATURE_FILE_BYTES: usize =
    64 + SIGNATURES_PER_FILE * (ENTRY_HEADER_LEN + MAX_ENCODED_SIGNATURE_BYTES);
/// Default bound on signature files held in memory per epoch.
pub const DEFAULT_OPEN_SIGNATURE_FILES: NonZeroUsize = NonZeroUsize::new(8).expect("non-zero");

const FILE_PREFIX: &str = "s";

const _: () = assert!(region_id::REGION_SIDE_CHUNKS % epoch::REGION_SIDE_CHUNKS == 0);
const _: () = assert!(SIGNATURES_PER_FILE <= u16::MAX as usize + 1);

/// The level-0 storage region whose file holds the signature of `pos`.
#[must_use]
pub fn file_region(pos: RegionPos) -> RegionId {
    RegionId::of_chunk(
        pos.x * epoch::REGION_SIDE_CHUNKS,
        pos.z * epoch::REGION_SIDE_CHUNKS,
    )
    .0
}

/// The file name of `region`'s signatures, which [`signature_region`] reads back.
#[must_use]
pub fn signature_file_name(region: RegionId) -> String {
    format!(
        "{FILE_PREFIX}.{}.{}.{SIGNATURE_FILE_EXTENSION}",
        region.x(),
        region.z()
    )
}

/// Parses a name written by [`signature_file_name`]; `None` for any other name.
#[must_use]
pub fn signature_region(name: &str) -> Option<RegionId> {
    let stem = name
        .strip_prefix(FILE_PREFIX)?
        .strip_prefix('.')?
        .strip_suffix(SIGNATURE_FILE_EXTENSION)?
        .strip_suffix('.')?;
    let (x, z) = stem.split_once('.')?;
    let region = RegionId::new(0, x.parse().ok()?, z.parse().ok()?)?;
    (signature_file_name(region) == name).then_some(region)
}

/// Something a cache did on its own that its owner should count and log.
#[derive(Debug)]
pub enum SignatureEvent {
    /// A file was malformed and is treated as empty.
    Unreadable { path: PathBuf, error: TableError },
    /// Reading a file failed. It reads as empty, and what is recorded over it is merged
    /// into the file once a read succeeds, never written over it before.
    ReadFailed { path: PathBuf, error: io::Error },
    /// Writing a file failed; its new signatures may be lost.
    WriteFailed { path: PathBuf, error: io::Error },
}

/// One loaded signature file.
#[derive(Debug, Default)]
struct SignatureFile {
    signatures: BTreeMap<RegionPos, RegionSignature>,
    dirty: bool,
    last_use: u64,
    /// Reading the file on disk failed, so it may hold signatures not loaded here.
    unread: bool,
}

/// The signature files of one epoch directory behind a bounded in-memory cache. A
/// read-only cache records in memory only and never writes.
#[derive(Debug)]
pub struct SignatureCache {
    dir: PathBuf,
    access: Access,
    capacity: NonZeroUsize,
    files: HashMap<RegionId, SignatureFile, FixedState>,
    tick: u64,
    events: Vec<SignatureEvent>,
    detached: bool,
}

impl SignatureCache {
    /// A cache over the epoch directory `dir`, holding at most `capacity` files.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>, access: Access, capacity: NonZeroUsize) -> Self {
        Self {
            dir: dir.into(),
            access,
            capacity,
            files: HashMap::default(),
            tick: 0,
            events: Vec::new(),
            detached: false,
        }
    }

    /// A read-only cache that never reads `dir` and holds only what is recorded in it, for
    /// the epochs a read-only store creates for its own sessions.
    #[must_use]
    pub fn detached(dir: impl Into<PathBuf>, capacity: NonZeroUsize) -> Self {
        Self {
            detached: true,
            ..Self::new(dir, Access::ReadOnly, capacity)
        }
    }

    /// The stored signature of `pos`.
    pub fn get(&mut self, pos: RegionPos) -> Option<&RegionSignature> {
        self.file(file_region(pos)).signatures.get(&pos)
    }

    /// Records a chunk's surface, replacing an older one.
    pub fn record(&mut self, chunk_x: i32, chunk_z: i32, surface: ChunkSurface) {
        let pos = RegionPos::of_chunk(chunk_x, chunk_z);
        let file = self.file(file_region(pos));
        file.dirty = true;
        file.signatures
            .entry(pos)
            .or_insert_with(|| RegionSignature::new(pos))
            .record(chunk_x, chunk_z, surface);
    }

    /// Records every chunk of `newer` over the stored signature of its region.
    pub fn merge(&mut self, newer: &RegionSignature) {
        let pos = newer.pos();
        let file = self.file(file_region(pos));
        file.dirty = true;
        file.signatures
            .entry(pos)
            .or_insert_with(|| RegionSignature::new(pos))
            .merge_from(newer);
    }

    /// Every stored signature: files on disk and in memory.
    pub fn all(&mut self) -> io::Result<Vec<RegionSignature>> {
        let mut regions = self.file_regions()?;
        regions.extend(self.files.keys().copied());
        regions.sort_unstable();
        regions.dedup();
        let mut all = Vec::new();
        for region in regions {
            all.extend(self.file(region).signatures.values().cloned());
        }
        Ok(all)
    }

    /// Writes every changed file; failures become events. True when all were written.
    pub fn flush(&mut self) -> bool {
        let mut all_written = true;
        for (&region, file) in &mut self.files {
            all_written &= write_file(&self.dir, self.access, region, file, &mut self.events);
        }
        all_written
    }

    /// Forgets `region`'s file and deletes it, returning the bytes freed.
    pub fn delete(&mut self, region: RegionId) -> io::Result<u64> {
        self.files.remove(&region);
        if self.access == Access::ReadOnly {
            return Ok(0);
        }
        let path = self.dir.join(signature_file_name(region));
        let len = match std::fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        table::remove_if_present(&path)?;
        Ok(len)
    }

    /// Takes the events recorded since the last call.
    pub fn drain_events(&mut self) -> Vec<SignatureEvent> {
        std::mem::take(&mut self.events)
    }

    /// Storage regions with a signature file on disk; none for a detached cache.
    pub fn file_regions(&self) -> io::Result<Vec<RegionId>> {
        if self.detached {
            return Ok(Vec::new());
        }
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut regions = Vec::new();
        for entry in entries {
            if let Some(region) = entry?.file_name().to_str().and_then(signature_region) {
                regions.push(region);
            }
        }
        regions.sort_unstable();
        Ok(regions)
    }

    /// The loaded file of `region`, loading it and evicting the least recently used file
    /// when the cache is full. Evicted changes are written first.
    fn file(&mut self, region: RegionId) -> &mut SignatureFile {
        self.tick += 1;
        if !self.files.contains_key(&region) {
            if self.files.len() >= self.capacity.get() {
                self.evict_least_recent();
            }
            let loaded = self.load(region);
            self.files.insert(region, loaded);
        }
        let file = self.files.get_mut(&region).expect("inserted above");
        file.last_use = self.tick;
        file
    }

    /// Reads `region`'s file; missing or unreadable files load empty.
    fn load(&mut self, region: RegionId) -> SignatureFile {
        if self.detached {
            return SignatureFile::default();
        }
        let path = self.dir.join(signature_file_name(region));
        let decoded = match table::read_bounded(&path, MAX_SIGNATURE_FILE_BYTES) {
            Ok(None) => return SignatureFile::default(),
            Ok(Some(bytes)) => bytes.and_then(|bytes| decode(region, &bytes)),
            Err(error) => {
                self.events.push(SignatureEvent::ReadFailed { path, error });
                return SignatureFile {
                    unread: true,
                    ..SignatureFile::default()
                };
            }
        };
        match decoded {
            Ok(signatures) => SignatureFile {
                signatures,
                ..SignatureFile::default()
            },
            Err(error) => {
                self.events.push(SignatureEvent::Unreadable { path, error });
                SignatureFile::default()
            }
        }
    }

    /// Writes the least recently used file when it changed, then drops it from memory.
    fn evict_least_recent(&mut self) {
        let Some(&region) = self
            .files
            .iter()
            .min_by_key(|(_, file)| file.last_use)
            .map(|(region, _)| region)
        else {
            return;
        };
        if let Some(mut file) = self.files.remove(&region) {
            write_file(&self.dir, self.access, region, &mut file, &mut self.events);
        }
    }
}

/// Writes `file` when it changed and the cache may write; true unless writing failed. A
/// file whose read failed is read again first and recorded over; while that read still
/// fails nothing is written, so the stored signatures are never lost to a transient
/// error.
fn write_file(
    dir: &Path,
    access: Access,
    region: RegionId,
    file: &mut SignatureFile,
    events: &mut Vec<SignatureEvent>,
) -> bool {
    if !file.dirty || access == Access::ReadOnly {
        return true;
    }
    let path = dir.join(signature_file_name(region));
    if file.unread {
        match table::read_bounded(&path, MAX_SIGNATURE_FILE_BYTES) {
            Err(error) => {
                events.push(SignatureEvent::ReadFailed { path, error });
                return false;
            }
            Ok(Some(Ok(bytes))) => {
                // A malformed file holds nothing worth keeping and is replaced.
                if let Ok(stored) = decode(region, &bytes) {
                    merge_under(&mut file.signatures, stored);
                }
            }
            Ok(_) => {}
        }
        file.unread = false;
    }
    let written = std::fs::create_dir_all(dir)
        .and_then(|()| table::replace(&path, &encode(region, file), Durability::Cached));
    match written {
        Ok(()) => {
            file.dirty = false;
            true
        }
        Err(error) => {
            events.push(SignatureEvent::WriteFailed { path, error });
            false
        }
    }
}

/// Adds `stored` signatures under the newer ones in `newer`, which win per chunk.
fn merge_under(
    newer: &mut BTreeMap<RegionPos, RegionSignature>,
    stored: BTreeMap<RegionPos, RegionSignature>,
) {
    for (pos, mut signature) in stored {
        if let Some(own) = newer.get(&pos) {
            signature.merge_from(own);
        }
        newer.insert(pos, signature);
    }
}

/// The sealed file: storage region x and z, entry count, then per signature its index
/// within the region (`z * SIGNATURE_SIDE + x`), byte length and encoding.
fn encode(region: RegionId, file: &SignatureFile) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&region.x().to_le_bytes());
    body.extend_from_slice(&region.z().to_le_bytes());
    body.extend_from_slice(&(file.signatures.len() as u32).to_le_bytes());
    let mut encoded = Vec::new();
    for (pos, signature) in &file.signatures {
        encoded.clear();
        signature.encode_into(&mut encoded);
        let local =
            pos.z.rem_euclid(SIGNATURE_SIDE) * SIGNATURE_SIDE + pos.x.rem_euclid(SIGNATURE_SIDE);
        body.extend_from_slice(&(local as u16).to_le_bytes());
        body.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
        body.extend_from_slice(&encoded);
    }
    seal(SIGNATURE_MAGIC, SIGNATURE_FORMAT_VERSION, &body)
}

/// Reads [`encode`] output for `region`, rejecting other regions, repeated or
/// out-of-range indices and malformed signatures.
fn decode(
    region: RegionId,
    bytes: &[u8],
) -> Result<BTreeMap<RegionPos, RegionSignature>, TableError> {
    let mut reader = BodyReader::new(unseal(SIGNATURE_MAGIC, SIGNATURE_FORMAT_VERSION, bytes)?);
    if (reader.i32()?, reader.i32()?) != (region.x(), region.z()) {
        return Err(TableError::Body("signature file names another region"));
    }
    let count = reader.count(ENTRY_HEADER_LEN)?;
    let mut signatures = BTreeMap::new();
    for _ in 0..count {
        let local = i32::from(reader.u16()?);
        if local as usize >= SIGNATURES_PER_FILE {
            return Err(TableError::Body("signature index outside the region"));
        }
        let len = reader.u32()? as usize;
        let pos = RegionPos {
            x: region.x() * SIGNATURE_SIDE + local % SIGNATURE_SIDE,
            z: region.z() * SIGNATURE_SIDE + local / SIGNATURE_SIDE,
        };
        let signature = RegionSignature::decode(pos, reader.take(len)?)
            .map_err(|_| TableError::Body("malformed region signature"))?;
        if signatures.insert(pos, signature).is_some() {
            return Err(TableError::Body("signature index repeated"));
        }
    }
    reader.finish()?;
    Ok(signatures)
}

#[cfg(test)]
mod tests;

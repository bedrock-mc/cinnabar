//! One region file: an append-only log of section records with a committed index.
//!
//! Layout, little-endian:
//!
//! | Bytes | Field |
//! | --- | --- |
//! | 0..4 | [`REGION_MAGIC`] |
//! | 4..6 | [`REGION_FORMAT_VERSION`] |
//! | 6 | level |
//! | 7 | reserved, zero |
//! | 8..12, 12..16 | region x, z |
//! | 16..20 | CRC32 of bytes 0..16 |
//! | 24..44, 44..64 | two commit slots: sequence `u64`, index frame offset `u64`, CRC32 |
//! | 64.. | [`crate::frame`]s |
//!
//! Records, tombstones and index frames are only ever appended. A commit appends the
//! buffered frames, then an index frame (a full index, or a delta over the previous index
//! frame), and last overwrites the older commit slot. A crash before that last write
//! leaves the newer slot and the index it names intact; a torn slot fails its CRC and the
//! other slot is used. When no slot is valid, or the named index chain is truncated or
//! corrupt, the file is scanned frame by frame and the index rebuilt from the valid
//! records, tombstones and chunk hashes in file order.
//!
//! Durability: this is a rebuildable cache, so commits are not fsynced; benchmarks showed
//! per-batch fsync costing over 20× the write throughput. A process crash loses only
//! uncommitted changes. A power loss can also lose recent commits, or leave a committed
//! record unreadable; reads then return [`RegionError::Corrupt`] and callers rebuild that
//! section from its chunks. [`RegionFile::sync`] and compaction flush to disk.

mod compact;
mod header;
mod load;
mod scan;
#[cfg(test)]
mod tests;

use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use lod::{codec::RecordHeader, geometry::SectionKey};

pub use self::compact::Compaction;
use self::{
    header::{commit_slot_bytes, commit_slot_offset, header_bytes},
    load::read_frame_at,
};
use crate::{
    error::{Recovery, RegionError},
    frame::{FRAME_HEADER_LEN, FrameError, FrameKind, append_frame, read_frame},
    positional::{read_exact_at, write_all_at},
    region_id::{ChunkSlot, RegionId, SectionSlot},
    region_index::{RecordEntry, RegionIndex},
};

/// Leading bytes of every region file.
pub const REGION_MAGIC: [u8; 4] = *b"CLDR";
/// Version of the region file layout.
pub const REGION_FORMAT_VERSION: u16 = 1;
/// Extension of a compaction's temporary sibling, which replaces the file by rename.
pub const TEMP_EXTENSION: &str = "tmp";
/// Delta index frames allowed after a full one before a commit writes a full index again.
pub const MAX_INDEX_CHAIN: u32 = 32;
/// Dead bytes a file must hold before compaction; below it rewriting gains too little.
pub const COMPACTION_MIN_DEAD_BYTES: u64 = 256 * 1024;
/// Offset of the first frame.
pub const DATA_START: u64 = 64;

const HEADER_CHECKED_LEN: usize = 16;
const COMMIT_SLOT_LEN: usize = 20;
const COMMIT_SLOT_OFFSETS: [usize; 2] = [24, 44];
/// Buffered appends are written out once they reach this size, even before a commit.
const PENDING_WRITE_BYTES: usize = 1 << 20;

const _: () = assert!(COMMIT_SLOT_OFFSETS[0] >= HEADER_CHECKED_LEN + 4);
const _: () = assert!(COMMIT_SLOT_OFFSETS[1] >= COMMIT_SLOT_OFFSETS[0] + COMMIT_SLOT_LEN);
const _: () = assert!(COMMIT_SLOT_OFFSETS[1] + COMMIT_SLOT_LEN <= DATA_START as usize);

/// Work that must be durable before a region file puts frames on disk. A store saves the
/// material registry here, so no record on disk names a material the saved registry lacks.
pub trait WriteBarrier: Send + Sync + fmt::Debug {
    /// Makes everything the frames about to be written depend on durable. An error stops
    /// the write, which then fails as if the file had failed.
    fn before_write(&self) -> io::Result<()>;
}

/// Whether a file may be written. A store that lost its lock opens read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadWrite,
    ReadOnly,
}

/// The committed index frames the next delta builds on.
#[derive(Debug, Clone, Copy, Default)]
struct IndexChain {
    /// Offset of the newest committed index frame.
    last: Option<u64>,
    /// Frame bytes of the chain's full index.
    full_bytes: u64,
    /// Frame bytes of the deltas after it.
    delta_bytes: u64,
    deltas: u32,
}

/// An open region file. Changes are buffered until [`Self::commit`]; dropping the handle
/// discards uncommitted changes, as a crash would.
#[derive(Debug)]
pub struct RegionFile {
    file: File,
    path: PathBuf,
    id: RegionId,
    access: Access,
    index: RegionIndex,
    /// Bytes on disk; buffered frames start here.
    written_end: u64,
    pending: Vec<u8>,
    live_record_bytes: u64,
    chain: IndexChain,
    /// Highest commit sequence seen in this file.
    seq: u64,
    /// The next commit writes a full index, after a scan rebuilt it.
    force_full: bool,
    recovery: Option<Recovery>,
    scratch: Vec<u8>,
    barrier: Option<Arc<dyn WriteBarrier>>,
}

impl RegionFile {
    /// Opens the file at `path` for writing, creating it when missing or empty. A leftover
    /// compaction sibling is removed. A header error leaves the file untouched; callers
    /// replace it with [`Self::create`].
    pub fn open_writable(path: &Path, id: RegionId) -> Result<Self, RegionError> {
        remove_temp_sibling(path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let len = file.metadata()?.len();
        if len == 0 {
            return Self::initialise(file, path, id);
        }
        Self::load(file, path, id, Access::ReadWrite, len)
    }

    /// [`Self::open_writable`] for a file that already exists; `None` when it does not, in
    /// which case nothing is created.
    pub fn open_existing_writable(path: &Path, id: RegionId) -> Result<Option<Self>, RegionError> {
        let file = match OpenOptions::new().read(true).write(true).open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        remove_temp_sibling(path)?;
        let len = file.metadata()?.len();
        if len == 0 {
            return Self::initialise(file, path, id).map(Some);
        }
        Self::load(file, path, id, Access::ReadWrite, len).map(Some)
    }

    /// Replaces any file at `path` with an empty region.
    pub fn create(path: &Path, id: RegionId) -> Result<Self, RegionError> {
        remove_temp_sibling(path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        Self::initialise(file, path, id)
    }

    /// Opens the file at `path` without ever writing it; `None` when it does not exist.
    /// The view is a snapshot: commits by a writer in another process after this open
    /// are not seen.
    pub fn open_read_only(path: &Path, id: RegionId) -> Result<Option<Self>, RegionError> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let len = file.metadata()?.len();
        Self::load(file, path, id, Access::ReadOnly, len).map(Some)
    }

    /// Writes the fixed header of an empty region with no valid commit slot.
    fn initialise(file: File, path: &Path, id: RegionId) -> Result<Self, RegionError> {
        write_all_at(&file, &header_bytes(id), 0)?;
        Ok(Self::empty(file, path, id, Access::ReadWrite, DATA_START))
    }

    /// A handle over `file` with no records and no committed index, whose frames start
    /// at `written_end`.
    fn empty(file: File, path: &Path, id: RegionId, access: Access, written_end: u64) -> Self {
        Self {
            file,
            path: path.to_owned(),
            id,
            access,
            index: RegionIndex::default(),
            written_end,
            pending: Vec::new(),
            live_record_bytes: 0,
            chain: IndexChain::default(),
            seq: 0,
            force_full: false,
            recovery: None,
            scratch: Vec::new(),
            barrier: None,
        }
    }

    /// Runs `barrier` before every later write of frames to this file.
    pub fn set_barrier(&mut self, barrier: Arc<dyn WriteBarrier>) {
        self.barrier = Some(barrier);
    }

    /// This file's region.
    #[must_use]
    pub fn id(&self) -> RegionId {
        self.id
    }

    /// This file's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether this handle may write.
    #[must_use]
    pub fn access(&self) -> Access {
        self.access
    }

    /// File length once buffered frames are written.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.written_end + self.pending.len() as u64
    }

    /// True when the file holds no frames.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == DATA_START
    }

    /// Number of live records.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.index.record_count()
    }

    /// Keys of every live record, in no particular order.
    pub fn keys(&self) -> impl Iterator<Item = SectionKey> + '_ {
        self.index
            .records()
            .filter_map(|(slot, _)| self.id.key_at(slot))
    }

    /// Bytes no live record or committed index uses: overwritten records, tombstones,
    /// superseded index frames and torn tails.
    #[must_use]
    pub fn dead_bytes(&self) -> u64 {
        let live =
            DATA_START + self.live_record_bytes + self.chain.full_bytes + self.chain.delta_bytes;
        self.len().saturating_sub(live)
    }

    /// True when dead bytes exceed half the file and [`COMPACTION_MIN_DEAD_BYTES`].
    #[must_use]
    pub fn needs_compaction(&self) -> bool {
        let dead = self.dead_bytes();
        self.access == Access::ReadWrite
            && dead >= COMPACTION_MIN_DEAD_BYTES
            && dead * 2 > self.len()
    }

    /// True when changes await [`Self::commit`].
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.index.is_dirty() || self.force_full || !self.pending.is_empty()
    }

    /// How the index was rebuilt when this file opened, if it was.
    #[must_use]
    pub fn recovery(&self) -> Option<&Recovery> {
        self.recovery.as_ref()
    }

    /// Takes the open-time recovery report so it is reported once.
    pub fn take_recovery(&mut self) -> Option<Recovery> {
        self.recovery.take()
    }

    /// True when a record for `key` is stored.
    pub fn contains(&self, key: SectionKey) -> Result<bool, RegionError> {
        Ok(self.index.record(self.slot_of(key)?).is_some())
    }

    /// Replaces `out` with the stored record of `key`; false when none is stored. A record
    /// that fails its frame checks returns [`RegionError::Corrupt`].
    pub fn get_into(&self, key: SectionKey, out: &mut Vec<u8>) -> Result<bool, RegionError> {
        let Some(entry) = self.index.record(self.slot_of(key)?) else {
            out.clear();
            return Ok(false);
        };
        self.read_record(key, entry, out)?;
        Ok(true)
    }

    /// The stored record of `key`, as [`Self::get_into`].
    pub fn get(&self, key: SectionKey) -> Result<Option<Vec<u8>>, RegionError> {
        let mut out = Vec::new();
        Ok(self.get_into(key, &mut out)?.then_some(out))
    }

    /// Stores a section record, replacing any record of its key, and returns the key.
    /// The record must pass [`RecordHeader::read`] and belong to this region.
    pub fn put(&mut self, record: &[u8]) -> Result<SectionKey, RegionError> {
        let key = RecordHeader::read(record)?.key;
        self.put_record(key, record)?;
        Ok(key)
    }

    /// [`Self::put`] for a record whose header was already read as `key`.
    /// A failed write leaves the file as it was before the call.
    pub(crate) fn put_record(&mut self, key: SectionKey, record: &[u8]) -> Result<(), RegionError> {
        self.require_writable()?;
        let slot = self.slot_of(key)?;
        let entry = RecordEntry {
            offset: self.len(),
            len: record.len() as u32,
        };
        self.append_pending(FrameKind::Record, key.bits(), record)?;
        self.live_record_bytes += record_frame_len(entry);
        if let Some(old) = self.index.set_record(slot, entry) {
            self.live_record_bytes -= record_frame_len(old);
        }
        Ok(())
    }

    /// Removes the record of `key`; false when none was stored.
    pub fn remove(&mut self, key: SectionKey) -> Result<bool, RegionError> {
        self.require_writable()?;
        let slot = self.slot_of(key)?;
        if self.index.record(slot).is_none() {
            return Ok(false);
        }
        self.append_pending(FrameKind::Tombstone, key.bits(), &[])?;
        let old = self.index.remove_record(slot).expect("checked above");
        self.live_record_bytes -= record_frame_len(old);
        Ok(true)
    }

    /// Forgets the record of `key` after a read found it corrupt, and clears the content
    /// hashes of the chunks a level-0 record covers, so those chunks are sampled again
    /// when next sent instead of being skipped as unchanged.
    pub fn drop_corrupt(&mut self, key: SectionKey) -> Result<(), RegionError> {
        let slot = self.slot_of(key)?;
        self.remove(key)?;
        if self.id.level() == 0 {
            for chunk in slot.chunk_slots() {
                self.index.set_hash(chunk, None);
            }
        }
        Ok(())
    }

    /// The content hash stored for chunk `(x, z)`.
    pub fn chunk_hash(&self, x: i32, z: i32) -> Result<Option<u64>, RegionError> {
        Ok(self.index.hash(self.chunk_slot_of(x, z)?))
    }

    /// Stores or clears the content hash of chunk `(x, z)`.
    pub fn set_chunk_hash(&mut self, x: i32, z: i32, hash: Option<u64>) -> Result<(), RegionError> {
        self.require_writable()?;
        let slot = self.chunk_slot_of(x, z)?;
        self.index.set_hash(slot, hash);
        Ok(())
    }

    /// Writes buffered frames and an index, then the commit slot; false when nothing
    /// changed. Read-only handles never have changes.
    pub fn commit(&mut self) -> Result<bool, RegionError> {
        if !self.is_dirty() {
            return Ok(false);
        }
        self.require_writable()?;
        let delta_frame = (FRAME_HEADER_LEN + self.index.delta_len()) as u64;
        let previous = self.chain.last.filter(|_| {
            !self.force_full
                && self.chain.deltas < MAX_INDEX_CHAIN
                && (self.chain.delta_bytes + delta_frame) * 2 <= self.chain.full_bytes
        });
        let seq = self.seq + 1;
        let offset = self.len();
        self.index.encode(previous, &mut self.scratch);
        let frame_len = (FRAME_HEADER_LEN + self.scratch.len()) as u64;
        let before = self.pending.len();
        append_frame(&mut self.pending, FrameKind::Index, seq, &self.scratch);
        if let Err(error) = self.write_pending() {
            // A failed commit leaves no index frame behind, so retries never pile up.
            self.pending.truncate(before);
            return Err(error.into());
        }
        write_all_at(
            &self.file,
            &commit_slot_bytes(seq, offset),
            commit_slot_offset(seq),
        )?;

        self.chain = match previous {
            Some(_) => IndexChain {
                last: Some(offset),
                delta_bytes: self.chain.delta_bytes + frame_len,
                deltas: self.chain.deltas + 1,
                ..self.chain
            },
            None => IndexChain {
                last: Some(offset),
                full_bytes: frame_len,
                delta_bytes: 0,
                deltas: 0,
            },
        };
        self.seq = seq;
        self.force_full = false;
        self.index.mark_clean();
        Ok(true)
    }

    /// Commits, then flushes the file's data to disk.
    pub fn sync(&mut self) -> Result<(), RegionError> {
        if self.access == Access::ReadOnly {
            return Ok(());
        }
        self.commit()?;
        self.file.sync_data()?;
        Ok(())
    }

    /// Fails with [`RegionError::ReadOnly`] unless this handle may write.
    fn require_writable(&self) -> Result<(), RegionError> {
        match self.access {
            Access::ReadWrite => Ok(()),
            Access::ReadOnly => Err(RegionError::ReadOnly),
        }
    }

    /// The slot of `key` in this file; fails for keys of another region.
    fn slot_of(&self, key: SectionKey) -> Result<SectionSlot, RegionError> {
        let (region, slot) = RegionId::of_key(key);
        if region != self.id {
            return Err(RegionError::KeyOutsideRegion {
                key,
                region: self.id,
            });
        }
        Ok(slot)
    }

    /// The hash slot of chunk `(x, z)`; fails above level 0 and for chunks of another
    /// region.
    fn chunk_slot_of(&self, x: i32, z: i32) -> Result<ChunkSlot, RegionError> {
        if self.id.level() != 0 {
            return Err(RegionError::NotLevelZero(self.id));
        }
        let (region, slot) = RegionId::of_chunk(x, z);
        if region != self.id {
            return Err(RegionError::ChunkOutsideRegion {
                x,
                z,
                region: self.id,
            });
        }
        Ok(slot)
    }

    /// Reads and verifies the record frame of `key` at `entry`, leaving its payload in
    /// `out`.
    fn read_record(
        &self,
        key: SectionKey,
        entry: RecordEntry,
        out: &mut Vec<u8>,
    ) -> Result<(), RegionError> {
        let corrupt = |error| RegionError::Corrupt {
            key,
            offset: entry.offset,
            error,
        };
        let total = record_frame_len(entry) as usize;
        out.clear();
        out.resize(total, 0);
        if entry.offset >= self.written_end {
            let start = (entry.offset - self.written_end) as usize;
            let buffered = self.pending.get(start..start + total).ok_or_else(|| {
                corrupt(FrameError::Truncated {
                    needed: total,
                    available: self.pending.len().saturating_sub(start),
                })
            })?;
            out.copy_from_slice(buffered);
        } else {
            match read_exact_at(&self.file, out, entry.offset) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                    let available = self.written_end.saturating_sub(entry.offset) as usize;
                    return Err(corrupt(FrameError::Truncated {
                        needed: total,
                        available,
                    }));
                }
                Err(error) => return Err(error.into()),
            }
        }
        let (header, _) = read_frame(out).map_err(corrupt)?;
        header
            .expect(FrameKind::Record, key.bits())
            .map_err(corrupt)?;
        out.copy_within(FRAME_HEADER_LEN.., 0);
        out.truncate(entry.len as usize);
        Ok(())
    }

    /// Buffers one frame, writing the buffer out once it reaches [`PENDING_WRITE_BYTES`].
    /// When that write fails the frame is taken back out, so the buffer stays bounded and
    /// the caller changes nothing else.
    fn append_pending(
        &mut self,
        kind: FrameKind,
        tag: u64,
        payload: &[u8],
    ) -> Result<(), RegionError> {
        let before = self.pending.len();
        append_frame(&mut self.pending, kind, tag, payload);
        if self.pending.len() >= PENDING_WRITE_BYTES
            && let Err(error) = self.write_pending()
        {
            self.pending.truncate(before);
            return Err(error.into());
        }
        Ok(())
    }

    /// Appends the buffered frames to the file after the write barrier. On failure the
    /// buffer is kept and nothing counts as written.
    fn write_pending(&mut self) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        if let Some(barrier) = &self.barrier {
            barrier.before_write()?;
        }
        write_all_at(&self.file, &self.pending, self.written_end)?;
        self.written_end += self.pending.len() as u64;
        self.pending.clear();
        Ok(())
    }
}

/// Bytes of the frame holding the record at `entry`.
const fn record_frame_len(entry: RecordEntry) -> u64 {
    FRAME_HEADER_LEN as u64 + entry.len as u64
}

/// The path of `path`'s compaction sibling.
fn temp_sibling(path: &Path) -> PathBuf {
    path.with_extension(TEMP_EXTENSION)
}

/// Removes a compaction sibling a crash left behind.
fn remove_temp_sibling(path: &Path) -> io::Result<()> {
    match fs::remove_file(temp_sibling(path)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

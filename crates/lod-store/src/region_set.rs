//! The region files of one dimension directory behind a bounded cache of open handles.

#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    fs, io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
};

use foldhash::fast::FixedState;
use lod::{codec::RecordHeader, geometry::SectionKey};

use crate::{
    error::{HeaderError, Recovery, RegionError},
    region_file::{Access, Compaction, RegionFile, WriteBarrier},
    region_id::RegionId,
};

/// Default bound on open region file handles: of one set, and of a whole store, which
/// shares it among its epochs. Well inside the 256 descriptors macOS allows a process by
/// default, which the renderer and the rest of the client share.
pub const DEFAULT_OPEN_REGIONS: NonZeroUsize = NonZeroUsize::new(64).expect("non-zero");
/// Default bytes of live data one [`RegionSet::commit_all`] rewrites by compaction, so a
/// flush never stalls the queued reads behind it for long. The file with the most dead
/// bytes goes first and is always compacted; the rest wait for later flushes.
pub const COMPACTION_BUDGET_BYTES: u64 = 16 << 20;

/// Something a set did on its own that its owner should count and log.
#[derive(Debug)]
pub enum RegionEvent {
    /// A file's index was rebuilt by scanning it.
    Recovered {
        region: RegionId,
        recovery: Recovery,
    },
    /// A file's header was unusable; a writer replaced it with an empty region and a
    /// reader treats it as absent.
    Unusable {
        region: RegionId,
        error: HeaderError,
        replaced: bool,
    },
    /// Committing a file failed; its uncommitted changes may be lost.
    CommitFailed {
        region: RegionId,
        error: RegionError,
    },
    /// A file was compacted.
    Compacted {
        region: RegionId,
        compaction: Compaction,
    },
    /// Compacting a file failed; the file is unchanged.
    CompactionFailed {
        region: RegionId,
        error: RegionError,
    },
}

/// A cached handle, or a remembered absent file.
#[derive(Debug)]
struct Cached {
    file: Option<RegionFile>,
    last_use: u64,
}

/// The region files of one epoch directory, keeping at most `capacity` open. A handle
/// evicted from the cache is committed first. Dropping the set discards uncommitted
/// changes; owners call [`Self::commit_all`] or [`Self::close`]. Reads never create files
/// or the directory: only writes do, so a lookup miss costs one failed open and is then
/// remembered. A read-only set sees each file as it was when first opened.
///
/// A level-0 file's chunk hashes vouch for the records of every level above it, so the
/// set never commits a level-0 file before the coarser files open beside it: a process
/// crash cannot leave a hash committed without the mips it vouches for. Commits are not
/// fsynced, so a power loss may still reorder them; callers that find a coarse record
/// missing or corrupt under a vouched chunk rebuild it from its children.
#[derive(Debug)]
pub struct RegionSet {
    dir: PathBuf,
    access: Access,
    capacity: NonZeroUsize,
    open: HashMap<RegionId, Cached, FixedState>,
    tick: u64,
    events: Vec<RegionEvent>,
    barrier: Option<Arc<dyn WriteBarrier>>,
    detached: bool,
    compaction_budget: u64,
}

impl RegionSet {
    /// A set over `dir`, which a writable set creates with its first file.
    #[must_use]
    pub fn open(dir: impl Into<PathBuf>, access: Access, capacity: NonZeroUsize) -> Self {
        Self {
            dir: dir.into(),
            access,
            capacity,
            open: HashMap::default(),
            tick: 0,
            events: Vec::new(),
            barrier: None,
            detached: false,
            compaction_budget: COMPACTION_BUDGET_BYTES,
        }
    }

    /// Rewrites at most `bytes` of live data by compaction per [`Self::commit_all`],
    /// beyond the first file, instead of [`COMPACTION_BUDGET_BYTES`].
    #[must_use]
    pub fn with_compaction_budget(mut self, bytes: u64) -> Self {
        self.compaction_budget = bytes;
        self
    }

    /// Runs `barrier` before any file of this set writes frames, including the commits
    /// of evicted handles.
    #[must_use]
    pub fn with_barrier(mut self, barrier: Arc<dyn WriteBarrier>) -> Self {
        self.barrier = Some(barrier);
        self
    }

    /// A read-only set with no files at all, which never touches `dir`: the epochs a
    /// read-only store creates for its own sessions exist only in its memory, and their
    /// ids may name another client's directories.
    #[must_use]
    pub fn detached(dir: impl Into<PathBuf>, capacity: NonZeroUsize) -> Self {
        Self {
            detached: true,
            ..Self::open(dir, Access::ReadOnly, capacity)
        }
    }

    /// The dimension directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether this set may write.
    #[must_use]
    pub fn access(&self) -> Access {
        self.access
    }

    /// Number of cached handles, including remembered absent files.
    #[must_use]
    pub fn cached_count(&self) -> usize {
        self.open.len()
    }

    /// Number of open files, each holding a descriptor.
    #[must_use]
    pub fn open_files(&self) -> usize {
        self.open
            .values()
            .filter(|cached| cached.file.is_some())
            .count()
    }

    /// Commits and closes the least recently used open file; false when none is open.
    pub fn close_least_recent(&mut self) -> bool {
        self.evict_least_recent(true)
    }

    /// The path of `region`'s file.
    #[must_use]
    pub fn region_path(&self, region: RegionId) -> PathBuf {
        self.dir.join(region.file_name())
    }

    /// Replaces `out` with the stored record of `key`; false when none is stored. A
    /// corrupt record is reported once as [`RegionError::Corrupt`]; a writable set then
    /// forgets it and the content hashes it covered (see [`RegionFile::drop_corrupt`]).
    pub fn get_into(&mut self, key: SectionKey, out: &mut Vec<u8>) -> Result<bool, RegionError> {
        let Some(file) = self.handle(RegionId::of_key(key).0, false)? else {
            out.clear();
            return Ok(false);
        };
        let read = file.get_into(key, out);
        if matches!(read, Err(RegionError::Corrupt { .. })) && file.access() == Access::ReadWrite {
            file.drop_corrupt(key)?;
        }
        read
    }

    /// The stored record of `key`.
    pub fn get(&mut self, key: SectionKey) -> Result<Option<Vec<u8>>, RegionError> {
        let mut out = Vec::new();
        Ok(self.get_into(key, &mut out)?.then_some(out))
    }

    /// True when a record for `key` is stored.
    pub fn contains(&mut self, key: SectionKey) -> Result<bool, RegionError> {
        match self.handle(RegionId::of_key(key).0, false)? {
            Some(file) => file.contains(key),
            None => Ok(false),
        }
    }

    /// Stores a section record in its region and returns its key; see [`RegionFile::put`].
    pub fn put(&mut self, record: &[u8]) -> Result<SectionKey, RegionError> {
        let key = RecordHeader::read(record)?.key;
        self.put_record(key, record)?;
        Ok(key)
    }

    /// [`Self::put`] for a record whose header was already read as `key`.
    pub(crate) fn put_record(&mut self, key: SectionKey, record: &[u8]) -> Result<(), RegionError> {
        self.require_writable()?;
        self.writable_handle(RegionId::of_key(key).0)?
            .put_record(key, record)
    }

    /// Removes the record of `key`; false when none was stored.
    pub fn remove(&mut self, key: SectionKey) -> Result<bool, RegionError> {
        self.require_writable()?;
        match self.handle(RegionId::of_key(key).0, false)? {
            Some(file) => file.remove(key),
            None => Ok(false),
        }
    }

    /// The content hash stored for chunk `(x, z)`.
    pub fn chunk_hash(&mut self, x: i32, z: i32) -> Result<Option<u64>, RegionError> {
        match self.handle(RegionId::of_chunk(x, z).0, false)? {
            Some(file) => file.chunk_hash(x, z),
            None => Ok(None),
        }
    }

    /// Stores or clears the content hash of chunk `(x, z)`.
    pub fn set_chunk_hash(&mut self, x: i32, z: i32, hash: Option<u64>) -> Result<(), RegionError> {
        self.require_writable()?;
        self.writable_handle(RegionId::of_chunk(x, z).0)?
            .set_chunk_hash(x, z, hash)
    }

    /// Commits every open file, coarsest level first, then compacts files past the
    /// dead-byte threshold within the compaction budget. Failures become events; true
    /// when every commit succeeded.
    pub fn commit_all(&mut self) -> bool {
        let all_committed = self.commit_levels(0);
        self.compact_within_budget();
        all_committed
    }

    /// Commits every open file at `min_level` or above, coarsest level first; true when
    /// every commit succeeded.
    fn commit_levels(&mut self, min_level: u8) -> bool {
        let mut regions: Vec<RegionId> = self
            .open
            .iter()
            .filter(|(region, cached)| region.level() >= min_level && cached.file.is_some())
            .map(|(&region, _)| region)
            .collect();
        regions.sort_unstable_by_key(|region| std::cmp::Reverse(region.level()));
        let mut all_committed = true;
        for region in regions {
            let file = self
                .open
                .get_mut(&region)
                .and_then(|cached| cached.file.as_mut())
                .expect("listed above");
            if let Err(error) = file.commit() {
                all_committed = false;
                self.events
                    .push(RegionEvent::CommitFailed { region, error });
            }
        }
        all_committed
    }

    /// Compacts open files past the dead-byte threshold, most dead bytes first, until
    /// the budget of live data was rewritten. The first is always compacted, so a file
    /// larger than the budget still is.
    fn compact_within_budget(&mut self) {
        let mut due: Vec<(u64, RegionId)> = self
            .open
            .iter()
            .filter_map(|(&region, cached)| {
                let file = cached.file.as_ref()?;
                file.needs_compaction().then(|| (file.dead_bytes(), region))
            })
            .collect();
        due.sort_unstable_by(|a, b| b.cmp(a));
        let mut rewritten = 0;
        for (_, region) in due {
            if rewritten >= self.compaction_budget {
                break;
            }
            let file = self
                .open
                .get_mut(&region)
                .and_then(|cached| cached.file.as_mut())
                .expect("listed above");
            rewritten += file.len() - file.dead_bytes();
            self.events.push(match file.compact() {
                Ok(compaction) => RegionEvent::Compacted { region, compaction },
                Err(error) => RegionEvent::CompactionFailed { region, error },
            });
        }
    }

    /// Commits and flushes every open file to disk; true when all succeeded.
    pub fn sync_all(&mut self) -> bool {
        let mut all_synced = self.commit_all();
        for (&region, cached) in &mut self.open {
            if let Some(file) = cached.file.as_mut()
                && let Err(error) = file.sync()
            {
                all_synced = false;
                self.events
                    .push(RegionEvent::CommitFailed { region, error });
            }
        }
        all_synced
    }

    /// Syncs every open file and returns the events not yet drained.
    pub fn close(mut self) -> Vec<RegionEvent> {
        self.sync_all();
        self.events
    }

    /// Takes the events recorded since the last call.
    pub fn drain_events(&mut self) -> Vec<RegionEvent> {
        std::mem::take(&mut self.events)
    }

    /// Every region file in the directory, by canonical file name. Missing directories
    /// and detached sets hold none.
    pub fn region_ids(&self) -> io::Result<Vec<RegionId>> {
        if self.detached {
            return Ok(Vec::new());
        }
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut regions = Vec::new();
        for entry in entries {
            let entry = entry?;
            if let Some(region) = entry
                .file_name()
                .to_str()
                .and_then(RegionId::from_file_name)
            {
                regions.push(region);
            }
        }
        regions.sort_unstable();
        Ok(regions)
    }

    /// Deletes `region`'s file and any uncommitted changes to it, returning the bytes
    /// freed; zero when it did not exist.
    pub fn delete_region(&mut self, region: RegionId) -> Result<u64, RegionError> {
        self.require_writable()?;
        self.open.remove(&region);
        let path = self.region_path(region);
        let len = match fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error.into()),
        };
        match fs::remove_file(&path) {
            Ok(()) => Ok(len),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(error) => Err(error.into()),
        }
    }

    /// Fails with [`RegionError::ReadOnly`] unless this set may write.
    fn require_writable(&self) -> Result<(), RegionError> {
        match self.access {
            Access::ReadWrite => Ok(()),
            Access::ReadOnly => Err(RegionError::ReadOnly),
        }
    }

    /// The writable handle of `region`, creating its file.
    fn writable_handle(&mut self, region: RegionId) -> Result<&mut RegionFile, RegionError> {
        Ok(self
            .handle(region, true)?
            .expect("a writable set creates every region it writes"))
    }

    /// The cached handle of `region`, opening it and evicting the least recently used
    /// handle when the cache is full. A missing file is created when `create` is set on
    /// a writable set, and is otherwise remembered as absent and returned as `None`.
    fn handle(
        &mut self,
        region: RegionId,
        create: bool,
    ) -> Result<Option<&mut RegionFile>, RegionError> {
        self.tick += 1;
        let tick = self.tick;
        let cached_present = self.open.get(&region).map(|cached| cached.file.is_some());
        if cached_present != Some(true) && (cached_present.is_none() || create) {
            let file = self.open_file(region, create)?;
            if cached_present.is_none() && self.open.len() >= self.capacity.get() {
                self.evict_least_recent(false);
            }
            self.open.insert(
                region,
                Cached {
                    file,
                    last_use: tick,
                },
            );
        }
        let cached = self.open.get_mut(&region).expect("inserted above");
        cached.last_use = tick;
        Ok(cached.file.as_mut())
    }

    /// Opens `region`'s file, replacing an unusable one when writable. `None` when the
    /// file is missing and not created.
    fn open_file(
        &mut self,
        region: RegionId,
        create: bool,
    ) -> Result<Option<RegionFile>, RegionError> {
        if self.detached {
            return Ok(None);
        }
        let path = self.region_path(region);
        let opened = match self.access {
            Access::ReadWrite if create => {
                fs::create_dir_all(&self.dir)?;
                RegionFile::open_writable(&path, region).map(Some)
            }
            Access::ReadWrite => RegionFile::open_existing_writable(&path, region),
            Access::ReadOnly => RegionFile::open_read_only(&path, region),
        };
        let mut file = match opened {
            Ok(file) => file,
            Err(RegionError::Header(error)) => {
                let replaced = self.access == Access::ReadWrite;
                self.events.push(RegionEvent::Unusable {
                    region,
                    error,
                    replaced,
                });
                if !replaced {
                    return Ok(None);
                }
                Some(RegionFile::create(&path, region)?)
            }
            Err(error) => return Err(error),
        };
        if let Some(file) = file.as_mut() {
            if let Some(recovery) = file.take_recovery() {
                self.events
                    .push(RegionEvent::Recovered { region, recovery });
            }
            if let Some(barrier) = &self.barrier {
                file.set_barrier(Arc::clone(barrier));
            }
        }
        Ok(file)
    }

    /// Commits and drops the least recently used cached handle, or only open files when
    /// `files_only` is set; false when there was none. A dirty level-0 file first has
    /// every coarser open file committed, keeping the commit order of
    /// [`Self::commit_all`].
    fn evict_least_recent(&mut self, files_only: bool) -> bool {
        let Some((&region, dirty)) = self
            .open
            .iter()
            .filter(|(_, cached)| !files_only || cached.file.is_some())
            .min_by_key(|(_, cached)| cached.last_use)
            .map(|(region, cached)| {
                let dirty = cached.file.as_ref().is_some_and(RegionFile::is_dirty);
                (region, dirty)
            })
        else {
            return false;
        };
        if region.level() == 0 && dirty {
            self.commit_levels(1);
        }
        if let Some(Cached {
            file: Some(mut file),
            ..
        }) = self.open.remove(&region)
            && let Err(error) = file.commit()
        {
            self.events
                .push(RegionEvent::CommitFailed { region, error });
        }
        true
    }
}

//! Reading and writing a session's epoch.

use lod::geometry::SectionKey;

use super::{EpochFiles, Store};
use crate::{
    access::RegionRef, epochs::EpochRef, error::StoreError, identity::Dimension, minute::Minute,
    region_file::Access, region_id::RegionId, region_set::RegionSet, signatures::SignatureCache,
};

impl Store {
    /// Replaces `out` with the stored record of `key` in `dimension`'s session epoch, or
    /// while the session is undecided, in its [`Self::leading_epoch`] when the candidate
    /// has none; false when neither stores one. Only a found record counts as a use of
    /// its region.
    pub fn get_into(
        &mut self,
        dimension: Dimension,
        key: SectionKey,
        out: &mut Vec<u8>,
        now: Minute,
    ) -> Result<bool, StoreError> {
        let session = self.session_epoch(dimension)?;
        let leading = self.leading_epoch(dimension);
        for epoch in std::iter::once(session).chain(leading) {
            if self.files(epoch)?.regions.get_into(key, out)? {
                self.touch(epoch, RegionId::of_key(key).0, now);
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Stores a section record in `dimension`'s session epoch and returns its key.
    pub fn put(
        &mut self,
        dimension: Dimension,
        record: &[u8],
        now: Minute,
    ) -> Result<SectionKey, StoreError> {
        let key = lod::codec::RecordHeader::read(record)
            .map_err(crate::error::RegionError::from)?
            .key;
        self.put_record(dimension, key, record, now)?;
        Ok(key)
    }

    /// [`Self::put`] for a record whose header was already read as `key`.
    pub(crate) fn put_record(
        &mut self,
        dimension: Dimension,
        key: SectionKey,
        record: &[u8],
        now: Minute,
    ) -> Result<(), StoreError> {
        let epoch = self.writable_epoch(dimension)?;
        self.files(epoch)?.regions.put_record(key, record)?;
        self.touch(epoch, RegionId::of_key(key).0, now);
        Ok(())
    }

    /// Removes the record of `key`; false when none was stored.
    pub fn remove(&mut self, dimension: Dimension, key: SectionKey) -> Result<bool, StoreError> {
        let epoch = self.writable_epoch(dimension)?;
        Ok(self.files(epoch)?.regions.remove(key)?)
    }

    /// The content hash stored for chunk `(x, z)` in `dimension`'s session epoch, never
    /// read through to a stored epoch, so an undecided session samples, and votes with,
    /// every chunk it is sent.
    pub fn chunk_hash(
        &mut self,
        dimension: Dimension,
        x: i32,
        z: i32,
    ) -> Result<Option<u64>, StoreError> {
        let epoch = self.session_epoch(dimension)?;
        Ok(self.files(epoch)?.regions.chunk_hash(x, z)?)
    }

    /// Stores or clears the content hash of chunk `(x, z)`.
    pub fn set_chunk_hash(
        &mut self,
        dimension: Dimension,
        x: i32,
        z: i32,
        hash: Option<u64>,
    ) -> Result<(), StoreError> {
        let epoch = self.writable_epoch(dimension)?;
        Ok(self.files(epoch)?.regions.set_chunk_hash(x, z, hash)?)
    }

    /// The open files of `epoch`, opening them when needed. Nothing is created on disk
    /// until a write, and a read-only store's own epochs never touch it. Other epochs
    /// close files first so that the one file the caller may open next keeps the store
    /// within [`super::StoreOptions::open_regions`].
    pub(super) fn files(&mut self, epoch: EpochRef) -> Result<&mut EpochFiles, StoreError> {
        self.close_other_files(epoch);
        if !self.epochs.contains_key(&epoch) {
            let dir = epoch.dir(&self.dir);
            let (regions, signatures) = if self.detached.contains(&epoch) {
                (
                    RegionSet::detached(&dir, self.options.open_regions),
                    SignatureCache::detached(dir, self.options.open_signature_files),
                )
            } else {
                let mut regions = RegionSet::open(&dir, self.access, self.options.open_regions)
                    .with_compaction_budget(self.options.compaction_budget_bytes);
                if self.access == Access::ReadWrite {
                    regions = regions.with_barrier(self.registry.clone());
                }
                let signatures =
                    SignatureCache::new(dir, self.access, self.options.open_signature_files);
                (regions, signatures)
            };
            self.epochs.insert(
                epoch,
                EpochFiles {
                    regions,
                    signatures,
                },
            );
        }
        Ok(self.epochs.get_mut(&epoch).expect("inserted above"))
    }

    /// Closes the least recently used files of epochs other than `epoch`, the one holding
    /// the most first, until a new file fits the store's bound or none is left open
    /// outside `epoch`, whose own set keeps itself within the bound.
    fn close_other_files(&mut self, epoch: EpochRef) {
        let limit = self.options.open_regions.get();
        loop {
            let open: usize = self
                .epochs
                .values()
                .map(|files| files.regions.open_files())
                .sum();
            if open < limit {
                return;
            }
            let fullest = self
                .epochs
                .iter_mut()
                .filter(|(other, _)| **other != epoch)
                .max_by_key(|(other, files)| (files.regions.open_files(), **other));
            let closed = fullest.is_some_and(|(_, files)| files.regions.close_least_recent());
            if !closed {
                return;
            }
        }
    }

    /// The epoch `dimension`'s session reads and writes.
    pub(super) fn session_epoch(&self, dimension: Dimension) -> Result<EpochRef, StoreError> {
        self.sessions
            .get(&dimension)
            .map(|session| session.status.epoch())
            .ok_or(StoreError::NoSession(dimension))
    }

    /// [`Self::session_epoch`], failing on a read-only store.
    pub(super) fn writable_epoch(&self, dimension: Dimension) -> Result<EpochRef, StoreError> {
        if self.access == Access::ReadOnly {
            return Err(StoreError::ReadOnly);
        }
        self.session_epoch(dimension)
    }

    /// Records a use of `epoch`'s `region` at `now` in the access table.
    pub(super) fn touch(&mut self, epoch: EpochRef, region: RegionId, now: Minute) {
        self.table_dirty |= self.table.touch(RegionRef { epoch, region }, now);
    }
}

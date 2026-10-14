//! Session epochs: which epoch of a dimension a session reads and writes, and the votes
//! that merge a candidate into a stored epoch or split it off as a new one.
//!
//! A session in a dimension with no established epoch writes a new established epoch at
//! once. Otherwise it writes a candidate of its own and compares each signature region it
//! records against every established epoch of the dimension, one vote per region and
//! epoch. The first epoch to reach [`EpochDecision::Merge`] absorbs the candidate; once
//! every stored epoch has split, the candidate becomes established. A candidate still
//! undecided when its session ends is discarded with its data, so unverified terrain
//! never joins a stored world. Proxies that switch worlds behind a dimension change are
//! handled by the caller beginning a new session after every dimension change.
//!
//! While undecided, a candidate reads section records through to its leading stored
//! epoch, the most recently used one that has not split, so a revisit shows its stored
//! terrain at once. A region whose own vote against that epoch found different terrain
//! stops reading through at once, at every level whose sections overlap it, so a
//! different world shows the stored one's terrain only where it has not voted yet. When
//! the leading epoch splits while another stays undecided, every read-through moves to
//! the next one, which [`Store::leading_epoch_changes`] counts. Chunk content hashes are
//! never read through: a candidate must sample every chunk it is sent, because those
//! samples are its votes. Writers merge into a candidate
//! through own-epoch reads ([`crate::actor::ReadTarget::OwnSection`]), so it holds only
//! terrain it sampled and splits off clean; a merge lays its records over the stored
//! ones.

use std::collections::HashSet;

use lod::{
    epoch::{ChunkSurface, EpochDecision, EpochVote, RegionComparison, RegionPos, compare_region},
    geometry::SectionKey,
};

use super::{Store, StoreEvent, hidden::HiddenRegions, open::remove_dir};
use crate::{
    epochs::{EpochRef, EpochState},
    error::StoreError,
    identity::Dimension,
    minute::Minute,
    region_file::{Access, RegionFile},
    region_id::{REGION_SIDE_CHUNKS, RegionId},
    region_set::RegionSet,
    signatures::SignatureCache,
};

/// Where a session's epoch stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochStatus {
    /// The session writes a candidate of its own while its votes are undecided.
    Candidate(EpochRef),
    /// The session's epoch is established: a first visit or a world unlike every stored
    /// one.
    Established(EpochRef),
    /// The candidate merged into a stored epoch, which the session now uses.
    Merged { from: EpochRef, into: EpochRef },
}

impl EpochStatus {
    /// The epoch the session reads and writes.
    #[must_use]
    pub fn epoch(self) -> EpochRef {
        match self {
            Self::Candidate(epoch) | Self::Established(epoch) => epoch,
            Self::Merged { into, .. } => into,
        }
    }

    /// True while votes are undecided.
    #[must_use]
    pub fn is_candidate(self) -> bool {
        matches!(self, Self::Candidate(_))
    }
}

/// One dimension's session.
#[derive(Debug)]
pub(super) struct Session {
    pub(super) status: EpochStatus,
    votes: Vec<StoredVote>,
    salt: u64,
}

/// A candidate's votes against one stored epoch.
#[derive(Debug)]
struct StoredVote {
    epoch: EpochRef,
    vote: EpochVote,
    /// Regions that already voted or abstained for good.
    settled: HashSet<RegionPos>,
    /// Regions that voted "mismatch": never read through to this epoch.
    hidden: HiddenRegions,
}

impl Store {
    /// Begins a session in `dimension`, ending any earlier one there, and returns its
    /// epoch. Callers begin again after every dimension change, since a proxy may have
    /// switched worlds. `now` seeds the session's column draws and dates the epoch's use.
    pub fn begin_session(
        &mut self,
        dimension: Dimension,
        now: Minute,
    ) -> Result<EpochStatus, StoreError> {
        self.end_session(dimension);
        let stored: Vec<EpochRef> = self.catalog.established(dimension).collect();
        let state = if stored.is_empty() {
            EpochState::Established
        } else {
            EpochState::Candidate
        };
        let epoch = self
            .catalog
            .create(dimension, state, now)
            .ok_or(StoreError::EpochIdsExhausted)?;
        self.catalog_dirty = true;
        if self.access == Access::ReadOnly {
            self.detached.insert(epoch);
        }
        let status = match state {
            EpochState::Established => EpochStatus::Established(epoch),
            EpochState::Candidate => EpochStatus::Candidate(epoch),
        };
        let votes = stored
            .into_iter()
            .map(|epoch| StoredVote {
                epoch,
                vote: EpochVote::new(),
                settled: HashSet::new(),
                hidden: HiddenRegions::default(),
            })
            .collect();
        let salt = u64::from(now.get()) << 32 | u64::from(epoch.id.0);
        self.sessions.insert(
            dimension,
            Session {
                status,
                votes,
                salt,
            },
        );
        self.events.push(StoreEvent::Epoch { dimension, status });
        if state == EpochState::Established {
            self.enforce_epoch_limit();
        }
        Ok(status)
    }

    /// Epochs the open sessions read, write or vote against.
    pub(super) fn epochs_in_use(&self) -> Vec<EpochRef> {
        self.sessions
            .values()
            .flat_map(|session| {
                std::iter::once(session.status.epoch())
                    .chain(session.votes.iter().map(|vote| vote.epoch))
            })
            .collect()
    }

    /// Ends the session in `dimension`, discarding its candidate when undecided.
    pub fn end_session(&mut self, dimension: Dimension) {
        let Some(session) = self.sessions.remove(&dimension) else {
            return;
        };
        if let EpochStatus::Candidate(candidate) = session.status {
            self.discard_epoch(candidate);
            self.events.push(StoreEvent::CandidateDropped(candidate));
        }
    }

    /// The status of `dimension`'s session.
    #[must_use]
    pub fn epoch_status(&self, dimension: Dimension) -> Option<EpochStatus> {
        self.sessions.get(&dimension).map(|session| session.status)
    }

    /// The stored epoch an undecided session reads section records through to: the
    /// most recently used one it has not split from. `None` once the session decided,
    /// or when it has no session.
    #[must_use]
    pub fn leading_epoch(&self, dimension: Dimension) -> Option<EpochRef> {
        self.leading_vote(dimension).map(|vote| vote.epoch)
    }

    /// The epoch an undecided session reads `key` through to: its
    /// [`Self::leading_epoch`], unless a signature region the section overlaps voted
    /// against that epoch.
    #[must_use]
    pub fn read_through_epoch(&self, dimension: Dimension, key: SectionKey) -> Option<EpochRef> {
        self.leading_vote(dimension)
            .filter(|vote| !vote.hidden.hides(key))
            .map(|vote| vote.epoch)
    }

    /// The vote against the leading epoch of `dimension`'s undecided session.
    fn leading_vote(&self, dimension: Dimension) -> Option<&StoredVote> {
        let session = self.sessions.get(&dimension)?;
        if !session.status.is_candidate() {
            return None;
        }
        session
            .votes
            .iter()
            .filter(|vote| vote.vote.decision() != EpochDecision::Split)
            .filter_map(|vote| Some((vote, self.catalog.get(vote.epoch)?)))
            .max_by_key(|(_, entry)| (entry.last_used, entry.epoch.id))
            .map(|(vote, _)| vote)
    }

    /// How many times an undecided session's leading epoch changed since the store opened
    /// without the session deciding: the leading epoch split while another stayed
    /// undecided, so every read-through may answer differently without a write.
    #[must_use]
    pub fn leading_epoch_changes(&self) -> u64 {
        self.leading_changes
    }

    /// How many regions have stopped reading through since the store opened; it moves on
    /// whenever a read-through read of a section overlapping one of them may answer
    /// differently without a write.
    #[must_use]
    pub fn hidden_regions(&self) -> u64 {
        self.hidden_regions
    }

    /// Takes the regions hidden from read-through since the last call, oldest first, with
    /// their dimensions.
    pub fn take_newly_hidden(&mut self) -> Vec<(Dimension, RegionPos)> {
        std::mem::take(&mut self.newly_hidden)
    }

    /// Records a sampled chunk's surface in the session epoch and, while it is a
    /// candidate, lets the chunk's signature region vote. Returns the session's status,
    /// which may have just merged or become established. A read-only store records and
    /// votes in memory only.
    pub fn record_surface(
        &mut self,
        dimension: Dimension,
        chunk_x: i32,
        chunk_z: i32,
        surface: ChunkSurface,
        now: Minute,
    ) -> Result<EpochStatus, StoreError> {
        let epoch = self.session_epoch(dimension)?;
        self.files(epoch)?
            .signatures
            .record(chunk_x, chunk_z, surface);
        let pos = RegionPos::of_chunk(chunk_x, chunk_z);
        if let EpochStatus::Candidate(candidate) = self.sessions[&dimension].status {
            let leading = self.leading_epoch(dimension);
            let voted = self
                .vote(dimension, candidate, pos)
                .and_then(|()| self.settle(dimension, candidate, now));
            if self.sessions[&dimension].status.is_candidate()
                && self.leading_epoch(dimension) != leading
            {
                self.leading_changes += 1;
            }
            voted?;
        }
        Ok(self.sessions[&dimension].status)
    }

    /// Compares `pos` of the candidate with every stored epoch that is still undecided
    /// and has not settled that region.
    fn vote(
        &mut self,
        dimension: Dimension,
        candidate: EpochRef,
        pos: RegionPos,
    ) -> Result<(), StoreError> {
        let Some(fresh) = self.files(candidate)?.signatures.get(pos).cloned() else {
            return Ok(());
        };
        let pending: Vec<(usize, EpochRef)> = self.sessions[&dimension]
            .votes
            .iter()
            .enumerate()
            .filter(|(_, vote)| {
                vote.vote.decision() == EpochDecision::Candidate && !vote.settled.contains(&pos)
            })
            .map(|(index, vote)| (index, vote.epoch))
            .collect();
        for (index, stored_epoch) in pending {
            let stored = self.files(stored_epoch)?.signatures.get(pos).cloned();
            let session = self
                .sessions
                .get_mut(&dimension)
                .expect("session checked by caller");
            let vote = &mut session.votes[index];
            let Some(stored) = stored else {
                // The stored epoch never saw this region; it cannot vote on it.
                vote.settled.insert(pos);
                continue;
            };
            let seed = draw_seed(session.salt, pos, stored_epoch);
            let comparison = compare_region(&stored, &fresh, seed);
            if comparison != RegionComparison::TooFewColumns {
                vote.settled.insert(pos);
                vote.vote.record(comparison);
            }
            if matches!(comparison, RegionComparison::Mismatch { .. }) {
                vote.hidden.hide(pos);
                self.hidden_regions += 1;
                self.newly_hidden.push((dimension, pos));
                self.events.push(StoreEvent::ReadThroughHidden {
                    dimension,
                    region: pos,
                    epoch: stored_epoch,
                });
            }
        }
        Ok(())
    }

    /// Merges or establishes the candidate once its votes allow it.
    fn settle(
        &mut self,
        dimension: Dimension,
        candidate: EpochRef,
        now: Minute,
    ) -> Result<(), StoreError> {
        let votes = &self.sessions[&dimension].votes;
        let merge_into = votes
            .iter()
            .find(|vote| vote.vote.decision() == EpochDecision::Merge)
            .map(|vote| vote.epoch);
        let all_split = votes
            .iter()
            .all(|vote| vote.vote.decision() == EpochDecision::Split);
        let status = if let Some(into) = merge_into {
            self.merge_epoch(candidate, into, now)?;
            EpochStatus::Merged {
                from: candidate,
                into,
            }
        } else if all_split {
            self.catalog.establish(candidate);
            self.catalog.touch(candidate, now);
            self.catalog_dirty = true;
            EpochStatus::Established(candidate)
        } else {
            return Ok(());
        };
        let session = self
            .sessions
            .get_mut(&dimension)
            .expect("session checked by caller");
        session.status = status;
        session.votes.clear();
        self.events.push(StoreEvent::Epoch { dimension, status });
        if matches!(status, EpochStatus::Established(_)) {
            self.enforce_epoch_limit();
        }
        Ok(())
    }

    /// Moves the candidate's records, chunk hashes and signatures into `into`, newer
    /// over older, then deletes the candidate. A record both store is laid over the
    /// stored one column by column ([`lod::section::overlay`]), as the candidate holds
    /// only what it sampled. A read-only store only drops it.
    fn merge_epoch(
        &mut self,
        candidate: EpochRef,
        into: EpochRef,
        now: Minute,
    ) -> Result<(), StoreError> {
        if self.access == Access::ReadWrite {
            let dir = candidate.dir(&self.dir);
            let signatures = match self.epochs.remove(&candidate) {
                Some(mut files) => {
                    files.regions.commit_all();
                    let signatures = files.signatures.all()?;
                    self.events
                        .extend(files.regions.close().into_iter().map(|event| {
                            StoreEvent::Region {
                                epoch: candidate,
                                event,
                            }
                        }));
                    signatures
                }
                None => {
                    SignatureCache::new(&dir, Access::ReadOnly, self.options.open_signature_files)
                        .all()?
                }
            };
            let regions =
                RegionSet::open(&dir, Access::ReadOnly, self.options.open_regions).region_ids()?;
            let target = self.files(into)?;
            for region in regions {
                copy_region(&dir, region, &mut target.regions)?;
            }
            for signature in &signatures {
                target.signatures.merge(signature);
            }
            remove_dir(&dir)?;
        } else {
            self.epochs.remove(&candidate);
            self.detached.remove(&candidate);
        }
        self.catalog.remove(candidate);
        self.catalog.touch(into, now);
        self.catalog_dirty = true;
        self.table.merge_epoch(candidate, into);
        self.table_dirty = true;
        Ok(())
    }

    /// Deletes `epoch`'s files and catalog entry without committing its changes, and
    /// drops any session's votes against it.
    pub(super) fn discard_epoch(&mut self, epoch: EpochRef) {
        self.epochs.remove(&epoch);
        self.detached.remove(&epoch);
        for session in self.sessions.values_mut() {
            session.votes.retain(|vote| vote.epoch != epoch);
        }
        if self.access == Access::ReadWrite
            && let Err(error) = remove_dir(&epoch.dir(&self.dir))
        {
            self.events.push(StoreEvent::Io {
                path: epoch.dir(&self.dir),
                error,
            });
        }
        self.catalog.remove(epoch);
        self.catalog_dirty = true;
        self.table.forget_epoch(epoch);
        self.table_dirty = true;
    }
}

/// Copies every readable record and chunk hash of the region file `region` in `dir`
/// into `target`. Corrupt records are skipped with the hashes of the chunks they cover, so
/// those chunks are sampled again later.
fn copy_region(
    dir: &std::path::Path,
    region: RegionId,
    target: &mut RegionSet,
) -> Result<(), StoreError> {
    let path = dir.join(region.file_name());
    let source = match RegionFile::open_read_only(&path, region) {
        Ok(Some(source)) => source,
        Ok(None) | Err(crate::error::RegionError::Header(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let keys: Vec<_> = source.keys().collect();
    let mut record = Vec::new();
    let mut overlay = Overlay::default();
    let mut unvouched = Vec::new();
    for key in keys {
        match source.get_into(key, &mut record) {
            Ok(true) => {
                let merged = overlay.onto_stored(key, &record, target)?;
                target.put_record(key, merged.unwrap_or(&record[..]))?;
            }
            Ok(false) => {}
            Err(crate::error::RegionError::Corrupt { .. }) => {
                unvouched.extend(RegionId::of_key(key).1.chunk_slots());
            }
            Err(error) => return Err(error.into()),
        }
    }
    if region.level() == 0 {
        for local_z in 0..REGION_SIDE_CHUNKS {
            for local_x in 0..REGION_SIDE_CHUNKS {
                let x = region.x() * REGION_SIDE_CHUNKS + local_x;
                let z = region.z() * REGION_SIDE_CHUNKS + local_z;
                if unvouched.contains(&RegionId::of_chunk(x, z).1) {
                    continue;
                }
                if let Some(hash) = source.chunk_hash(x, z)? {
                    target.set_chunk_hash(x, z, Some(hash))?;
                }
            }
        }
    }
    Ok(())
}

/// Lays a merging candidate's records on the target epoch's, reusing its buffers.
#[derive(Default)]
struct Overlay {
    codec: lod::codec::SectionCodec,
    scratch: lod::storage::PackScratch,
    stored: Vec<u8>,
    merged: Vec<u8>,
}

impl Overlay {
    /// `record` of `key` laid on the record `target` stores there ([`lod::section::overlay`]),
    /// since a candidate never reads its target's records and knows only what it sampled;
    /// `None` keeps `record` as it is, when nothing is stored there or either record does
    /// not decode.
    fn onto_stored(
        &mut self,
        key: lod::geometry::SectionKey,
        record: &[u8],
        target: &mut RegionSet,
    ) -> Result<Option<&[u8]>, StoreError> {
        match target.get_into(key, &mut self.stored) {
            Ok(true) => {}
            Ok(false) | Err(crate::error::RegionError::Corrupt { .. }) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let (Ok(over), Ok(under)) = (
            self.codec.decode_expecting(record, key),
            self.codec.decode_expecting(&self.stored, key),
        ) else {
            return Ok(None);
        };
        let merged = lod::section::overlay(&over, &under, &mut self.scratch);
        self.codec.encode_into(&merged, &mut self.merged);
        Ok(Some(&self.merged))
    }
}

/// The column draw for one region's comparison with one stored epoch: varies per session,
/// region and epoch.
fn draw_seed(salt: u64, pos: RegionPos, epoch: EpochRef) -> u64 {
    let mut value = salt
        ^ (pos.x as u32 as u64) << 32
        ^ u64::from(pos.z as u32)
        ^ u64::from(epoch.id.0).rotate_left(17);
    value = (value ^ value >> 33).wrapping_mul(0xff51_afd7_ed55_8ccd);
    value ^ value >> 33
}

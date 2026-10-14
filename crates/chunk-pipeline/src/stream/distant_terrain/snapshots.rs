//! The store records each mesh reads, cached by the mesher thread within a byte budget and
//! decoded by the mesh job, off the mesher thread.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, atomic::Ordering},
};

use lod::{
    codec::SectionCodec,
    epoch::RegionPos,
    geometry::{Face, MAX_LEVEL, SectionKey, SectionPos},
    mesh::input::{FACE_CHILDREN, FaceInput, FaceSlabs, MeshInput},
    section::Section,
};
use lod_store::{
    actor::{ReadOutcome, ReadTarget, SubmitError},
    identity::Dimension,
};

use super::meshes::{LOD_MESH_SNAPSHOT_BYTES, MeshShared};

/// Bytes counted for each cached entry besides its record or section.
const ENTRY_OVERHEAD_BYTES: usize = 64;

/// One position's store record as the mesher thread caches it.
#[derive(Clone, Debug)]
pub(super) enum Cached {
    /// No record holds a section there, or its record is corrupt.
    Missing,
    /// The record as read; a mesh job decodes it.
    Encoded(Arc<[u8]>),
    /// The section a mesh job decoded from the record.
    Decoded(Arc<Section>),
}

impl Cached {
    /// Bytes this entry holds.
    fn bytes(&self) -> usize {
        ENTRY_OVERHEAD_BYTES
            + match self {
                Self::Missing => 0,
                Self::Encoded(record) => record.len(),
                Self::Decoded(section) => size_of::<Section>() + section.voxels().heap_bytes(),
            }
    }
}

/// A section a mesh job decoded: its key, the record it came from and the section,
/// `None` when the record did not decode.
pub(super) type DecodedSection = (SectionKey, Arc<[u8]>, Option<Arc<Section>>);

/// Why a snapshot could not be built.
#[derive(Debug)]
pub(super) enum SnapshotError {
    /// The store's queue is full; retry after the frame's nudge.
    Full,
    /// A read failed; retry after the frame's nudge, reading the store again.
    Failed,
    /// The store stopped.
    Stopped,
}

/// Records by key, bounded by [`LOD_MESH_SNAPSHOT_BYTES`] with the least recently used
/// dropped first. Failed reads are never cached.
#[derive(Default)]
pub(super) struct SectionSnapshots {
    entries: HashMap<SectionKey, (u64, Cached)>,
    bytes: usize,
    tick: u64,
}

impl SectionSnapshots {
    /// The records at `positions` as `dimension`'s session reads them, in order: cached
    /// ones from memory and the rest in one batched store read. Successful reads are
    /// cached even when another in the batch failed.
    pub(super) fn get_many(
        &mut self,
        shared: &MeshShared,
        dimension: Dimension,
        positions: &[SectionPos],
    ) -> Result<Vec<Cached>, SnapshotError> {
        let found = self.get_many_with(positions, |targets| {
            let count = targets.len() as u64;
            let outcomes = shared.client.read_batch(dimension, targets, shared.now());
            if outcomes.is_ok() {
                shared.section_reads.fetch_add(count, Ordering::Relaxed);
            }
            outcomes
        });
        if matches!(found, Err(SnapshotError::Failed)) {
            shared.read_failures.fetch_add(1, Ordering::Relaxed);
        }
        found
    }

    /// The cached records at `positions`, the rest from one call of `read` with their
    /// section targets; whatever it answers is cached unless the read failed.
    fn get_many_with(
        &mut self,
        positions: &[SectionPos],
        read: impl FnOnce(Vec<ReadTarget>) -> Result<Vec<ReadOutcome>, SubmitError<()>>,
    ) -> Result<Vec<Cached>, SnapshotError> {
        self.tick += 1;
        let mut found: Vec<Option<Cached>> = Vec::with_capacity(positions.len());
        let mut missing: Vec<SectionKey> = Vec::new();
        for pos in positions {
            let key = pos.key();
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.0 = self.tick;
                found.push(Some(entry.1.clone()));
            } else {
                if !missing.contains(&key) {
                    missing.push(key);
                }
                found.push(None);
            }
        }
        if missing.is_empty() {
            return Ok(found.into_iter().flatten().collect());
        }
        let targets = missing
            .iter()
            .map(|&key| ReadTarget::Section(key))
            .collect();
        let outcomes = match read(targets) {
            Ok(outcomes) => outcomes,
            Err(SubmitError::Full(())) => return Err(SnapshotError::Full),
            Err(SubmitError::Stopped(())) => return Err(SnapshotError::Stopped),
        };
        let mut failed = outcomes.len() != missing.len();
        let mut read_back: Vec<(SectionKey, Cached)> = Vec::with_capacity(missing.len());
        for (key, outcome) in missing.into_iter().zip(outcomes) {
            let cached = match outcome {
                ReadOutcome::Found(record) => Cached::Encoded(record.into()),
                ReadOutcome::Missing | ReadOutcome::Corrupt => Cached::Missing,
                ReadOutcome::ChunkHash(_) | ReadOutcome::Failed(_) => {
                    failed = true;
                    continue;
                }
            };
            self.insert(key, cached.clone());
            read_back.push((key, cached));
        }
        if failed {
            return Err(SnapshotError::Failed);
        }
        Ok(positions
            .iter()
            .zip(found)
            .map(|(pos, cached)| {
                cached.unwrap_or_else(|| {
                    let key = pos.key();
                    read_back
                        .iter()
                        .find(|(read, _)| *read == key)
                        .map(|(_, cached)| cached.clone())
                        .expect("every uncached position was read")
                })
            })
            .collect())
    }

    /// Replaces the record a job decoded with its section, unless the entry changed since.
    pub(super) fn decoded(&mut self, (key, record, section): DecodedSection) {
        let Some((tick, Cached::Encoded(cached))) = self.entries.get(&key) else {
            return;
        };
        if !Arc::ptr_eq(cached, &record) {
            return;
        }
        let tick = *tick;
        let section = section.map_or(Cached::Missing, Cached::Decoded);
        self.remove(key);
        self.bytes += section.bytes();
        self.entries.insert(key, (tick, section));
        self.trim();
    }

    /// Caches `cached` for `key`, dropping the least recently used beyond the budget.
    fn insert(&mut self, key: SectionKey, cached: Cached) {
        self.remove(key);
        self.bytes += cached.bytes();
        self.entries.insert(key, (self.tick, cached));
        self.trim();
    }

    /// Drops the least recently used entries until three quarters of the budget are left,
    /// once the budget is exceeded.
    fn trim(&mut self) {
        if self.bytes <= LOD_MESH_SNAPSHOT_BYTES {
            return;
        }
        let mut by_age: Vec<(u64, SectionKey)> = self
            .entries
            .iter()
            .map(|(&key, (tick, _))| (*tick, key))
            .collect();
        by_age.sort_unstable();
        for (_, key) in by_age {
            if self.bytes <= LOD_MESH_SNAPSHOT_BYTES / 4 * 3 {
                break;
            }
            self.remove(key);
        }
    }

    /// Forgets `key`, so it is read again.
    pub(super) fn remove(&mut self, key: SectionKey) {
        if let Some((_, cached)) = self.entries.remove(&key) {
            self.bytes -= cached.bytes();
        }
    }

    /// Forgets every section overlapping `columns`.
    pub(super) fn forget_columns(&mut self, columns: &HiddenColumns) {
        let keys: Vec<SectionKey> = self
            .entries
            .keys()
            .filter(|key| columns.covers(key.pos()))
            .copied()
            .collect();
        for key in keys {
            self.remove(key);
        }
    }

    /// Forgets every section.
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    /// Reads the section at `pos` and every section its mesh reads, the uncached ones in
    /// one batched store read; `None` when no record holds the section itself.
    pub(super) fn snapshot(
        &mut self,
        shared: &MeshShared,
        dimension: Dimension,
        pos: SectionPos,
    ) -> Result<Option<MeshSnapshot>, SnapshotError> {
        let slabs = Face::ALL.map(|face| FaceSlabs::of(pos, face));
        let mut positions = vec![pos];
        for face in &slabs {
            positions.extend(face.same);
            positions.extend(face.coarser);
            positions.extend(face.finer.iter().flatten());
        }
        let mut records = self
            .get_many(shared, dimension, &positions)?
            .into_iter()
            .zip(positions);
        let (section, _) = records.next().expect("the section itself");
        if matches!(section, Cached::Missing) {
            return Ok(None);
        }
        let mut take = |at: Option<SectionPos>| {
            at.map(|_| {
                let (cached, at) = records.next().expect("one record per position");
                (at, cached)
            })
        };
        let faces = slabs.map(|face| FaceRecords {
            same: take(face.same),
            coarser: take(face.coarser),
            finer: face.finer.map(&mut take),
        });
        Ok(Some(MeshSnapshot {
            pos,
            section,
            faces,
        }))
    }
}

/// The section columns of every level that overlap some hidden regions.
pub(super) struct HiddenColumns(HashSet<(u8, i64, i64)>);

impl HiddenColumns {
    /// The columns overlapping `regions`.
    pub(super) fn of(regions: impl IntoIterator<Item = RegionPos>) -> Self {
        let mut columns = HashSet::new();
        for region in regions {
            for level in 0..=MAX_LEVEL {
                let [xs, zs] = region.section_columns(level);
                for x in xs {
                    for z in zs.clone() {
                        columns.insert((level, x, z));
                    }
                }
            }
        }
        Self(columns)
    }

    /// True when the section at `pos` overlaps a hidden region.
    pub(super) fn covers(&self, pos: SectionPos) -> bool {
        self.0
            .contains(&(pos.level(), i64::from(pos.x()), i64::from(pos.z())))
    }
}

/// The records across one face of a snapshot, with their positions.
#[derive(Default)]
struct FaceRecords {
    same: Option<(SectionPos, Cached)>,
    coarser: Option<(SectionPos, Cached)>,
    finer: [Option<(SectionPos, Cached)>; FACE_CHILDREN],
}

/// The records one mesh reads, owned so the job outlives the thread's cache.
pub(super) struct MeshSnapshot {
    pos: SectionPos,
    section: Cached,
    faces: [FaceRecords; 6],
}

/// The sections across one face, decoded.
#[derive(Default)]
struct FaceSections {
    same: Option<Arc<Section>>,
    coarser: Option<Arc<Section>>,
    finer: [Option<Arc<Section>>; FACE_CHILDREN],
}

/// A snapshot's sections, decoded on the mesh job.
pub(super) struct DecodedSnapshot {
    section: Arc<Section>,
    faces: [FaceSections; 6],
}

impl MeshSnapshot {
    /// Decodes every record not decoded yet with `codec`, appending what it decoded to
    /// `decoded` for the thread's cache; `None` when the section's own record does not
    /// decode. Records that do not decode read as missing.
    pub(super) fn decode(
        self,
        codec: &mut SectionCodec,
        decoded: &mut Vec<DecodedSection>,
    ) -> Option<DecodedSnapshot> {
        let mut resolve = |pos: SectionPos, cached: Cached| match cached {
            Cached::Missing => None,
            Cached::Decoded(section) => Some(section),
            Cached::Encoded(record) => {
                let key = pos.key();
                let section = codec.decode_expecting(&record, key).ok().map(Arc::new);
                decoded.push((key, record, section.clone()));
                section
            }
        };
        let section = resolve(self.pos, self.section);
        let mut faces: [FaceSections; 6] = Default::default();
        for (records, sections) in self.faces.into_iter().zip(&mut faces) {
            let mut resolve_slot =
                |slot: Option<(SectionPos, Cached)>| slot.and_then(|(at, c)| resolve(at, c));
            sections.same = resolve_slot(records.same);
            sections.coarser = resolve_slot(records.coarser);
            for (out, slot) in sections.finer.iter_mut().zip(records.finer) {
                *out = resolve_slot(slot);
            }
        }
        Some(DecodedSnapshot {
            section: section?,
            faces,
        })
    }
}

impl DecodedSnapshot {
    /// The mesher's view of this snapshot.
    pub(super) fn input(&self) -> MeshInput<'_> {
        let faces = std::array::from_fn(|index| {
            let face = &self.faces[index];
            FaceInput {
                same: face.same.as_deref(),
                coarser: face.coarser.as_deref(),
                finer: std::array::from_fn(|quadrant| face.finer[quadrant].as_deref()),
            }
        });
        MeshInput::new(&self.section, faces)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: i32) -> SectionPos {
        SectionPos::new(0, x, 0, 1).expect("pos")
    }

    #[test]
    fn a_failed_read_is_not_cached_and_the_next_get_reads_again() {
        let mut snapshots = SectionSnapshots::default();
        let positions = [pos(1), pos(2)];
        let failed = snapshots.get_many_with(&positions, |targets| {
            assert_eq!(targets.len(), 2);
            Ok(vec![
                ReadOutcome::Found(vec![1]),
                ReadOutcome::Failed("disk".into()),
            ])
        });
        assert!(matches!(failed, Err(SnapshotError::Failed)));
        let found = snapshots.get_many_with(&positions, |targets| {
            assert_eq!(
                targets,
                [ReadTarget::Section(pos(2).key())],
                "only the failed read is read again"
            );
            Ok(vec![ReadOutcome::Found(vec![2, 3])])
        });
        let found = found.unwrap_or_else(|_| panic!("both records"));
        let [Cached::Encoded(first), Cached::Encoded(second)] = found.as_slice() else {
            panic!("both records");
        };
        assert_eq!((&**first, &**second), (&[1][..], &[2, 3][..]));
        let cached = snapshots.get_many_with(&positions, |_| panic!("cached reads stay in memory"));
        assert!(matches!(
            cached.as_deref(),
            Ok([Cached::Encoded(_), Cached::Encoded(_)])
        ));
    }

    #[test]
    fn repeated_positions_are_read_once_and_answered_in_order() {
        let mut snapshots = SectionSnapshots::default();
        let positions = [pos(1), pos(2), pos(1)];
        let found = snapshots
            .get_many_with(&positions, |targets| {
                assert_eq!(targets.len(), 2);
                Ok(vec![ReadOutcome::Found(vec![1]), ReadOutcome::Missing])
            })
            .expect("read");
        assert!(matches!(
            found.as_slice(),
            [Cached::Encoded(a), Cached::Missing, Cached::Encoded(b)] if **a == [1] && **b == [1]
        ));
    }

    #[test]
    fn decoded_sections_replace_only_the_record_they_came_from() {
        let mut snapshots = SectionSnapshots::default();
        let key = pos(1).key();
        let read = |snapshots: &mut SectionSnapshots, record: Vec<u8>| match snapshots
            .get_many_with(&[pos(1)], |_| Ok(vec![ReadOutcome::Found(record)]))
        {
            Ok(mut found) => found.remove(0),
            Err(_) => panic!("a record"),
        };
        let Cached::Encoded(first) = read(&mut snapshots, vec![1]) else {
            panic!("a record");
        };
        // A write made the thread read the section again before the job's result arrived.
        snapshots.remove(key);
        let _ = read(&mut snapshots, vec![2]);
        snapshots.decoded((key, first, None));
        assert!(matches!(
            snapshots.get_many_with(&[pos(1)], |_| panic!("cached")).as_deref(),
            Ok([Cached::Encoded(record)]) if **record == [2]
        ));
    }
}

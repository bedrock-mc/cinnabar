//! A region file's index: where each section record lives and each chunk's content hash.
//!
//! Index frame payload, little-endian:
//!
//! | Bytes | Field |
//! | --- | --- |
//! | 0 | 0 for a full index, 1 for a delta over the previous index frame |
//! | 1..8 | reserved, zero |
//! | 8..16 | file offset of the previous index frame for a delta, else zero |
//! | 16..20 | record entry count |
//! | 20..24 | hash entry count |
//!
//! Then each record entry as slot `u32`, frame offset `u64` and payload length `u32`, where
//! a delta removes a slot with offset and length zero; then each hash entry as slot `u16`,
//! a present flag `u8`, a zero byte and the hash `u64`, where a delta clears a slot with
//! the flag zero. A full index holds no removals; neither kind repeats a slot.

use std::collections::{HashMap, HashSet};

use foldhash::fast::FixedState;

use crate::region_id::{ChunkSlot, REGION_CHUNKS, REGION_SLOTS, SectionSlot};

const PREAMBLE_LEN: usize = 24;
const RECORD_ENTRY_LEN: usize = 16;
const HASH_ENTRY_LEN: usize = 12;
const FULL: u8 = 0;
const DELTA: u8 = 1;
/// Largest index payload: every section slot and every chunk hash.
pub const MAX_INDEX_LEN: usize = PREAMBLE_LEN
    + REGION_SLOTS as usize * RECORD_ENTRY_LEN
    + REGION_CHUNKS as usize * HASH_ENTRY_LEN;

/// Where one live record frame sits in its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordEntry {
    /// Offset of the frame header.
    pub offset: u64,
    /// Length of the frame payload.
    pub len: u32,
}

/// Why an index payload was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndexError {
    #[error("index payload has {actual} bytes; its counts need {expected}")]
    Length { expected: usize, actual: usize },
    #[error("index kind {0} is unknown")]
    Kind(u8),
    #[error("index reserved bytes are not zero")]
    Reserved,
    #[error("full index names a previous frame at {0}")]
    FullWithPrevious(u64),
    #[error("index entry names slot {0}, past the region")]
    Slot(u64),
    #[error("index entry repeats slot {0}")]
    Duplicate(u64),
    #[error("full index removes slot {0}")]
    FullRemoval(u64),
    #[error("index entry for slot {0} has a malformed record location or hash flag")]
    Entry(u64),
    #[error("index holds chunk hashes but its region is not level 0")]
    HashesAboveLevelZero,
}

/// One decoded index frame, applied to a [`RegionIndex`] in commit order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexFrame {
    /// The previous index frame's offset for a delta; `None` for a full index.
    pub previous: Option<u64>,
    /// Record locations, `None` for a removal.
    pub records: Vec<(SectionSlot, Option<RecordEntry>)>,
    /// Chunk hashes, `None` for a cleared slot.
    pub hashes: Vec<(ChunkSlot, Option<u64>)>,
}

impl IndexFrame {
    /// Decodes and validates a payload; `hashes_allowed` is false above level 0.
    pub fn decode(payload: &[u8], hashes_allowed: bool) -> Result<Self, IndexError> {
        let length_error = |expected| IndexError::Length {
            expected,
            actual: payload.len(),
        };
        if payload.len() < PREAMBLE_LEN {
            return Err(length_error(PREAMBLE_LEN));
        }
        let u32_at = |at: usize| u32::from_le_bytes(payload[at..at + 4].try_into().expect("4"));
        let u64_at = |at: usize| u64::from_le_bytes(payload[at..at + 8].try_into().expect("8"));
        let delta = match payload[0] {
            FULL => false,
            DELTA => true,
            kind => return Err(IndexError::Kind(kind)),
        };
        if payload[1..8] != [0; 7] {
            return Err(IndexError::Reserved);
        }
        let previous = u64_at(8);
        if !delta && previous != 0 {
            return Err(IndexError::FullWithPrevious(previous));
        }
        let record_count = u32_at(16) as usize;
        let hash_count = u32_at(20) as usize;
        if record_count > REGION_SLOTS as usize || hash_count > REGION_CHUNKS as usize {
            return Err(length_error(MAX_INDEX_LEN));
        }
        let expected = PREAMBLE_LEN + record_count * RECORD_ENTRY_LEN + hash_count * HASH_ENTRY_LEN;
        if payload.len() != expected {
            return Err(length_error(expected));
        }
        if hash_count > 0 && !hashes_allowed {
            return Err(IndexError::HashesAboveLevelZero);
        }

        let mut seen = HashSet::with_capacity_and_hasher(record_count, FixedState::default());
        let mut records = Vec::with_capacity(record_count);
        for entry in payload[PREAMBLE_LEN..][..record_count * RECORD_ENTRY_LEN]
            .as_chunks::<RECORD_ENTRY_LEN>()
            .0
        {
            let raw = u32::from_le_bytes(entry[0..4].try_into().expect("4"));
            let offset = u64::from_le_bytes(entry[4..12].try_into().expect("8"));
            let len = u32::from_le_bytes(entry[12..16].try_into().expect("4"));
            let slot = SectionSlot::from_raw(raw).ok_or(IndexError::Slot(raw.into()))?;
            if !seen.insert(slot) {
                return Err(IndexError::Duplicate(raw.into()));
            }
            let location = match (offset, len) {
                (0, 0) if delta => None,
                (0, 0) => return Err(IndexError::FullRemoval(raw.into())),
                (0, _) | (_, 0) => return Err(IndexError::Entry(raw.into())),
                _ => Some(RecordEntry { offset, len }),
            };
            records.push((slot, location));
        }

        let mut seen = HashSet::with_capacity_and_hasher(hash_count, FixedState::default());
        let mut hashes = Vec::with_capacity(hash_count);
        let hash_start = PREAMBLE_LEN + record_count * RECORD_ENTRY_LEN;
        for entry in payload[hash_start..].as_chunks::<HASH_ENTRY_LEN>().0 {
            let raw = u16::from_le_bytes(entry[0..2].try_into().expect("2"));
            let slot = ChunkSlot::from_raw(raw).ok_or(IndexError::Slot(raw.into()))?;
            if !seen.insert(slot) {
                return Err(IndexError::Duplicate(raw.into()));
            }
            let hash = u64::from_le_bytes(entry[4..12].try_into().expect("8"));
            let value = match (entry[2], entry[3]) {
                (1, 0) => Some(hash),
                (0, 0) if delta && hash == 0 => None,
                (0, 0) if !delta => return Err(IndexError::FullRemoval(raw.into())),
                _ => return Err(IndexError::Entry(raw.into())),
            };
            hashes.push((slot, value));
        }
        Ok(Self {
            previous: delta.then_some(previous),
            records,
            hashes,
        })
    }
}

/// The live index of one region file and the slots changed since its last commit.
#[derive(Debug, Clone, Default)]
pub struct RegionIndex {
    records: HashMap<SectionSlot, RecordEntry, FixedState>,
    hashes: HashMap<ChunkSlot, u64, FixedState>,
    dirty_records: HashSet<SectionSlot, FixedState>,
    dirty_hashes: HashSet<ChunkSlot, FixedState>,
}

impl RegionIndex {
    /// The location of the record in `slot`.
    #[must_use]
    pub fn record(&self, slot: SectionSlot) -> Option<RecordEntry> {
        self.records.get(&slot).copied()
    }

    /// Every live record, in no particular order.
    pub fn records(&self) -> impl Iterator<Item = (SectionSlot, RecordEntry)> + '_ {
        self.records.iter().map(|(&slot, &entry)| (slot, entry))
    }

    /// Number of live records.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    /// The content hash stored for `slot`.
    #[must_use]
    pub fn hash(&self, slot: ChunkSlot) -> Option<u64> {
        self.hashes.get(&slot).copied()
    }

    /// Records `entry` for `slot`, returning the entry it replaces.
    pub fn set_record(&mut self, slot: SectionSlot, entry: RecordEntry) -> Option<RecordEntry> {
        self.dirty_records.insert(slot);
        self.records.insert(slot, entry)
    }

    /// Removes `slot`, returning its entry.
    pub fn remove_record(&mut self, slot: SectionSlot) -> Option<RecordEntry> {
        let old = self.records.remove(&slot);
        if old.is_some() {
            self.dirty_records.insert(slot);
        }
        old
    }

    /// Every stored chunk hash, in no particular order.
    pub fn hashes(&self) -> impl Iterator<Item = (ChunkSlot, u64)> + '_ {
        self.hashes.iter().map(|(&slot, &hash)| (slot, hash))
    }

    /// Sets or clears the hash of `slot`.
    pub fn set_hash(&mut self, slot: ChunkSlot, hash: Option<u64>) {
        let old = match hash {
            Some(hash) => self.hashes.insert(slot, hash),
            None => self.hashes.remove(&slot),
        };
        if old != hash {
            self.dirty_hashes.insert(slot);
        }
    }

    /// True when a slot changed since the last [`Self::mark_clean`].
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.dirty_records.is_empty() || !self.dirty_hashes.is_empty()
    }

    /// Forgets which slots changed, after a commit.
    pub fn mark_clean(&mut self) {
        self.dirty_records.clear();
        self.dirty_hashes.clear();
    }

    /// Applies a decoded frame. A full frame replaces the whole index.
    pub fn apply(&mut self, frame: &IndexFrame) {
        if frame.previous.is_none() {
            self.records.clear();
            self.hashes.clear();
        }
        for &(slot, entry) in &frame.records {
            match entry {
                Some(entry) => self.records.insert(slot, entry),
                None => self.records.remove(&slot),
            };
        }
        self.apply_hashes(frame);
    }

    /// Applies only a frame's hash entries, as a rebuilding scan does.
    pub fn apply_hashes(&mut self, frame: &IndexFrame) {
        if frame.previous.is_none() {
            self.hashes.clear();
        }
        for &(slot, hash) in &frame.hashes {
            match hash {
                Some(hash) => self.hashes.insert(slot, hash),
                None => self.hashes.remove(&slot),
            };
        }
    }

    /// Byte length of the delta [`Self::encode`] would write now.
    #[must_use]
    pub fn delta_len(&self) -> usize {
        PREAMBLE_LEN
            + self.dirty_records.len() * RECORD_ENTRY_LEN
            + self.dirty_hashes.len() * HASH_ENTRY_LEN
    }

    /// Replaces `out` with a full index, or with a delta of the changed slots over the
    /// index frame at `previous`. Entries are sorted by slot.
    pub fn encode(&self, previous: Option<u64>, out: &mut Vec<u8>) {
        let mut records: Vec<(SectionSlot, Option<RecordEntry>)> = match previous {
            None => self.records.iter().map(|(&s, &e)| (s, Some(e))).collect(),
            Some(_) => self
                .dirty_records
                .iter()
                .map(|&slot| (slot, self.record(slot)))
                .collect(),
        };
        records.sort_unstable_by_key(|&(slot, _)| slot);
        let mut hashes: Vec<(ChunkSlot, Option<u64>)> = match previous {
            None => self.hashes.iter().map(|(&s, &h)| (s, Some(h))).collect(),
            Some(_) => self
                .dirty_hashes
                .iter()
                .map(|&slot| (slot, self.hash(slot)))
                .collect(),
        };
        hashes.sort_unstable_by_key(|&(slot, _)| slot);

        out.clear();
        out.push(if previous.is_some() { DELTA } else { FULL });
        out.extend_from_slice(&[0; 7]);
        out.extend_from_slice(&previous.unwrap_or(0).to_le_bytes());
        out.extend_from_slice(&(records.len() as u32).to_le_bytes());
        out.extend_from_slice(&(hashes.len() as u32).to_le_bytes());
        for (slot, entry) in records {
            let entry = entry.unwrap_or(RecordEntry { offset: 0, len: 0 });
            out.extend_from_slice(&slot.raw().to_le_bytes());
            out.extend_from_slice(&entry.offset.to_le_bytes());
            out.extend_from_slice(&entry.len.to_le_bytes());
        }
        for (slot, hash) in hashes {
            out.extend_from_slice(&slot.raw().to_le_bytes());
            out.extend_from_slice(&[u8::from(hash.is_some()), 0]);
            out.extend_from_slice(&hash.unwrap_or(0).to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(raw: u32) -> SectionSlot {
        SectionSlot::from_raw(raw).expect("slot")
    }

    fn chunk(raw: u16) -> ChunkSlot {
        ChunkSlot::from_raw(raw).expect("chunk")
    }

    fn entry(offset: u64) -> RecordEntry {
        RecordEntry { offset, len: 40 }
    }

    #[test]
    fn full_and_delta_frames_rebuild_the_same_index() {
        let mut index = RegionIndex::default();
        index.set_record(slot(1), entry(100));
        index.set_record(slot(2), entry(200));
        index.set_hash(chunk(3), Some(33));
        let mut full = Vec::new();
        index.encode(None, &mut full);
        index.mark_clean();

        index.remove_record(slot(1));
        index.set_record(slot(5), entry(500));
        index.set_hash(chunk(3), None);
        index.set_hash(chunk(4), Some(44));
        let mut delta = Vec::new();
        index.encode(Some(64), &mut delta);
        assert_eq!(delta.len(), index.delta_len());

        let mut rebuilt = RegionIndex::default();
        rebuilt.apply(&IndexFrame::decode(&full, true).expect("full"));
        let delta = IndexFrame::decode(&delta, true).expect("delta");
        assert_eq!(delta.previous, Some(64));
        rebuilt.apply(&delta);
        let mut expected: Vec<_> = index.records().collect();
        let mut actual: Vec<_> = rebuilt.records().collect();
        expected.sort_unstable_by_key(|&(slot, _)| slot);
        actual.sort_unstable_by_key(|&(slot, _)| slot);
        assert_eq!(actual, expected);
        assert_eq!(rebuilt.hash(chunk(3)), None);
        assert_eq!(rebuilt.hash(chunk(4)), Some(44));
    }

    #[test]
    fn setting_an_equal_hash_leaves_the_index_clean() {
        let mut index = RegionIndex::default();
        index.set_hash(chunk(1), Some(7));
        index.mark_clean();
        index.set_hash(chunk(1), Some(7));
        index.set_hash(chunk(2), None);
        assert!(!index.is_dirty());
    }

    #[test]
    fn malformed_payloads_are_rejected() {
        let mut index = RegionIndex::default();
        index.set_record(slot(1), entry(100));
        index.set_hash(chunk(1), Some(1));
        let mut good = Vec::new();
        index.encode(None, &mut good);
        let decode = |edit: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = good.clone();
            edit(&mut bytes);
            IndexFrame::decode(&bytes, true)
        };
        assert_eq!(decode(&|b| b[0] = 7), Err(IndexError::Kind(7)));
        assert_eq!(decode(&|b| b[3] = 1), Err(IndexError::Reserved));
        assert_eq!(decode(&|b| b[8] = 1), Err(IndexError::FullWithPrevious(1)));
        assert!(matches!(
            decode(&|b| b.truncate(30)),
            Err(IndexError::Length { .. })
        ));
        assert!(matches!(
            decode(&|b| b[16..20].copy_from_slice(&u32::MAX.to_le_bytes())),
            Err(IndexError::Length { .. })
        ));
        assert_eq!(
            decode(&|b| b[24..28].copy_from_slice(&REGION_SLOTS.to_le_bytes())),
            Err(IndexError::Slot(REGION_SLOTS.into()))
        );
        assert_eq!(
            decode(&|b| b[28..36].copy_from_slice(&0u64.to_le_bytes())),
            Err(IndexError::Entry(1))
        );
        assert_eq!(decode(&|b| b[42] = 0), Err(IndexError::FullRemoval(1)));
        assert_eq!(decode(&|b| b[42] = 2), Err(IndexError::Entry(1)));
        assert_eq!(
            IndexFrame::decode(&good, false),
            Err(IndexError::HashesAboveLevelZero)
        );
    }

    #[test]
    fn repeated_slots_are_rejected() {
        let mut bytes = vec![FULL, 0, 0, 0, 0, 0, 0, 0];
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for _ in 0..2 {
            bytes.extend_from_slice(&9u32.to_le_bytes());
            bytes.extend_from_slice(&64u64.to_le_bytes());
            bytes.extend_from_slice(&30u32.to_le_bytes());
        }
        assert_eq!(
            IndexFrame::decode(&bytes, true),
            Err(IndexError::Duplicate(9))
        );
    }
}

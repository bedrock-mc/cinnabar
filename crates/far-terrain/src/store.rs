//! Compressed in-memory summaries with least-recently-used eviction under a byte budget.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::{ColumnSummary, EncodedSummary, SummaryDecodeError};

/// Chunk column coordinates inside one far-terrain namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ColumnPos {
    pub x: i32,
    pub z: i32,
}

impl ColumnPos {
    #[must_use]
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

/// What admitting a summary did to the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Inserted,
    Replaced,
    /// The stored summary already had this content; only its recency and version changed.
    Unchanged,
    /// The stored summary is at least as new; nothing changed.
    Stale,
}

/// A summary with the version it was taken at; of two, the higher version is newer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Versioned {
    pub version: u64,
    pub summary: EncodedSummary,
}

#[derive(Debug)]
struct Entry {
    summary: EncodedSummary,
    version: u64,
    stamp: u64,
}

/// Compressed summaries of one namespace; content changes are queued for consumers.
#[derive(Debug)]
pub struct SummaryStore {
    entries: HashMap<ColumnPos, Entry>,
    recency: BTreeMap<u64, ColumnPos>,
    next_stamp: u64,
    bytes: usize,
    budget_bytes: usize,
    /// Bounded by the entries: an evicted position leaves the set.
    changed: BTreeSet<ColumnPos>,
}

impl SummaryStore {
    #[must_use]
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            recency: BTreeMap::new(),
            next_stamp: 0,
            bytes: 0,
            budget_bytes,
            changed: BTreeSet::new(),
        }
    }

    /// Stores `summary` as newer than anything stored; equal content only refreshes recency.
    pub fn admit(&mut self, pos: ColumnPos, summary: &ColumnSummary) -> Admission {
        let version = self.version(pos).map_or(0, |version| version + 1);
        if let Some(entry) = self.entries.get_mut(&pos)
            && entry.summary.hash == summary.content_hash()
        {
            entry.version = version;
            self.touch(pos);
            return Admission::Unchanged;
        }
        self.admit_versioned(
            pos,
            Versioned {
                version,
                summary: summary.encode(),
            },
        )
    }

    /// Stores `summary` unless the stored one is at least as new.
    pub fn admit_versioned(&mut self, pos: ColumnPos, summary: Versioned) -> Admission {
        if self
            .version(pos)
            .is_some_and(|stored| stored >= summary.version)
        {
            return Admission::Stale;
        }
        let Versioned { version, summary } = summary;
        let stamp = self.stamp();
        let bytes = entry_bytes(&summary);
        let hash = summary.hash;
        let entry = Entry {
            summary,
            version,
            stamp,
        };
        let admission = match self.entries.insert(pos, entry) {
            Some(old) => {
                self.retire(&old);
                if old.summary.hash == hash {
                    Admission::Unchanged
                } else {
                    Admission::Replaced
                }
            }
            None => Admission::Inserted,
        };
        self.recency.insert(stamp, pos);
        self.bytes += bytes;
        if admission != Admission::Unchanged {
            self.changed.insert(pos);
        }
        self.evict_to_budget();
        admission
    }

    /// Decodes the stored summary and marks it recently used.
    pub fn summary(&mut self, pos: ColumnPos) -> Option<Result<ColumnSummary, SummaryDecodeError>> {
        self.touch(pos)?;
        Some(ColumnSummary::decode(&self.entries[&pos].summary.bytes))
    }

    /// The stored summary's version, without changing recency.
    #[must_use]
    pub fn version(&self, pos: ColumnPos) -> Option<u64> {
        self.entries.get(&pos).map(|entry| entry.version)
    }

    /// The stored encoding, without changing recency.
    #[must_use]
    pub fn encoded(&self, pos: ColumnPos) -> Option<&EncodedSummary> {
        self.entries.get(&pos).map(|entry| &entry.summary)
    }

    #[must_use]
    pub fn contains(&self, pos: ColumnPos) -> bool {
        self.entries.contains_key(&pos)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Approximate resident bytes, including per-entry overhead.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    /// Resident positions whose content was inserted or replaced since the last call.
    pub fn take_changed(&mut self) -> Vec<ColumnPos> {
        std::mem::take(&mut self.changed).into_iter().collect()
    }

    /// Changes the byte budget, evicting the least recently used at once if it shrank.
    pub fn set_budget(&mut self, budget_bytes: usize) {
        self.budget_bytes = budget_bytes;
        self.evict_to_budget();
    }

    /// Drops every summary, as when the namespace changes.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.recency.clear();
        self.bytes = 0;
        self.changed.clear();
    }

    fn touch(&mut self, pos: ColumnPos) -> Option<()> {
        let stamp = self.stamp();
        let entry = self.entries.get_mut(&pos)?;
        self.recency.remove(&entry.stamp);
        entry.stamp = stamp;
        self.recency.insert(stamp, pos);
        Some(())
    }

    fn retire(&mut self, old: &Entry) {
        self.recency.remove(&old.stamp);
        self.bytes -= entry_bytes(&old.summary);
    }

    fn stamp(&mut self) -> u64 {
        self.next_stamp += 1;
        self.next_stamp
    }

    fn evict_to_budget(&mut self) {
        while self.bytes > self.budget_bytes {
            let Some((_, pos)) = self.recency.pop_first() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&pos) {
                self.bytes -= entry_bytes(&entry.summary);
            }
            self.changed.remove(&pos);
        }
    }
}

/// Bytes charged for one entry: its encoding plus map and recency bookkeeping.
fn entry_bytes(summary: &EncodedSummary) -> usize {
    summary.bytes.len()
        + std::mem::size_of::<(ColumnPos, Entry)>()
        + std::mem::size_of::<(u64, ColumnPos)>()
}

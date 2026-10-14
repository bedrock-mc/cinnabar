//! What ingestion knows about each column: the newest capture a writer accepted, and the
//! content hash the store holds or is about to hold for it.
//!
//! Captures finish in any order, so every capture carries a sequence number taken when it
//! was captured. The column's writer, the book's only writer of its hash, accepts a capture
//! only when no newer one of the column was accepted or skipped. A sample job, or the frame for a
//! column whose hash it already knows, may skip the column when its hash equals the
//! book's, which also marks its sequence as seen.
//!
//! Each hash is stamped with the ingestion generation it was learned or accepted in
//! ([`super::shared::LodShared::generation`]), which moves on with every store session
//! change and failed request. A hash of an older generation reads as unknown, so a column
//! is never skipped for content another session epoch holds.

use hashbrown::HashMap;

use lod_store::identity::Dimension;
use world::ChunkKey;

use super::MAX_LOD_BOOK_COLUMNS;

/// The store's content hash of a column, as far as the book knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) enum StoredHash {
    /// Not asked yet, or forgotten after a write was lost.
    Unknown,
    /// The store holds no hash for the column.
    Missing,
    /// The hash of the newest content written or queued for the column.
    Hash(u64),
}

/// One column's entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    /// The newest capture accepted or skipped.
    seq: u64,
    stored: StoredHash,
    /// The generation `stored` belongs to.
    generation: u64,
}

/// How a capture compares with what the book knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) enum Verdict {
    /// A newer capture of the column was accepted or skipped; drop this one.
    Superseded,
    /// The store holds or is about to hold this content; this capture is now the newest.
    Unchanged,
    /// The store's hash is unknown; ask it first.
    Unknown,
    /// The content differs from the stored one.
    Changed,
}

/// Columns by dimension and chunk; see the module documentation.
#[derive(Debug, Default)]
pub(in crate::stream) struct ColumnBook {
    entries: HashMap<(Dimension, ChunkKey), Entry>,
}

impl ColumnBook {
    /// Compares capture `seq` of `column` with content `hash` against the book in
    /// `generation`, marking `seq` as the newest when the content is unchanged.
    pub(in crate::stream) fn check(
        &mut self,
        column: (Dimension, ChunkKey),
        seq: u64,
        hash: u64,
        generation: u64,
    ) -> Verdict {
        let Some(entry) = self.entries.get_mut(&column) else {
            return Verdict::Unknown;
        };
        if seq < entry.seq {
            return Verdict::Superseded;
        }
        if entry.generation != generation {
            return Verdict::Unknown;
        }
        match entry.stored {
            StoredHash::Hash(stored) if stored == hash => {
                entry.seq = seq;
                Verdict::Unchanged
            }
            StoredHash::Unknown => Verdict::Unknown,
            StoredHash::Hash(_) | StoredHash::Missing => Verdict::Changed,
        }
    }

    /// Records the store's answer for `column`, read in `generation`, unless the book
    /// already holds a hash of that generation or a later one: content the writer accepted
    /// meanwhile.
    pub(in crate::stream) fn learn(
        &mut self,
        column: (Dimension, ChunkKey),
        stored: StoredHash,
        generation: u64,
    ) {
        let entry = self.entries.entry(column).or_insert(Entry {
            seq: 0,
            stored: StoredHash::Unknown,
            generation,
        });
        if entry.generation < generation
            || (entry.generation == generation && entry.stored == StoredHash::Unknown)
        {
            entry.stored = stored;
            entry.generation = generation;
        }
    }

    /// Accepts capture `seq` of `column` with content `hash` for writing in `generation`;
    /// false when a newer capture was accepted or skipped.
    pub(in crate::stream) fn accept(
        &mut self,
        column: (Dimension, ChunkKey),
        seq: u64,
        hash: u64,
        generation: u64,
    ) -> bool {
        let entry = self.entries.entry(column).or_insert(Entry {
            seq,
            stored: StoredHash::Unknown,
            generation,
        });
        if seq < entry.seq {
            return false;
        }
        *entry = Entry {
            seq,
            stored: StoredHash::Hash(hash),
            generation,
        };
        true
    }

    /// Forgets the hash of `column`, whose write failed.
    pub(in crate::stream) fn forget(&mut self, column: (Dimension, ChunkKey)) {
        if let Some(entry) = self.entries.get_mut(&column) {
            entry.stored = StoredHash::Unknown;
        }
    }

    /// Empties the book once it outgrew [`MAX_LOD_BOOK_COLUMNS`]. The caller makes sure no
    /// capture is outstanding, so every later capture is newer than all forgotten ones.
    pub(in crate::stream) fn trim_when_idle(&mut self) {
        if self.entries.len() > MAX_LOD_BOOK_COLUMNS {
            self.entries = HashMap::new();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMN: (Dimension, ChunkKey) = (Dimension(0), ChunkKey::new(0, 1, 0));

    /// The generation every capture of these tests is taken in.
    const NOW: u64 = 7;

    #[test]
    fn an_older_capture_never_overwrites_a_newer_one() {
        let mut book = ColumnBook::default();
        assert!(book.accept(COLUMN, 5, 50, NOW));
        assert!(
            !book.accept(COLUMN, 4, 40, NOW),
            "older content landing late"
        );
        assert_eq!(book.check(COLUMN, 3, 30, NOW), Verdict::Superseded);
        assert!(book.accept(COLUMN, 6, 60, NOW));
    }

    #[test]
    fn an_unchanged_skip_supersedes_older_captures() {
        let mut book = ColumnBook::default();
        assert!(book.accept(COLUMN, 1, 10, NOW));
        // Capture 3 reverted the column to the accepted content; capture 2 is in flight.
        assert_eq!(book.check(COLUMN, 3, 10, NOW), Verdict::Unchanged);
        assert!(!book.accept(COLUMN, 2, 20, NOW));
    }

    #[test]
    fn accepted_content_is_what_later_captures_compare_with() {
        let mut book = ColumnBook::default();
        assert_eq!(book.check(COLUMN, 1, 10, NOW), Verdict::Unknown);
        book.learn(COLUMN, StoredHash::Hash(10), NOW);
        assert!(book.accept(COLUMN, 2, 20, NOW));
        // The store answer for an older question cannot replace the accepted hash.
        book.learn(COLUMN, StoredHash::Hash(10), NOW);
        book.learn(COLUMN, StoredHash::Hash(10), NOW - 1);
        assert_eq!(book.check(COLUMN, 3, 10, NOW), Verdict::Changed);
        assert_eq!(book.check(COLUMN, 3, 20, NOW), Verdict::Unchanged);
        book.forget(COLUMN);
        assert_eq!(book.check(COLUMN, 4, 20, NOW), Verdict::Unknown);
        assert!(
            !book.accept(COLUMN, 2, 20, NOW),
            "sequences survive forgetting"
        );
    }

    #[test]
    fn hashes_of_an_earlier_generation_are_unknown() {
        let mut book = ColumnBook::default();
        assert!(book.accept(COLUMN, 1, 10, NOW));
        // A session change: the hash may be another epoch's.
        assert_eq!(book.check(COLUMN, 2, 10, NOW + 1), Verdict::Unknown);
        assert_eq!(book.check(COLUMN, 0, 10, NOW + 1), Verdict::Superseded);
        // An answer read before the change stays unknown; one read after it counts.
        book.learn(COLUMN, StoredHash::Missing, NOW);
        assert_eq!(book.check(COLUMN, 2, 10, NOW + 1), Verdict::Unknown);
        book.learn(COLUMN, StoredHash::Missing, NOW + 1);
        assert_eq!(book.check(COLUMN, 2, 10, NOW + 1), Verdict::Changed);
    }
}

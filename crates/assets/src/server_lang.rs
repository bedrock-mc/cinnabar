//! Optional, bounded session localization. This is not a pinned base carrier.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

pub const MAX_SERVER_LANG_INPUT_BYTES: usize = 1024 * 1024;
const MAX_TABLE_BYTES: usize = 4 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 512 * 1024;
const POOL_BYTES: usize = 8 * 1024 * 1024;
const ALLOCATION_OVERHEAD: usize = 64;
// Fixed inflater/window and buffered CRC reader allowance; no directory rebuild.
const READER_SCRATCH_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
struct Credits(AtomicUsize);

impl Credits {
    fn reserve(self: &Arc<Self>, bytes: usize) -> Option<Permit> {
        self.0
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|next| *next <= POOL_BYTES)
            })
            .ok()?;
        Some(Permit {
            owner: Arc::clone(self),
            bytes,
        })
    }
}

#[derive(Debug)]
struct Permit {
    owner: Arc<Credits>,
    bytes: usize,
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.owner.0.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

fn process_credits() -> Arc<Credits> {
    static CREDITS: OnceLock<Arc<Credits>> = OnceLock::new();
    Arc::clone(CREDITS.get_or_init(|| Arc::new(Credits(AtomicUsize::new(0)))))
}

struct Entry {
    key: Box<str>,
    value: Box<str>,
    ordinal: usize,
    _strings: Permit,
}

/// Immutable optional table. Clones share entries and their allocation permits.
/// Lookup borrows text; independently retained resolved text belongs to its consumer.
pub struct ServerLangOverlay {
    entries: Vec<Entry>,
    _metadata: Permit,
}

impl std::fmt::Debug for ServerLangOverlay {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServerLangOverlay")
            .field("entry_count", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl ServerLangOverlay {
    /// Reserves candidate memory before invoking a fixed-slice reader. A refused
    /// or absent file leaves the base table usable; no remote carrier is trusted.
    /// The reader must verify exact size and integrity, including an EOF probe.
    pub fn read(declared_bytes: usize, read: impl FnOnce(&mut [u8]) -> bool) -> Option<Arc<Self>> {
        Self::read_with_credits(declared_bytes, read, process_credits())
    }

    fn read_with_credits(
        declared_bytes: usize,
        read: impl FnOnce(&mut [u8]) -> bool,
        credits: Arc<Credits>,
    ) -> Option<Arc<Self>> {
        if declared_bytes > MAX_SERVER_LANG_INPUT_BYTES {
            return None;
        }
        let _input = credits.reserve(declared_bytes.checked_add(ALLOCATION_OVERHEAD)?)?;
        let _reader = credits.reserve(READER_SCRATCH_BYTES)?;
        let mut input = vec![0; declared_bytes];
        if !read(&mut input) {
            return None;
        }
        let text = std::str::from_utf8(&input).ok()?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let rows = translation_rows(text);
        let entry_count = rows.clone().count();
        let metadata = entry_count
            .checked_mul(std::mem::size_of::<Entry>())?
            .checked_add(std::mem::size_of::<Self>())?
            .checked_add(ALLOCATION_OVERHEAD)?;
        if metadata > MAX_METADATA_BYTES {
            return None;
        }
        let metadata_permit = credits.reserve(metadata)?;
        let mut entries = Vec::with_capacity(entry_count);
        let mut table_bytes = metadata;
        for (ordinal, key, value) in rows {
            let strings = key
                .len()
                .checked_add(value.len())?
                .checked_add(2 * ALLOCATION_OVERHEAD)?;
            table_bytes = table_bytes.checked_add(strings)?;
            if table_bytes > MAX_TABLE_BYTES {
                return None;
            }
            let permit = credits.reserve(strings)?;
            entries.push(Entry {
                key: key.into(),
                value: value.into(),
                ordinal,
                _strings: permit,
            });
        }
        // The ordinal makes duplicate resolution deterministic despite an unstable sort.
        entries.sort_unstable_by(|a, b| a.key.cmp(&b.key).then(a.ordinal.cmp(&b.ordinal)));
        entries.reverse();
        entries.dedup_by(|later, earlier| later.key == earlier.key);
        entries.reverse();
        Some(Arc::new(Self {
            entries,
            _metadata: metadata_permit,
        }))
    }

    #[must_use]
    pub fn lookup(&self, key: &str) -> Option<&str> {
        self.entries
            .binary_search_by(|entry| entry.key.as_ref().cmp(key))
            .ok()
            .map(|index| self.entries[index].value.as_ref())
    }
}

/// Borrows valid translations in source order so counting and allocation use the same rows.
fn translation_rows(text: &str) -> impl Iterator<Item = (usize, &str, &str)> + Clone {
    text.lines().enumerate().filter_map(|(ordinal, line)| {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with("##") {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        if key.is_empty() {
            return None;
        }
        let value = value.split_once("\t#").map_or(value, |(value, _)| value);
        Some((ordinal, key, value))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates an isolated credit pool for deterministic allocation tests.
    fn owner() -> Arc<Credits> {
        Arc::new(Credits(AtomicUsize::new(0)))
    }

    #[test]
    fn small_tables_fit_without_reserving_unused_rows() {
        let credits = owner();
        let occupied = credits
            .reserve(POOL_BYTES - READER_SCRATCH_BYTES - 4096)
            .unwrap();
        let table = ServerLangOverlay::read_with_credits(
            3,
            |target| {
                target.copy_from_slice(b"k=v");
                true
            },
            Arc::clone(&credits),
        )
        .unwrap();
        assert_eq!(table.lookup("k"), Some("v"));
        drop(table);
        assert_eq!(credits.0.load(Ordering::Acquire), occupied.bytes);
        drop(occupied);
        assert_eq!(credits.0.load(Ordering::Acquire), 0);
    }

    #[test]
    fn ignored_rows_do_not_consume_metadata_budget() {
        let credits = owner();
        let mut input = "## comment\nmalformed\n=empty\n"
            .repeat(MAX_METADATA_BYTES / std::mem::size_of::<Entry>());
        input.push_str("k=v\n");
        let table = ServerLangOverlay::read_with_credits(
            input.len(),
            |target| {
                target.copy_from_slice(input.as_bytes());
                true
            },
            Arc::clone(&credits),
        )
        .unwrap();
        assert_eq!(table.lookup("k"), Some("v"));
        drop(table);
        assert_eq!(credits.0.load(Ordering::Acquire), 0);
    }

    #[test]
    fn metadata_budget_refusal_returns_all_candidate_credits() {
        let credits = owner();
        let input = "k=v\n".repeat(MAX_METADATA_BYTES / std::mem::size_of::<Entry>() + 1);
        assert!(
            ServerLangOverlay::read_with_credits(
                input.len(),
                |target| {
                    target.copy_from_slice(input.as_bytes());
                    true
                },
                Arc::clone(&credits),
            )
            .is_none()
        );
        assert_eq!(credits.0.load(Ordering::Acquire), 0);
    }

    #[test]
    fn permits_survive_clones_and_return_only_on_final_drop() {
        let credits = owner();
        let table = ServerLangOverlay::read_with_credits(
            5,
            |target| {
                target.copy_from_slice(b"k=old");
                true
            },
            Arc::clone(&credits),
        )
        .unwrap();
        let retained = Arc::clone(&table);
        let charged = credits.0.load(Ordering::Acquire);
        assert!(charged > 0);
        drop(table);
        assert_eq!(credits.0.load(Ordering::Acquire), charged);
        let new = ServerLangOverlay::read_with_credits(
            5,
            |target| {
                target.copy_from_slice(b"k=new");
                true
            },
            Arc::clone(&credits),
        )
        .unwrap();
        assert_eq!(retained.lookup("k"), Some("old"));
        assert_eq!(new.lookup("k"), Some("new"));
        drop(new);
        assert_eq!(credits.0.load(Ordering::Acquire), charged);
        drop(retained);
        assert_eq!(credits.0.load(Ordering::Acquire), 0);
    }

    #[test]
    fn shared_pool_refusal_and_parse_failure_never_remint() {
        let credits = owner();
        let full = credits.reserve(POOL_BYTES).unwrap();
        assert!(
            ServerLangOverlay::read_with_credits(
                3,
                |_| panic!("uncredited reader"),
                Arc::clone(&credits)
            )
            .is_none()
        );
        assert_eq!(credits.0.load(Ordering::Acquire), POOL_BYTES);
        drop(full);
        assert!(
            ServerLangOverlay::read_with_credits(
                3,
                |target| {
                    target.copy_from_slice(b"k=\xff");
                    true
                },
                Arc::clone(&credits)
            )
            .is_none()
        );
        assert_eq!(credits.0.load(Ordering::Acquire), 0);
        assert!(ServerLangOverlay::read_with_credits(3, |_| false, Arc::clone(&credits)).is_none());
        assert_eq!(credits.0.load(Ordering::Acquire), 0);
    }

    #[test]
    fn real_retained_candidates_exhaust_one_owner_without_resetting_it() {
        let credits = owner();
        let input = (0..512)
            .map(|index| format!("key{index}={}\n", "x".repeat(1024)))
            .collect::<String>();
        let mut retained = Vec::new();
        let mut refused = false;
        for _ in 0..32 {
            let before = credits.0.load(Ordering::Acquire);
            match ServerLangOverlay::read_with_credits(
                input.len(),
                |target| {
                    target.copy_from_slice(input.as_bytes());
                    true
                },
                Arc::clone(&credits),
            ) {
                Some(table) => retained.push(table),
                None => {
                    assert_eq!(credits.0.load(Ordering::Acquire), before);
                    refused = true;
                    break;
                }
            }
        }
        assert!(refused);
        let last = Arc::clone(retained.last().unwrap());
        retained.clear();
        assert!(credits.0.load(Ordering::Acquire) > 0);
        assert_eq!(last.lookup("key0").unwrap().len(), 1024);
        drop(last);
        assert_eq!(credits.0.load(Ordering::Acquire), 0);
    }
}

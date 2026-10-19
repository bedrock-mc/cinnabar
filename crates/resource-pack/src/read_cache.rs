//! Shared, bounded plaintext storage with persistent per-entry content hashes.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::{AdmissionError, fingerprint::hash_bytes};

/// Each admitted stack retains at most 64 MiB of plaintext, evicting in insertion order.
/// Cloned packs share the budget; composing independently admitted stacks keeps their budgets.
/// Hashes survive eviction and are bounded by the admitted entry count. Active readers and
/// returned file copies own their bytes separately from this retained-storage budget.
pub(crate) const MAX_CACHED_READ_BYTES: usize = 64 * 1024 * 1024;

type Bytes = Arc<Box<[u8]>>;
type Fingerprint = Result<[u8; 32], AdmissionError>;

#[derive(Debug, Default)]
struct Retained {
    files: HashMap<usize, Bytes>,
    order: VecDeque<usize>,
    bytes: usize,
}

#[derive(Debug)]
pub(crate) struct ReadCache {
    budget: usize,
    next_id: AtomicUsize,
    retained: Mutex<Retained>,
}

#[derive(Debug)]
pub(crate) struct CachedEntry {
    id: usize,
    fingerprint: Mutex<Option<Fingerprint>>,
    #[cfg(test)]
    pub(crate) reads: AtomicUsize,
}

impl Default for ReadCache {
    fn default() -> Self {
        Self::new(MAX_CACHED_READ_BYTES)
    }
}

impl ReadCache {
    /// Returns retained payload bytes for deterministic budget checks.
    #[cfg(test)]
    pub(crate) fn retained_bytes(&self) -> usize {
        self.retained
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .bytes
    }

    /// Creates a cache whose retained payloads never exceed `budget` bytes.
    pub(crate) fn new(budget: usize) -> Self {
        Self {
            budget,
            next_id: AtomicUsize::new(0),
            retained: Mutex::default(),
        }
    }

    /// Assigns a distinct entry identity shared by logical aliases and cloned packs.
    pub(crate) fn entry(&self) -> Arc<CachedEntry> {
        Arc::new(CachedEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            fingerprint: Mutex::default(),
            #[cfg(test)]
            reads: AtomicUsize::new(0),
        })
    }

    /// Reads under the entry lock so concurrent subscribers inflate and decrypt only once.
    pub(crate) fn read(
        &self,
        entry: &CachedEntry,
        load: impl FnOnce() -> Result<Box<[u8]>, AdmissionError>,
    ) -> Result<Bytes, AdmissionError> {
        let mut fingerprint = entry.fingerprint.lock().unwrap_or_else(|e| e.into_inner());
        self.load(entry, &mut fingerprint, load)
    }

    /// Returns the remembered hash without requiring the plaintext to remain resident.
    pub(crate) fn fingerprint(
        &self,
        entry: &CachedEntry,
        load: impl FnOnce() -> Result<Box<[u8]>, AdmissionError>,
    ) -> Fingerprint {
        let mut fingerprint = entry.fingerprint.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(result) = *fingerprint {
            return result;
        }
        self.load(entry, &mut fingerprint, load)?;
        fingerprint.expect("successful read records its fingerprint")
    }

    /// Loads a missing payload while its caller holds only this entry's lock.
    fn load(
        &self,
        entry: &CachedEntry,
        fingerprint: &mut Option<Fingerprint>,
        load: impl FnOnce() -> Result<Box<[u8]>, AdmissionError>,
    ) -> Result<Bytes, AdmissionError> {
        if let Some(Err(error)) = *fingerprint {
            return Err(error);
        }
        if let Some(bytes) = self
            .retained
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .files
            .get(&entry.id)
            .cloned()
        {
            return Ok(bytes);
        }
        #[cfg(test)]
        entry.reads.fetch_add(1, Ordering::Relaxed);
        let bytes = match load() {
            Ok(bytes) => Arc::new(bytes),
            Err(error) => {
                *fingerprint = Some(Err(error));
                return Err(error);
            }
        };
        fingerprint.get_or_insert_with(|| Ok(hash_bytes(bytes.as_ref())));
        if bytes.len() <= self.budget && self.budget != 0 {
            let mut retained = self.retained.lock().unwrap_or_else(|e| e.into_inner());
            while retained.bytes + bytes.len() > self.budget {
                let oldest = retained.order.pop_front().expect("retained payload exists");
                retained.bytes -= retained.files.remove(&oldest).expect("queued entry").len();
            }
            retained.bytes += bytes.len();
            retained.order.push_back(entry.id);
            retained.files.insert(entry.id, Arc::clone(&bytes));
        }
        Ok(bytes)
    }
}

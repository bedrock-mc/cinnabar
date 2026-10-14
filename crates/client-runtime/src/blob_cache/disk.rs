//! Bounded background I/O for the protocol's verified memory cache.

use protocol::{BlobCacheLimits, BlobCacheStore, ClientBlobCache, WeakClientBlobCache};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::Duration,
};
use tokio::sync::oneshot;

mod database;
mod file;
#[cfg(test)]
mod tests;
mod worker;

/// Hard limit on the database file, including its indexes and transaction headroom.
pub const CLIENT_BLOB_DISK_CAP_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_QUEUED_BYTES: usize = protocol::MAX_CLIENT_BLOB_RECONSTRUCTED_BYTES;
const MAX_DISK_ENTRIES: usize = protocol::MAX_CLIENT_BLOB_CACHE_ENTRIES * 4;
const MAX_READ_REQUESTS: usize = 128;
const MAX_READ_HASHES: usize = protocol::MAX_CLIENT_BLOB_CACHE_ENTRIES;
const WRITE_BATCH_ENTRIES: usize = 256;
const WRITE_BATCH_BYTES: usize = MAX_QUEUED_BYTES / 8;
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
const PREFETCH_BUDGET: Duration = Duration::from_millis(2);

#[derive(Debug)]
struct ReadRequest {
    hashes: Vec<u64>,
    done: oneshot::Sender<()>,
}

#[derive(Debug, Default)]
struct Mailbox {
    writes: HashMap<u64, (Arc<[u8]>, u64)>,
    touches: HashMap<u64, u64>,
    queued_bytes: usize,
    reads: VecDeque<ReadRequest>,
    read_hashes: usize,
    keys: HashSet<u64>,
    clock: u64,
    ready: bool,
    failed: bool,
    stopping: bool,
}

#[derive(Debug, Default)]
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Condvar,
}

struct DiskCache {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for DiskCache {
    /// Shows ownership without dumping the disk index or queued blob bodies.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiskCache").finish_non_exhaustive()
    }
}

/// Starts disk work without opening a file on the calling thread.
pub(super) fn start(path: PathBuf, limits: BlobCacheLimits) -> ClientBlobCache {
    let cache = ClientBlobCache::with_limits(limits);
    let shared = Arc::new(Shared::default());
    let weak = cache.downgrade();
    let worker_shared = shared.clone();
    let worker = std::thread::Builder::new()
        .name("client-blob-cache".into())
        .spawn(move || {
            worker::run(
                path,
                CLIENT_BLOB_DISK_CAP_BYTES,
                limits,
                weak,
                worker_shared,
            )
        });
    match worker {
        Ok(worker) => cache.with_store(Arc::new(DiskCache {
            shared,
            worker: Mutex::new(Some(worker)),
        })),
        Err(error) => {
            tracing::warn!(%error, "blob cache worker unavailable; using memory only");
            cache
        }
    }
}

impl BlobCacheStore for DiskCache {
    /// Retains a shared body within independent queue byte and entry ceilings.
    fn persist(&self, hash: u64, payload: Arc<[u8]>) {
        let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
            return;
        };
        if mailbox.failed || mailbox.stopping || mailbox.writes.contains_key(&hash) {
            return;
        }
        let bytes = mailbox.queued_bytes.saturating_add(payload.len());
        if bytes > MAX_QUEUED_BYTES || mailbox.writes.len() >= WRITE_BATCH_ENTRIES {
            return;
        }
        mailbox.clock = mailbox.clock.saturating_add(1);
        let stamp = mailbox.clock;
        mailbox.writes.insert(hash, (payload, stamp));
        mailbox.queued_bytes = bytes;
        if bytes >= WRITE_BATCH_BYTES || mailbox.writes.len() >= WRITE_BATCH_ENTRIES {
            self.shared.wake.notify_one();
        }
    }

    /// Coalesces hits into one access stamp per key, without a filesystem call.
    fn touch(&self, hashes: &[u64]) {
        let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
            return;
        };
        if mailbox.failed || mailbox.stopping {
            return;
        }
        for hash in hashes {
            if !mailbox.keys.contains(hash) && !mailbox.writes.contains_key(hash) {
                continue;
            }
            mailbox.clock = mailbox.clock.saturating_add(1);
            let stamp = mailbox.clock;
            if mailbox.touches.len() < MAX_DISK_ENTRIES || mailbox.touches.contains_key(hash) {
                mailbox.touches.insert(*hash, stamp);
            }
        }
    }

    /// Waits briefly for an off-thread read; failed, unknown or busy stores finish immediately.
    fn prefetch(&self, hashes: Vec<u64>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let done = {
                let Ok(mut mailbox) = self.shared.mailbox.try_lock() else {
                    return;
                };
                if !mailbox.ready || mailbox.failed || mailbox.stopping {
                    return;
                }
                let mut hashes = hashes
                    .into_iter()
                    .filter(|hash| mailbox.keys.contains(hash))
                    .collect::<Vec<_>>();
                hashes.sort_unstable();
                hashes.dedup();
                if hashes.is_empty()
                    || mailbox.reads.len() >= MAX_READ_REQUESTS
                    || mailbox.read_hashes.saturating_add(hashes.len()) > MAX_READ_HASHES
                {
                    return;
                }
                let (done, receiver) = oneshot::channel();
                mailbox.read_hashes += hashes.len();
                mailbox.reads.push_back(ReadRequest { hashes, done });
                self.shared.wake.notify_one();
                receiver
            };
            let _ = tokio::time::timeout(PREFETCH_BUDGET, done).await;
        })
    }
}

impl Drop for DiskCache {
    /// Flushes the last batch and releases the database lock when its final owner goes away.
    fn drop(&mut self) {
        self.shared
            .mailbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopping = true;
        self.shared.wake.notify_one();
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = worker.join();
        }
    }
}

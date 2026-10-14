use super::*;
use anyhow::Result;
use database::Store;
use std::time::Instant;

/// Owns all filesystem work and converts a terminal storage failure into memory-only operation.
pub(super) fn run(
    path: PathBuf,
    cap: u64,
    limits: BlobCacheLimits,
    cache: WeakClientBlobCache,
    shared: Arc<Shared>,
) {
    if let Err(error) = service(&path, cap, limits, &cache, &shared) {
        tracing::warn!(%error, path = %path.display(), "blob disk cache unavailable; using memory only");
        let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
        mailbox.failed = true;
        mailbox.keys.clear();
        mailbox.writes.clear();
        mailbox.touches.clear();
        mailbox.reads.clear();
        mailbox.queued_bytes = 0;
        mailbox.read_hashes = 0;
    }
}

/// Loads the key index, warms recent bodies, and services reads ahead of batched writes.
fn service(
    path: &std::path::Path,
    cap: u64,
    limits: BlobCacheLimits,
    cache: &WeakClientBlobCache,
    shared: &Shared,
) -> Result<()> {
    let mut store = Store::open(path, cap)?;
    let mut warm = store
        .index
        .iter()
        .map(|(&hash, entry)| (entry.stamp, hash, entry.bytes))
        .collect::<Vec<_>>();
    warm.sort_unstable();
    {
        let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
        let previous = warm.last().map_or(0, |entry| entry.0);
        mailbox.clock = mailbox.clock.saturating_add(previous);
        for (_, stamp) in mailbox.writes.values_mut() {
            *stamp = stamp.saturating_add(previous)
        }
        for stamp in mailbox.touches.values_mut() {
            *stamp = stamp.saturating_add(previous)
        }
        mailbox.keys.extend(store.index.keys().copied());
        mailbox.ready = true;
    }
    let mut warm_bytes = 0u64;
    let mut selected = Vec::new();
    for entry in warm.into_iter().rev() {
        if selected.len() >= protocol::MAX_CLIENT_BLOB_CACHE_ENTRIES / 2 {
            break;
        }
        if warm_bytes.saturating_add(entry.2) <= limits.trim_floor_bytes as u64 {
            warm_bytes += entry.2;
            selected.push(entry);
        }
    }
    let mut last_flush = Instant::now();
    // Admit older selected bodies first, so the memory LRU retains the newest bodies longest.
    // Once a join starts demand reads, stop warming to avoid evicting its requested working set.
    while let Some((_, hash, _)) = selected.pop() {
        if service_reads(&mut store, cache, shared)? {
            break;
        }
        load(&mut store, cache, shared, hash)?;
        if shared
            .mailbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopping
        {
            break;
        }
    }
    loop {
        service_reads(&mut store, cache, shared)?;
        let (writes, touches, stopping) = {
            let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
            let due = last_flush.elapsed() >= FLUSH_INTERVAL
                || mailbox.queued_bytes >= WRITE_BATCH_BYTES
                || mailbox.writes.len() >= WRITE_BATCH_ENTRIES
                || mailbox.stopping;
            if !due {
                if !mailbox.reads.is_empty() {
                    drop(mailbox);
                    continue;
                }
                let wait = FLUSH_INTERVAL.saturating_sub(last_flush.elapsed());
                let _ = shared
                    .wake
                    .wait_timeout(mailbox, wait)
                    .unwrap_or_else(|e| e.into_inner());
                continue;
            }
            mailbox.queued_bytes = 0;
            (
                std::mem::take(&mut mailbox.writes),
                std::mem::take(&mut mailbox.touches),
                mailbox.stopping,
            )
        };
        if !writes.is_empty() || !touches.is_empty() {
            let written = writes.keys().copied().collect::<Vec<_>>();
            let evicted = store.flush(writes, touches)?;
            // Access-only batches neither scan the index nor compact blob pages.
            if !written.is_empty() {
                if std::fs::metadata(path)?.len() > cap / 2 {
                    store.compact()?
                }
                let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
                for hash in evicted {
                    mailbox.keys.remove(&hash);
                }
                for hash in written {
                    if store.index.contains_key(&hash) {
                        mailbox.keys.insert(hash);
                    }
                }
            }
        }
        last_flush = Instant::now();
        if stopping {
            return Ok(());
        }
    }
}

/// Drains a bounded read batch without holding the mailbox or protocol cache lock during I/O.
fn service_reads(store: &mut Store, cache: &WeakClientBlobCache, shared: &Shared) -> Result<bool> {
    let requests = {
        let mut mailbox = shared.mailbox.lock().unwrap_or_else(|e| e.into_inner());
        mailbox.read_hashes = 0;
        std::mem::take(&mut mailbox.reads)
    };
    let serviced = !requests.is_empty();
    for request in requests {
        service_request(request, |hash| load(store, cache, shared, hash))?;
    }
    Ok(serviced)
}

/// Stops a request between loads when its caller no longer waits for the prefetched bodies.
fn service_request(request: ReadRequest, mut load: impl FnMut(u64) -> Result<bool>) -> Result<()> {
    for hash in request.hashes {
        if request.done.is_closed() {
            break;
        }
        load(hash)?;
    }
    let _ = request.done.send(());
    Ok(())
}

/// Validates before admitting a disk body and removes corrupt records without session errors.
fn load(
    store: &mut Store,
    cache: &WeakClientBlobCache,
    shared: &Shared,
    hash: u64,
) -> Result<bool> {
    let Some(payload) = store.read(hash)? else {
        store.remove(hash)?;
        shared
            .mailbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys
            .remove(&hash);
        return Ok(false);
    };
    match cache.admit(hash, payload) {
        Ok(admitted) => Ok(admitted),
        Err(protocol::BlobCacheError::HashMismatch { .. }) => {
            store.remove(hash)?;
            shared
                .mailbox
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys
                .remove(&hash);
            Ok(false)
        }
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expiration during the first load prevents all remaining reads without relying on timing.
    #[test]
    fn blob_cache_prefetch_stops_when_request_expires_during_load() {
        let (done, receiver) = oneshot::channel();
        let request = ReadRequest {
            hashes: vec![1, 2, 3],
            done,
        };
        let mut receiver = Some(receiver);
        let mut loaded = Vec::new();
        service_request(request, |hash| {
            loaded.push(hash);
            drop(receiver.take());
            Ok(true)
        })
        .unwrap();
        assert_eq!(loaded, [1]);
    }
}

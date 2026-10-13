use super::*;
use database::{ACCESS, BLOBS, Store};
use redb::ReadableDatabase;
use std::path::Path;

/// Runs the production worker with test-owned limits and exposes its bounded mailbox.
fn cache_at(path: &Path, cap: u64, limits: BlobCacheLimits) -> (ClientBlobCache, Arc<Shared>) {
    let cache = ClientBlobCache::with_limits(limits);
    let shared = Arc::new(Shared::default());
    let worker_shared = shared.clone();
    let path = path.to_path_buf();
    let weak = cache.downgrade();
    let worker = std::thread::spawn(move || worker::run(path, cap, limits, weak, worker_shared));
    let cache = cache.with_store(Arc::new(DiskCache {
        shared: shared.clone(),
        worker: Mutex::new(Some(worker)),
    }));
    (cache, shared)
}

/// Waits for the worker to finish opening and indexing its database without sleeping in tests.
fn ready(shared: &Shared) {
    test_time::eventually("blob store ready", || shared.mailbox.lock().unwrap().ready);
}

/// Creates one timestamped, content-addressed insertion batch.
fn batch(payload: &[u8], stamp: u64) -> HashMap<u64, (Arc<[u8]>, u64)> {
    HashMap::from([(
        protocol::client_blob_hash(payload),
        (Arc::from(payload), stamp),
    )])
}

/// Flush on final-owner drop survives a restart and re-verifies the warmed body.
#[test]
fn blob_cache_restart_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("blobs.redb");
    let (cache, shared) = cache_at(
        &path,
        CLIENT_BLOB_DISK_CAP_BYTES,
        BlobCacheLimits::default(),
    );
    ready(&shared);
    let hash = protocol::client_blob_hash(b"restart-body");
    test_time::eventually("restart write enqueued", || {
        cache.insert(b"restart-body").unwrap();
        let mailbox = shared.mailbox.lock().unwrap();
        mailbox.writes.contains_key(&hash) || mailbox.keys.contains(&hash)
    });
    drop(cache);
    let (cache, shared) = cache_at(
        &path,
        CLIENT_BLOB_DISK_CAP_BYTES,
        BlobCacheLimits::default(),
    );
    test_time::eventually("verified restart body", || cache.contains(hash));
    ready(&shared);
    assert_eq!(cache.total_bytes(), b"restart-body".len());
    drop(cache);
    let store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
    assert_eq!(store.read(hash).unwrap().unwrap().as_ref(), b"restart-body");
}

/// A readable record with the wrong content hash is deleted and cannot become a cache hit.
#[test]
fn blob_cache_corrupt_body_becomes_miss_and_is_deleted() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("blobs.redb");
    let hash = protocol::client_blob_hash(b"correct");
    {
        let store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
        let write = store.db.begin_write().unwrap();
        write
            .open_table(BLOBS)
            .unwrap()
            .insert(hash, b"corrupt".as_slice())
            .unwrap();
        write
            .open_table(ACCESS)
            .unwrap()
            .insert(hash, (7, 1))
            .unwrap();
        write.commit().unwrap();
    }
    let (cache, shared) = cache_at(
        &path,
        CLIENT_BLOB_DISK_CAP_BYTES,
        BlobCacheLimits::default(),
    );
    ready(&shared);
    // Final-owner drop joins the worker after its verification/deletion finishes.
    drop(cache);
    let store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
    assert!(!store.index.contains_key(&hash));
    assert!(store.read(hash).unwrap().is_none());
}

/// Uncommitted partial table updates disappear on reopening; earlier committed bodies survive.
#[test]
fn blob_cache_partial_transaction_is_ignored() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("blobs.redb");
    let good = protocol::client_blob_hash(b"committed");
    let partial = protocol::client_blob_hash(b"partial");
    {
        let mut store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
        store.flush(batch(b"committed", 1), HashMap::new()).unwrap();
        let write = store.db.begin_write().unwrap();
        write
            .open_table(BLOBS)
            .unwrap()
            .insert(partial, b"partial".as_slice())
            .unwrap();
        // Drop between the body and metadata writes, without committing.
    }
    let store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
    assert_eq!(store.read(good).unwrap().unwrap().as_ref(), b"committed");
    assert!(store.read(partial).unwrap().is_none());
}

/// A truncated database fails open and leaves the working memory tier available.
#[test]
fn blob_cache_torn_database_degrades_to_memory() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("blobs.redb");
    {
        let mut store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
        store.flush(batch(b"durable", 1), HashMap::new()).unwrap();
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(19)
        .unwrap();
    let (cache, shared) = cache_at(
        &path,
        CLIENT_BLOB_DISK_CAP_BYTES,
        BlobCacheLimits::default(),
    );
    test_time::eventually("torn store disabled", || {
        shared.mailbox.lock().unwrap().failed
    });
    let hash = cache.insert(b"memory survives").unwrap();
    assert!(cache.contains(hash));
    assert!(!cache.contains(protocol::client_blob_hash(b"durable")));
}

/// Accesses flushed before pressure protect the recent entry instead of the oldest insertion.
#[test]
fn blob_cache_disk_cap_evicts_least_recently_used() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("blobs.redb");
    // Leave room for whole database-page allocations while forcing three bodies over the live cap.
    let cap = 20 * 1024 * 1024;
    let mut store = Store::open(&path, cap).unwrap();
    let payloads = [vec![1; 1_900_000], vec![2; 1_900_000], vec![3; 1_900_000]];
    let hashes = payloads
        .each_ref()
        .map(|payload| protocol::client_blob_hash(payload));
    store.flush(batch(&payloads[0], 1), HashMap::new()).unwrap();
    store.flush(batch(&payloads[1], 2), HashMap::new()).unwrap();
    store
        .flush(batch(&payloads[2], 4), HashMap::from([(hashes[0], 3)]))
        .unwrap();
    assert!(store.index.contains_key(&hashes[0]));
    assert!(!store.index.contains_key(&hashes[1]));
    assert!(store.index.contains_key(&hashes[2]));
    assert!(store.bytes <= cap / 4);
    store.compact().unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() <= cap);
    drop(store);
    let reopened = Store::open(&path, cap).unwrap();
    assert!(!reopened.index.contains_key(&hashes[1]));
}

/// The backend's hard ceiling also bounds physical growth inside an unfinished transaction.
#[test]
fn blob_cache_file_cap_rejects_growth_before_writing() {
    use redb::StorageBackend;
    let temp = tempfile::tempdir().unwrap();
    let file = file::CappedFile::open(&temp.path().join("bounded"), 64).unwrap();
    file.set_len(64).unwrap();
    assert!(file.set_len(65).is_err());
    assert!(file.write(63, &[1, 2]).is_err());
    assert!(file.write(u64::MAX, &[1]).is_err());
    assert_eq!(file.len().unwrap(), 64);
}

/// Repeated hits coalesce in memory, and only an explicit batch changes persisted access order.
#[test]
fn blob_cache_timestamps_are_batched() {
    let temp = tempfile::tempdir().unwrap();
    let mut store =
        Store::open(&temp.path().join("blobs.redb"), CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
    let hash = protocol::client_blob_hash(b"hit");
    store.flush(batch(b"hit", 1), HashMap::new()).unwrap();
    let shared = Arc::new(Shared::default());
    shared.mailbox.lock().unwrap().keys.insert(hash);
    shared.mailbox.lock().unwrap().clock = 1;
    let disk = DiskCache {
        shared: shared.clone(),
        worker: Mutex::new(None),
    };
    for _ in 0..1_000 {
        disk.touch(&[hash]);
    }
    let read = store.db.begin_read().unwrap();
    assert_eq!(
        read.open_table(ACCESS)
            .unwrap()
            .get(hash)
            .unwrap()
            .unwrap()
            .value()
            .1,
        1
    );
    drop(read);
    let touches = std::mem::take(&mut shared.mailbox.lock().unwrap().touches);
    assert_eq!(touches.len(), 1);
    store.flush(HashMap::new(), touches).unwrap();
    drop(store);
    let store = Store::open(&temp.path().join("blobs.redb"), CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
    assert_eq!(store.index[&hash].stamp, 1_001);
}

/// Both queued body bytes and queued metadata stay bounded when disk service is unavailable.
#[test]
fn blob_cache_writer_queue_is_bounded_and_nonblocking() {
    let shared = Arc::new(Shared::default());
    let disk = DiskCache {
        shared: shared.clone(),
        worker: Mutex::new(None),
    };
    let body: Arc<[u8]> = Arc::from(vec![0; 128 * 1024]);
    for hash in 0..1_000 {
        disk.persist(hash, body.clone());
    }
    let mailbox = shared.mailbox.lock().unwrap();
    assert!(mailbox.queued_bytes <= MAX_QUEUED_BYTES);
    assert!(mailbox.writes.len() <= WRITE_BATCH_ENTRIES);
    let before = mailbox.writes.len();
    disk.persist(u64::MAX, body);
    assert_eq!(mailbox.writes.len(), before);
    drop(mailbox);
}

/// An unusable parent path disables persistence once while inserts and hits keep working.
#[test]
fn blob_cache_open_failure_degrades_to_memory() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("file");
    std::fs::write(&parent, b"not a directory").unwrap();
    let (cache, shared) = cache_at(
        &parent.join("blobs.redb"),
        CLIENT_BLOB_DISK_CAP_BYTES,
        BlobCacheLimits::default(),
    );
    test_time::eventually("failed store disabled", || {
        shared.mailbox.lock().unwrap().failed
    });
    let hash = cache.insert(b"still cached").unwrap();
    assert!(cache.contains(hash));
    assert!(shared.mailbox.lock().unwrap().writes.is_empty());
}

/// A second cache cannot share the writer lease, and does not damage the first database.
#[test]
fn blob_cache_competing_process_lease_degrades() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("blobs.redb");
    let first = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
    assert!(Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).is_err());
    drop(first);
    assert!(Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).is_ok());
}

/// The larger disk tier supplies an old body on demand after bounded recent warming.
#[tokio::test]
async fn blob_cache_demand_read_promotes_body_without_growing_memory() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("blobs.redb");
    let old = b"older-body";
    let recent = b"newer-body";
    let old_hash = protocol::client_blob_hash(old);
    let recent_hash = protocol::client_blob_hash(recent);
    {
        let mut store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
        store.flush(batch(old, 1), HashMap::new()).unwrap();
        store.flush(batch(recent, 2), HashMap::new()).unwrap();
    }
    let limits = BlobCacheLimits {
        trim_trigger_bytes: recent.len(),
        trim_floor_bytes: recent.len(),
    };
    let (cache, shared) = cache_at(&path, CLIENT_BLOB_DISK_CAP_BYTES, limits);
    test_time::eventually("most recent body warmed", || cache.contains(recent_hash));
    assert!(!cache.contains(old_hash));
    let (done, receiver) = oneshot::channel();
    {
        let mut mailbox = shared.mailbox.lock().unwrap();
        mailbox.reads.push_back(ReadRequest {
            hashes: vec![old_hash],
            done,
        });
        mailbox.read_hashes += 1;
    }
    shared.wake.notify_one();
    tokio::time::timeout(test_time::DEFAULT_TIMEOUT, receiver)
        .await
        .unwrap()
        .unwrap();
    assert!(cache.contains(old_hash));
    assert!(!cache.contains(recent_hash));
    assert!(cache.total_bytes() <= limits.trim_trigger_bytes);
}

/// The store never queues a disk read for an unknown key or a failed index.
#[tokio::test]
async fn blob_cache_unknown_or_failed_store_does_not_wait() {
    use std::task::{Context, Poll};
    let shared = Arc::new(Shared::default());
    shared.mailbox.lock().unwrap().ready = true;
    let disk = DiskCache {
        shared: shared.clone(),
        worker: Mutex::new(None),
    };
    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = disk.prefetch(vec![1]);
    assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(()));
    drop(future);
    assert!(shared.mailbox.lock().unwrap().reads.is_empty());
    shared.mailbox.lock().unwrap().failed = true;
    let mut future = disk.prefetch(vec![1]);
    assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(()));
}

/// A small cache workload measures batched durable writes, index loading and verified reads.
#[test]
fn blob_cache_store_workload_cost() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("blobs.redb");
    let mut store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
    let mut hashes = Vec::new();
    let mut batches = Vec::new();
    for batch_index in 0_u64..8 {
        let writes = (0_u64..128)
            .map(|index| {
                let index = batch_index * 128 + index;
                let mut payload = vec![0; 4096];
                payload[..8].copy_from_slice(&index.to_le_bytes());
                let hash = protocol::client_blob_hash(&payload);
                hashes.push(hash);
                (hash, (Arc::from(payload), index + 1))
            })
            .collect();
        let start = std::time::Instant::now();
        store.flush(writes, HashMap::new()).unwrap();
        batches.push(start.elapsed().as_secs_f64() * 1e6);
    }
    drop(store);
    let start = std::time::Instant::now();
    let store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
    let index_us = start.elapsed().as_secs_f64() * 1e6;
    let cache = ClientBlobCache::default();
    let weak = cache.downgrade();
    let start = std::time::Instant::now();
    for hash in hashes {
        assert!(
            weak.admit(hash, store.read(hash).unwrap().unwrap())
                .unwrap()
        );
    }
    let read_us = start.elapsed().as_secs_f64() * 1e6;
    batches.sort_by(f64::total_cmp);
    eprintln!(
        "BLOB_DISK entries={} body_bytes={} batch_128_p50_us={:.3} batch_128_max_us={:.3} reopen_index_us={index_us:.3} read_verify_1024_us={read_us:.3}",
        cache.entry_count(),
        cache.total_bytes(),
        batches[4],
        batches[7]
    );
    assert!(cache.total_bytes() <= cache.limits().trim_trigger_bytes);
}

/// Terminates only this test's child process and reopens its unfinished cache transaction.
#[test]
fn blob_cache_crash_mid_transaction_preserves_committed_body() {
    const CHILD_PATH: &str = "CINNABAR_BLOB_CRASH_TEST_PATH";
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        let path = std::path::PathBuf::from(path);
        let store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
        let write = store.db.begin_write().unwrap();
        let payload = vec![7; 4 * 1024 * 1024];
        write
            .open_table(BLOBS)
            .unwrap()
            .insert(protocol::client_blob_hash(&payload), payload.as_slice())
            .unwrap();
        std::fs::write(path.with_extension("ready"), b"partial body written").unwrap();
        // The parent owns this stdin pipe and kills this exact child while the transaction is live.
        use std::io::Read;
        let _ = std::io::stdin().read_exact(&mut [0]);
        return;
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        /// Cleans up only the process this fixture spawned, including after a failed assertion.
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("blobs.redb");
    let committed = protocol::client_blob_hash(b"before crash");
    {
        let mut store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
        store
            .flush(batch(b"before crash", 1), HashMap::new())
            .unwrap();
    }
    let mut child = Child(std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact").arg("blob_cache::disk::tests::blob_cache_crash_mid_transaction_preserves_committed_body")
        .env(CHILD_PATH, &path)
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().unwrap());
    test_time::eventually("child's unfinished blob transaction", || {
        path.with_extension("ready").is_file()
    });
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let store = Store::open(&path, CLIENT_BLOB_DISK_CAP_BYTES).unwrap();
    assert_eq!(
        store.read(committed).unwrap().unwrap().as_ref(),
        b"before crash"
    );
    assert!(
        store
            .read(protocol::client_blob_hash(&vec![7; 4 * 1024 * 1024]))
            .unwrap()
            .is_none()
    );
}

/// A valid maximum-sized reconstructed body can enter an otherwise empty writer queue.
#[test]
fn blob_cache_writer_accepts_largest_deliverable_body() {
    let shared = Arc::new(Shared::default());
    let disk = DiskCache {
        shared: shared.clone(),
        worker: Mutex::new(None),
    };
    let payload: Arc<[u8]> = Arc::from(vec![0; protocol::MAX_CLIENT_BLOB_RECONSTRUCTED_BYTES]);
    disk.persist(1, payload);
    let mailbox = shared.mailbox.lock().unwrap();
    assert_eq!(mailbox.writes.len(), 1);
    assert_eq!(
        mailbox.queued_bytes,
        protocol::MAX_CLIENT_BLOB_RECONSTRUCTED_BYTES
    );
}

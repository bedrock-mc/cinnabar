use std::{path::PathBuf, sync::Arc};

use tempfile::TempDir;

use super::*;
use crate::{
    error::{HeaderError, IndexFault},
    test_support::{flip_byte, key, record, truncate, zero_bytes},
};

fn region0() -> RegionId {
    RegionId::new(0, 0, 0).expect("region")
}

fn path_in(dir: &TempDir) -> PathBuf {
    dir.path().join(region0().file_name())
}

fn open(path: &Path) -> RegionFile {
    RegionFile::open_writable(path, region0()).expect("open writable")
}

fn wipe_commit_slots(path: &Path) {
    for at in COMMIT_SLOT_OFFSETS {
        zero_bytes(path, at as u64, COMMIT_SLOT_LEN);
    }
}

#[test]
fn committed_records_survive_reopening() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let records: Vec<_> = (0..8)
        .map(|i| record(key(0, i, i - 2, 31 - i), i as u32, 4))
        .collect();
    let mut region = open(&path);
    for bytes in &records {
        region.put(bytes).expect("put");
    }
    assert!(region.commit().expect("commit"));
    assert!(!region.commit().expect("nothing to commit"));
    drop(region);

    let region = open(&path);
    assert_eq!(region.recovery(), None);
    assert_eq!(region.record_count(), records.len());
    for (i, bytes) in records.iter().enumerate() {
        let i = i as i32;
        assert_eq!(
            region.get(key(0, i, i - 2, 31 - i)).expect("get"),
            Some(bytes.clone())
        );
    }
    assert_eq!(region.get(key(0, 9, 0, 0)).expect("absent"), None);
    assert_eq!(region.dead_bytes(), 0);
}

#[test]
fn uncommitted_changes_are_lost_but_the_previous_commit_stays_valid() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let a = key(0, 1, 0, 1);
    let b = key(0, 2, 0, 2);
    let mut region = open(&path);
    region.put(&record(a, 1, 3)).expect("put a");
    region.commit().expect("commit");
    region.put(&record(a, 2, 3)).expect("overwrite a");
    region.put(&record(b, 3, 3)).expect("put b");
    region.set_chunk_hash(0, 0, Some(9)).expect("hash");
    // The frames reach the file, as when the process dies before the commit slot write.
    region.write_pending().expect("append frames");
    drop(region);

    let region = open(&path);
    assert_eq!(region.recovery(), None);
    assert_eq!(region.get(a).expect("a"), Some(record(a, 1, 3)));
    assert_eq!(region.get(b).expect("b"), None);
    assert_eq!(region.chunk_hash(0, 0).expect("hash"), None);
    assert!(region.dead_bytes() > 0, "orphaned frames are dead bytes");
}

#[test]
fn a_torn_newest_commit_slot_falls_back_to_the_older_one() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let a = key(0, 1, 0, 1);
    let b = key(0, 2, 0, 2);
    let mut region = open(&path);
    region.put(&record(a, 1, 3)).expect("put a");
    region.commit().expect("first commit");
    region.put(&record(b, 2, 3)).expect("put b");
    region.commit().expect("second commit");
    let newest = commit_slot_offset(region.seq);
    drop(region);
    flip_byte(&path, newest + 3);

    let mut region = open(&path);
    assert_eq!(region.recovery(), None);
    assert!(region.contains(a).expect("a"));
    assert!(!region.contains(b).expect("b"));
    // The next commit must not overwrite the slot that is still valid.
    region.put(&record(b, 3, 3)).expect("put b again");
    region.commit().expect("commit");
    drop(region);
    let region = open(&path);
    assert_eq!(region.recovery(), None);
    assert_eq!(region.get(b).expect("b"), Some(record(b, 3, 3)));
}

#[test]
fn a_corrupt_index_frame_is_rebuilt_by_scanning() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let keys: Vec<_> = (0..5).map(|i| key(0, i, 0, 0)).collect();
    let mut region = open(&path);
    for (i, &k) in keys.iter().enumerate() {
        region.put(&record(k, i as u32, 3)).expect("put");
    }
    region.set_chunk_hash(3, 4, Some(77)).expect("hash");
    region.commit().expect("commit");
    let index_offset = region.chain.last.expect("committed index");
    drop(region);
    flip_byte(&path, index_offset + FRAME_HEADER_LEN as u64 + 2);

    let mut region = open(&path);
    let recovery = region.recovery().expect("rebuilt").clone();
    assert!(matches!(
        recovery.fault,
        IndexFault::Frame {
            error: FrameError::Checksum { .. },
            ..
        }
    ));
    assert_eq!(recovery.records, keys.len());
    for (i, &k) in keys.iter().enumerate() {
        assert_eq!(region.get(k).expect("get"), Some(record(k, i as u32, 3)));
    }
    // The corrupt index frame held the only copy of the hash.
    assert_eq!(region.chunk_hash(3, 4).expect("hash"), None);
    assert!(region.is_dirty(), "a rebuilt index is committed in full");
    region.commit().expect("commit rebuilt index");
    drop(region);
    assert_eq!(open(&path).recovery(), None);
}

#[test]
fn a_truncated_file_keeps_the_records_before_the_cut() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let first = key(0, 1, 1, 1);
    let second = key(0, 2, 2, 2);
    let mut region = open(&path);
    region.put(&record(first, 1, 5)).expect("put first");
    let cut = region.len() + 40;
    region.put(&record(second, 2, 5)).expect("put second");
    region.commit().expect("commit");
    drop(region);
    truncate(&path, cut);

    let mut region = open(&path);
    let recovery = region.recovery().expect("rebuilt").clone();
    assert!(matches!(
        recovery.fault,
        IndexFault::Frame {
            error: FrameError::Truncated { .. },
            ..
        }
    ));
    assert_eq!(recovery.skipped_bytes, 40);
    assert_eq!(region.get(first).expect("first"), Some(record(first, 1, 5)));
    assert_eq!(region.get(second).expect("second"), None);

    region
        .put(&record(second, 3, 5))
        .expect("append after the torn tail");
    region.commit().expect("commit");
    drop(region);
    let region = open(&path);
    assert_eq!(region.recovery(), None);
    assert_eq!(
        region.get(second).expect("second"),
        Some(record(second, 3, 5))
    );
}

#[test]
fn corrupt_records_read_as_typed_errors_and_scans_skip_them() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let keys = [key(0, 1, 0, 0), key(0, 2, 0, 0), key(0, 3, 0, 0)];
    let mut region = open(&path);
    for (i, &k) in keys.iter().enumerate() {
        region.put(&record(k, i as u32, 3)).expect("put");
    }
    region.commit().expect("commit");
    let middle = region
        .index
        .record(RegionId::of_key(keys[1]).1)
        .expect("stored");
    drop(region);
    flip_byte(&path, middle.offset);
    flip_byte(&path, middle.offset + FRAME_HEADER_LEN as u64 + 30);

    let region = open(&path);
    assert!(matches!(
        region.get(keys[1]),
        Err(RegionError::Corrupt { key, error: FrameError::Magic(_), .. }) if key == keys[1]
    ));
    assert!(region.get(keys[0]).expect("first").is_some());
    assert!(region.get(keys[2]).expect("last").is_some());
    drop(region);

    wipe_commit_slots(&path);
    let region = open(&path);
    let recovery = region.recovery().expect("rebuilt").clone();
    assert_eq!(recovery.fault, IndexFault::NoCommit);
    assert_eq!(recovery.records, 2);
    assert!(recovery.skipped_bytes >= record_frame_len(middle));
    assert_eq!(region.get(keys[1]).expect("dropped"), None);
    assert_eq!(
        region.get(keys[2]).expect("resynced"),
        Some(record(keys[2], 2, 3))
    );
}

#[test]
fn tombstones_and_chunk_hashes_survive_a_scan_rebuild() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let (a, b) = (key(0, 1, 0, 0), key(0, 2, 0, 0));
    let mut region = open(&path);
    region.put(&record(a, 1, 3)).expect("put a");
    region.put(&record(b, 2, 3)).expect("put b");
    region.set_chunk_hash(0, 0, Some(1)).expect("hash");
    region.set_chunk_hash(63, 63, Some(2)).expect("hash");
    region.commit().expect("commit");
    assert!(region.remove(a).expect("remove a"));
    assert!(!region.remove(a).expect("already removed"));
    region.set_chunk_hash(0, 0, None).expect("clear hash");
    region.commit().expect("commit");
    drop(region);

    let region = open(&path);
    assert!(!region.contains(a).expect("a"));
    assert_eq!(region.chunk_hash(63, 63).expect("hash"), Some(2));
    drop(region);
    wipe_commit_slots(&path);

    let region = open(&path);
    assert!(region.recovery().is_some());
    assert!(!region.contains(a).expect("a stays removed"));
    assert!(region.contains(b).expect("b"));
    assert_eq!(region.chunk_hash(0, 0).expect("cleared"), None);
    assert_eq!(region.chunk_hash(63, 63).expect("kept"), Some(2));
}

#[test]
fn a_lost_index_never_brings_back_the_hashes_it_cleared() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let k = key(0, 1, 0, 0);
    let mut region = open(&path);
    region.put(&record(k, 1, 3)).expect("put");
    let chunks: Vec<_> = RegionId::of_key(k).1.chunk_slots().collect();
    let (x, z) = (2, 0);
    assert!(chunks.contains(&RegionId::of_chunk(x, z).1));
    region.set_chunk_hash(x, z, Some(11)).expect("hash");
    region.set_chunk_hash(63, 63, Some(12)).expect("hash");
    region.commit().expect("commit");
    // A read found the record corrupt: a tombstone, then an index clearing its hashes.
    region.drop_corrupt(k).expect("drop");
    region.commit().expect("commit");
    let index_offset = region.chain.last.expect("committed index");
    drop(region);
    // Power loss tears the last index; the tombstone before it survives.
    flip_byte(&path, index_offset + FRAME_HEADER_LEN as u64 + 2);

    let region = open(&path);
    assert!(region.recovery().is_some());
    assert!(!region.contains(k).expect("still removed"));
    assert_eq!(
        region.chunk_hash(x, z).expect("hash"),
        None,
        "the older index's hash would skip a chunk with no record"
    );
    assert_eq!(
        region.chunk_hash(63, 63).expect("hash"),
        None,
        "a lost index may have changed any hash"
    );
}

#[test]
fn chunk_hashes_belong_to_level_zero_regions() {
    let dir = TempDir::new().expect("temp dir");
    let mut region = open(&path_in(&dir));
    assert!(matches!(
        region.set_chunk_hash(64, 0, Some(1)),
        Err(RegionError::ChunkOutsideRegion { x: 64, z: 0, .. })
    ));
    let coarse = RegionId::new(1, 0, 0).expect("region");
    let mut region =
        RegionFile::open_writable(&dir.path().join(coarse.file_name()), coarse).expect("open");
    assert!(matches!(
        region.chunk_hash(0, 0),
        Err(RegionError::NotLevelZero(_))
    ));
    assert!(matches!(
        region.put(&record(key(0, 0, 0, 0), 1, 1)),
        Err(RegionError::KeyOutsideRegion { .. })
    ));
}

#[test]
fn delta_chains_stay_bounded_and_reload_exactly() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let mut region = open(&path);
    let mut longest = 0;
    let mut fulls = 0;
    for i in 0..200 {
        let k = key(0, i % 32, i / 32, 0);
        region.put(&record(k, i as u32, 1)).expect("put");
        region
            .set_chunk_hash(i % 64, 0, Some(i as u64))
            .expect("hash");
        region.commit().expect("commit");
        longest = longest.max(region.chain.deltas);
        fulls += usize::from(region.chain.deltas == 0);
    }
    assert!(longest <= MAX_INDEX_CHAIN);
    assert!(fulls > 1, "full indexes are rewritten as deltas accumulate");
    let expected: Vec<_> = (0..200).map(|i| key(0, i % 32, i / 32, 0)).collect();
    drop(region);

    let region = open(&path);
    assert_eq!(region.recovery(), None);
    assert_eq!(region.record_count(), expected.len());
    for (i, &k) in expected.iter().enumerate() {
        assert_eq!(region.get(k).expect("get"), Some(record(k, i as u32, 1)));
    }
    for x in 0..64 {
        let last = (0..200).filter(|i| i % 64 == x).max().expect("written");
        assert_eq!(region.chunk_hash(x, 0).expect("hash"), Some(last as u64));
    }
}

#[test]
fn index_entries_outside_the_file_trigger_a_rebuild() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let a = key(0, 1, 0, 0);
    let mut region = open(&path);
    region.put(&record(a, 1, 2)).expect("put");
    region.commit().expect("commit");
    // Commit a full index that points one entry far past the end of the file.
    let slot = RegionId::of_key(key(0, 9, 0, 0)).1;
    region.index.set_record(
        slot,
        RecordEntry {
            offset: 1 << 40,
            len: 64,
        },
    );
    region.force_full = true;
    region.commit().expect("commit bad index");
    drop(region);

    let region = open(&path);
    let recovery = region.recovery().expect("rebuilt");
    assert_eq!(
        recovery.fault,
        IndexFault::EntryOutOfBounds { slot: slot.raw() }
    );
    assert_eq!(region.get(a).expect("a"), Some(record(a, 1, 2)));
    assert_eq!(region.record_count(), 1);
}

#[test]
fn compaction_rewrites_live_data_once_most_bytes_are_dead() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let keys: Vec<_> = (0..64).map(|i| key(0, i % 32, i / 32, 3)).collect();
    let mut region = open(&path);
    region.set_chunk_hash(5, 6, Some(56)).expect("hash");
    for round in 0..4 {
        for (i, &k) in keys.iter().enumerate() {
            region
                .put(&record(k, (round * 100 + i) as u32, 12))
                .expect("put");
        }
        region.commit().expect("commit");
    }
    assert!(
        region.needs_compaction(),
        "dead {} of {}",
        region.dead_bytes(),
        region.len()
    );
    let compaction = region.compact().expect("compact");
    assert!(compaction.after * 3 < compaction.before);
    assert!(compaction.dropped.is_empty());
    assert_eq!(region.dead_bytes(), 0);
    assert_eq!(fs::metadata(&path).expect("file").len(), compaction.after);
    assert!(!temp_sibling(&path).exists());

    region
        .put(&record(keys[0], 999, 12))
        .expect("put after compaction");
    region.commit().expect("commit after compaction");
    drop(region);
    let region = open(&path);
    assert_eq!(region.recovery(), None);
    assert_eq!(
        region.get(keys[0]).expect("first"),
        Some(record(keys[0], 999, 12))
    );
    for (i, &k) in keys.iter().enumerate().skip(1) {
        assert_eq!(
            region.get(k).expect("get"),
            Some(record(k, (300 + i) as u32, 12))
        );
    }
    assert_eq!(region.chunk_hash(5, 6).expect("hash"), Some(56));
}

#[test]
fn compaction_drops_corrupt_records_and_reports_them() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let (a, b) = (key(0, 1, 0, 0), key(0, 2, 0, 0));
    let mut region = open(&path);
    region.put(&record(a, 1, 3)).expect("put a");
    region.put(&record(b, 2, 3)).expect("put b");
    region.commit().expect("commit");
    let entry = region.index.record(RegionId::of_key(a).1).expect("a");
    flip_byte(&path, entry.offset + FRAME_HEADER_LEN as u64 + 40);

    let compaction = region.compact().expect("compact");
    assert_eq!(compaction.dropped, vec![a]);
    assert!(!region.contains(a).expect("a"));
    assert_eq!(region.get(b).expect("b"), Some(record(b, 2, 3)));
}

#[test]
fn opening_removes_a_leftover_compaction_sibling() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    fs::write(temp_sibling(&path), b"partial").expect("leftover");
    drop(open(&path));
    assert!(!temp_sibling(&path).exists());
}

#[test]
fn read_only_handles_never_write() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    assert!(
        RegionFile::open_read_only(&path, region0())
            .expect("open")
            .is_none()
    );
    let a = key(0, 1, 0, 0);
    let mut region = open(&path);
    region.put(&record(a, 1, 3)).expect("put");
    region.commit().expect("commit");
    drop(region);
    wipe_commit_slots(&path);
    let before = fs::read(&path).expect("bytes");

    let mut region = RegionFile::open_read_only(&path, region0())
        .expect("open")
        .expect("exists");
    assert!(region.recovery().is_some());
    assert_eq!(region.get(a).expect("a"), Some(record(a, 1, 3)));
    assert!(!region.is_dirty(), "a read-only rebuild is never committed");
    assert!(matches!(
        region.put(&record(a, 2, 3)),
        Err(RegionError::ReadOnly)
    ));
    assert!(matches!(region.remove(a), Err(RegionError::ReadOnly)));
    assert!(matches!(
        region.set_chunk_hash(0, 0, Some(1)),
        Err(RegionError::ReadOnly)
    ));
    assert!(matches!(region.compact(), Err(RegionError::ReadOnly)));
    assert!(!region.commit().expect("nothing to commit"));
    region.sync().expect("sync is a no-op");
    drop(region);
    assert_eq!(fs::read(&path).expect("bytes"), before);
}

#[test]
fn malformed_headers_are_typed_errors() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    drop(open(&path));
    let header = |path: &Path, id| match RegionFile::open_writable(path, id) {
        Err(RegionError::Header(error)) => error,
        other => panic!("expected a header error, got {other:?}"),
    };
    let elsewhere = RegionId::new(0, 1, 0).expect("region");
    assert!(matches!(
        header(&path, elsewhere),
        HeaderError::Identity { .. }
    ));
    flip_byte(&path, 9);
    assert!(matches!(
        header(&path, region0()),
        HeaderError::Checksum { .. }
    ));
    flip_byte(&path, 0);
    assert!(matches!(header(&path, region0()), HeaderError::Magic(_)));
    truncate(&path, 10);
    assert_eq!(header(&path, region0()), HeaderError::Truncated(10));

    let mut region = RegionFile::create(&path, region0()).expect("replace");
    assert!(region.is_empty());
    region
        .put(&record(key(0, 0, 0, 0), 1, 1))
        .expect("usable again");
}

#[test]
fn invalid_records_are_rejected_before_writing() {
    let dir = TempDir::new().expect("temp dir");
    let mut region = open(&path_in(&dir));
    let mut bad = record(key(0, 1, 0, 0), 1, 3);
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert!(matches!(region.put(&bad), Err(RegionError::Record(_))));
    assert!(matches!(region.put(b"short"), Err(RegionError::Record(_))));
    assert!(!region.is_dirty());
}

#[test]
fn reads_see_buffered_and_written_frames_before_a_commit() {
    let dir = TempDir::new().expect("temp dir");
    let mut region = open(&path_in(&dir));
    let keys: Vec<_> = (0..256).map(|i| key(0, i % 32, i / 32, 0)).collect();
    for (i, &k) in keys.iter().enumerate() {
        region.put(&record(k, i as u32, 30)).expect("put");
    }
    assert!(
        region.written_end > DATA_START,
        "large batches are written early"
    );
    assert!(!region.pending.is_empty(), "the tail is still buffered");
    for (i, &k) in keys.iter().enumerate() {
        assert_eq!(region.get(k).expect("get"), Some(record(k, i as u32, 30)));
    }
}

/// A barrier that records the file's length on disk at each call, and can fail.
#[derive(Debug)]
struct Recording {
    path: PathBuf,
    lengths: std::sync::Mutex<Vec<u64>>,
    fail: std::sync::atomic::AtomicBool,
}

impl Recording {
    fn new(path: &Path) -> Arc<Self> {
        Arc::new(Self {
            path: path.to_owned(),
            lengths: Default::default(),
            fail: Default::default(),
        })
    }

    fn lengths(&self) -> Vec<u64> {
        self.lengths.lock().expect("lengths").clone()
    }
}

impl WriteBarrier for Recording {
    fn before_write(&self) -> io::Result<()> {
        let len = fs::metadata(&self.path)?.len();
        self.lengths.lock().expect("lengths").push(len);
        if self.fail.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(io::Error::other("barrier failed"));
        }
        Ok(())
    }
}

#[test]
fn the_barrier_runs_before_any_frame_reaches_disk() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let mut region = open(&path);
    let barrier = Recording::new(&path);
    region.set_barrier(barrier.clone());
    // Enough for the write buffer to fill before any commit.
    for i in 0..256 {
        region
            .put(&record(key(0, i % 32, i / 32, 0), i as u32, 30))
            .expect("put");
    }
    assert!(region.written_end > DATA_START, "written before a commit");
    assert_eq!(
        barrier.lengths().first(),
        Some(&DATA_START),
        "the barrier ran before the first frame was written"
    );
    let calls = barrier.lengths().len();
    region.commit().expect("commit");
    assert_eq!(barrier.lengths().len(), calls + 1, "and before the commit");
}

#[test]
fn failed_writes_leave_the_file_and_buffer_as_they_were() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    let mut region = open(&path);
    let barrier = Recording::new(&path);
    region.set_barrier(barrier.clone());
    let stored = key(0, 0, 0, 0);
    region.put(&record(stored, 1, 3)).expect("put");
    region.commit().expect("commit");
    barrier
        .fail
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let mut failures = 0;
    for i in 0..512 {
        let k = key(0, i % 32, 1 + i / 32, 0);
        if region.put(&record(k, i as u32, 30)).is_err() {
            failures += 1;
            assert!(
                !region.contains(k).expect("contains"),
                "a failed put is undone"
            );
        }
        assert!(
            region.pending.len() < PENDING_WRITE_BYTES,
            "failed writes never grow the buffer"
        );
    }
    assert!(failures > 0);
    let pending = region.pending.len();
    assert!(region.commit().is_err());
    assert!(region.commit().is_err());
    assert_eq!(
        region.pending.len(),
        pending,
        "failed commits add no index frames"
    );
    assert!(
        region.contains(stored).expect("contains"),
        "committed data stays"
    );

    barrier
        .fail
        .store(false, std::sync::atomic::Ordering::Relaxed);
    region.commit().expect("commit once writes succeed");
    let expected = state_keys(&region);
    drop(region);
    let region = open(&path);
    assert_eq!(region.recovery(), None);
    assert_eq!(state_keys(&region), expected);
}

fn state_keys(region: &RegionFile) -> Vec<u64> {
    let mut keys: Vec<_> = region.keys().map(SectionKey::bits).collect();
    keys.sort_unstable();
    keys
}

#[test]
fn corrupt_level_zero_records_take_their_chunk_hashes_with_them() {
    let dir = TempDir::new().expect("temp dir");
    let path = path_in(&dir);
    // The level-0 section at x 1 covers chunks 2 and 3 along x, 0 and 1 along z.
    let (lost, kept) = (key(0, 1, 0, 0), key(0, 20, 0, 20));
    let mut region = open(&path);
    region.put(&record(lost, 1, 3)).expect("put lost");
    region.put(&record(kept, 2, 3)).expect("put kept");
    for (x, z) in [(2, 0), (3, 1), (40, 40)] {
        region.set_chunk_hash(x, z, Some(7)).expect("hash");
    }
    region.commit().expect("commit");
    let entry = region.index.record(RegionId::of_key(lost).1).expect("lost");
    drop(region);
    flip_byte(&path, entry.offset + FRAME_HEADER_LEN as u64 + 30);
    let unvouched = |region: &RegionFile| {
        (
            region.chunk_hash(2, 0).expect("hash"),
            region.chunk_hash(3, 1).expect("hash"),
            region.chunk_hash(40, 40).expect("hash"),
        )
    };

    // A read that finds it corrupt.
    let mut region = open(&path);
    assert!(matches!(region.get(lost), Err(RegionError::Corrupt { .. })));
    region.drop_corrupt(lost).expect("drop");
    assert_eq!(region.get(lost).expect("forgotten"), None);
    assert_eq!(unvouched(&region), (None, None, Some(7)));
    drop(region);

    // A scan that cannot read it.
    wipe_commit_slots(&path);
    let region = open(&path);
    assert!(region.recovery().is_some());
    assert_eq!(region.get(lost).expect("skipped"), None);
    assert_eq!(unvouched(&region), (None, None, Some(7)));
    drop(region);

    // Compaction that drops it.
    let mut region = open(&path);
    region.commit().expect("commit rebuilt index");
    drop(region);
    let mut region = open(&path);
    region.put(&record(lost, 3, 3)).expect("put again");
    region.set_chunk_hash(2, 0, Some(8)).expect("hash");
    region.commit().expect("commit");
    let entry = region.index.record(RegionId::of_key(lost).1).expect("lost");
    flip_byte(&path, entry.offset + FRAME_HEADER_LEN as u64 + 30);
    let compaction = region.compact().expect("compact");
    assert_eq!(compaction.dropped, vec![lost]);
    assert_eq!(unvouched(&region), (None, None, Some(7)));
    assert!(region.contains(kept).expect("kept"));
}

mod crash;

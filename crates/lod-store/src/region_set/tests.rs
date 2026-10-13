use tempfile::TempDir;

use super::*;
use crate::{
    region_file::DATA_START,
    test_support::{flip_byte, key, record},
};

fn capacity(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("non-zero")
}

#[test]
fn records_spread_across_regions_and_survive_handle_eviction() {
    let dir = TempDir::new().expect("temp dir");
    let mut set = RegionSet::open(dir.path().join("dim0"), Access::ReadWrite, capacity(2));
    let keys: Vec<_> = (0..5i32)
        .map(|i| key((i % 2) as u8, i * 40, 0, -i * 40))
        .collect();
    for (i, &k) in keys.iter().enumerate() {
        assert_eq!(set.put(&record(k, i as u32, 3)).expect("put"), k);
        assert!(set.cached_count() <= 2);
    }
    set.set_chunk_hash(-100, 7, Some(5)).expect("hash");
    // Evicted handles were committed, so every record is readable again.
    for (i, &k) in keys.iter().enumerate() {
        assert_eq!(set.get(k).expect("get"), Some(record(k, i as u32, 3)));
    }
    assert!(set.drain_events().is_empty());
    assert!(set.close().is_empty());

    let mut set = RegionSet::open(dir.path().join("dim0"), Access::ReadWrite, capacity(8));
    assert_eq!(set.region_ids().expect("list").len(), keys.len() + 1);
    for (i, &k) in keys.iter().enumerate() {
        assert_eq!(set.get(k).expect("get"), Some(record(k, i as u32, 3)));
    }
    assert_eq!(set.chunk_hash(-100, 7).expect("hash"), Some(5));
    assert_eq!(set.chunk_hash(100, 7).expect("no file"), None);
}

#[test]
fn dropping_without_committing_discards_changes() {
    let dir = TempDir::new().expect("temp dir");
    let a = key(0, 1, 0, 0);
    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, DEFAULT_OPEN_REGIONS);
    set.put(&record(a, 1, 3)).expect("put");
    drop(set);
    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, DEFAULT_OPEN_REGIONS);
    assert_eq!(set.get(a).expect("get"), None);
}

#[test]
fn read_only_sets_never_create_or_change_files() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().join("dim");
    let mut reader = RegionSet::open(&root, Access::ReadOnly, capacity(4));
    let a = key(0, 1, 0, 0);
    assert_eq!(reader.get(a).expect("absent"), None);
    assert!(!root.exists());
    assert!(matches!(
        reader.put(&record(a, 1, 1)),
        Err(RegionError::ReadOnly)
    ));
    assert!(matches!(reader.remove(a), Err(RegionError::ReadOnly)));
    assert!(matches!(
        reader.delete_region(RegionId::of_key(a).0),
        Err(RegionError::ReadOnly)
    ));

    let mut writer = RegionSet::open(&root, Access::ReadWrite, capacity(4));
    writer.put(&record(a, 1, 1)).expect("put");
    assert!(writer.commit_all());
    let mut reader = RegionSet::open(&root, Access::ReadOnly, capacity(4));
    assert_eq!(reader.get(a).expect("get"), Some(record(a, 1, 1)));
    assert!(reader.sync_all());
}

#[test]
fn unusable_headers_are_replaced_by_writers_and_skipped_by_readers() {
    let dir = TempDir::new().expect("temp dir");
    let a = key(0, 1, 0, 0);
    let region = RegionId::of_key(a).0;
    let mut writer = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    writer.put(&record(a, 1, 1)).expect("put");
    assert!(writer.close().is_empty());
    let path = dir.path().join(region.file_name());
    flip_byte(&path, 0);

    let mut reader = RegionSet::open(dir.path(), Access::ReadOnly, capacity(4));
    assert_eq!(reader.get(a).expect("unusable reads as absent"), None);
    assert!(matches!(
        reader.drain_events().as_slice(),
        [RegionEvent::Unusable {
            replaced: false,
            ..
        }]
    ));

    let mut writer = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    assert_eq!(writer.get(a).expect("replaced"), None);
    assert!(matches!(
        writer.drain_events().as_slice(),
        [RegionEvent::Unusable { replaced: true, .. }]
    ));
    assert_eq!(fs::metadata(&path).expect("file").len(), DATA_START);
}

#[test]
fn recoveries_are_reported_once() {
    let dir = TempDir::new().expect("temp dir");
    let a = key(0, 1, 0, 0);
    let mut writer = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    writer.put(&record(a, 1, 1)).expect("put");
    assert!(writer.close().is_empty());
    let path = dir.path().join(RegionId::of_key(a).0.file_name());
    let len = fs::metadata(&path).expect("file").len();
    flip_byte(&path, len - 1);

    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    assert!(set.contains(a).expect("recovered"));
    assert!(matches!(
        set.drain_events().as_slice(),
        [RegionEvent::Recovered { .. }]
    ));
    assert!(set.contains(a).expect("cached"));
    assert!(set.drain_events().is_empty());
}

#[test]
fn deleting_a_region_frees_its_file() {
    let dir = TempDir::new().expect("temp dir");
    let a = key(0, 1, 0, 0);
    let region = RegionId::of_key(a).0;
    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    set.put(&record(a, 1, 1)).expect("put");
    assert!(set.commit_all());
    let len = fs::metadata(set.region_path(region)).expect("file").len();
    assert_eq!(set.delete_region(region).expect("delete"), len);
    assert_eq!(set.delete_region(region).expect("already gone"), 0);
    assert_eq!(set.get(a).expect("get"), None);
    assert!(!set.remove(key(0, 900, 0, 0)).expect("no file to create"));
    assert!(
        set.region_ids().expect("list").is_empty(),
        "reading a deleted region never recreates it"
    );
}

#[test]
fn reads_of_absent_regions_create_nothing_until_a_write() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().join("e0");
    let mut set = RegionSet::open(&root, Access::ReadWrite, capacity(2));
    let probes: Vec<_> = (0..8i32)
        .map(|i| key((i % 3) as u8, i * 40, 0, 0))
        .collect();
    for &probe in &probes {
        assert_eq!(set.get(probe).expect("get"), None);
        assert!(!set.contains(probe).expect("contains"));
    }
    assert_eq!(set.chunk_hash(-500, 9).expect("hash"), None);
    assert!(!root.exists(), "misses leave no directory or files");

    // A remembered absence still gives way to the first write.
    set.put(&record(probes[7], 1, 2)).expect("put");
    assert_eq!(
        set.get(probes[7]).expect("get"),
        Some(record(probes[7], 1, 2))
    );
    assert!(set.commit_all());
    assert_eq!(
        set.region_ids().expect("list"),
        vec![RegionId::of_key(probes[7]).0]
    );
}

#[test]
fn listing_ignores_foreign_and_temporary_files() {
    let dir = TempDir::new().expect("temp dir");
    let set = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    let region = RegionId::new(3, -2, 5).expect("region");
    fs::write(dir.path().join(region.file_name()), b"").expect("region");
    fs::write(dir.path().join("r.3.-2.5.tmp"), b"").expect("temp");
    fs::write(dir.path().join("notes.txt"), b"").expect("foreign");
    assert_eq!(set.region_ids().expect("list"), vec![region]);
}

#[test]
fn a_corrupt_read_is_reported_once_and_its_chunks_are_sampled_again() {
    let dir = TempDir::new().expect("temp dir");
    let a = key(0, 1, 0, 0);
    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    set.put(&record(a, 1, 3)).expect("put");
    set.set_chunk_hash(2, 0, Some(5)).expect("hash");
    assert!(set.close().is_empty());
    let path = dir.path().join(RegionId::of_key(a).0.file_name());
    // Inside the record's payload, past the frame header after the fixed file header.
    flip_byte(&path, DATA_START + 40);

    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    assert!(matches!(set.get(a), Err(RegionError::Corrupt { .. })));
    assert_eq!(set.get(a).expect("forgotten"), None);
    assert_eq!(set.chunk_hash(2, 0).expect("hash"), None);
    assert!(set.close().is_empty());
    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    assert_eq!(set.chunk_hash(2, 0).expect("hash"), None, "committed");
}

/// Overwrites every record of a level-0 region four times, committing each round, so
/// most of its file is dead.
fn churn(set: &mut RegionSet, region_x: i32) {
    let keys: Vec<_> = (0..64)
        .map(|i| key(0, region_x * 32 + i % 32, i / 32, 3))
        .collect();
    for round in 0..4 {
        for (i, &k) in keys.iter().enumerate() {
            set.put(&record(k, round * 100 + i as u32, 12))
                .expect("put");
        }
        assert!(set.commit_levels(0));
    }
}

#[test]
fn compaction_per_commit_stays_within_its_budget() {
    let dir = TempDir::new().expect("temp dir");
    let mut set =
        RegionSet::open(dir.path(), Access::ReadWrite, capacity(4)).with_compaction_budget(1);
    churn(&mut set, 0);
    churn(&mut set, 5);
    let compacted = |set: &mut RegionSet| {
        set.drain_events()
            .into_iter()
            .filter(|event| matches!(event, RegionEvent::Compacted { .. }))
            .count()
    };
    assert!(set.commit_all());
    assert_eq!(compacted(&mut set), 1, "one file past a tiny budget");
    assert!(set.commit_all());
    assert_eq!(compacted(&mut set), 1, "the other on the next commit");
    assert!(set.commit_all());
    assert_eq!(compacted(&mut set), 0);
}

#[test]
fn evicting_a_level_zero_file_commits_the_coarser_files_first() {
    let dir = TempDir::new().expect("temp dir");
    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, capacity(2));
    let fine = key(0, 0, 0, 0);
    let coarse = key(1, 0, 0, 0);
    set.put(&record(fine, 1, 3)).expect("put");
    set.set_chunk_hash(0, 0, Some(9)).expect("hash");
    set.put(&record(coarse, 2, 3)).expect("put");
    // A third region evicts the level-0 file, whose hash vouches for the mip.
    set.put(&record(key(0, 32 * 7, 0, 0), 3, 3)).expect("put");
    // The process dies before any flush.
    drop(set);

    let mut set = RegionSet::open(dir.path(), Access::ReadOnly, capacity(4));
    assert_eq!(set.chunk_hash(0, 0).expect("hash"), Some(9));
    assert_eq!(
        set.get(coarse).expect("get"),
        Some(record(coarse, 2, 3)),
        "the mip the hash vouches for was committed before it"
    );
}

#[test]
fn closing_the_least_recent_file_frees_its_descriptor() {
    let dir = TempDir::new().expect("temp dir");
    let mut set = RegionSet::open(dir.path(), Access::ReadWrite, capacity(4));
    assert!(!set.close_least_recent(), "nothing open");
    set.put(&record(key(0, 0, 0, 0), 1, 3)).expect("put");
    assert!(!set.contains(key(0, 32 * 3, 0, 0)).expect("absent"));
    assert_eq!((set.cached_count(), set.open_files()), (2, 1));
    assert!(set.close_least_recent());
    assert_eq!(set.open_files(), 0);
    assert_eq!(
        set.get(key(0, 0, 0, 0)).expect("get"),
        Some(record(key(0, 0, 0, 0), 1, 3)),
        "committed on close"
    );
}

use super::*;
use crate::test_support::{flip_byte, surface};

fn cache(dir: &Path, access: Access, capacity: usize) -> SignatureCache {
    SignatureCache::new(dir, access, NonZeroUsize::new(capacity).expect("capacity"))
}

#[test]
fn signatures_persist_per_storage_region() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut writer = cache(dir.path(), Access::ReadWrite, 4);
    // Chunks in two storage regions, one of them at negative coordinates.
    writer.record(1, 2, surface(1, 1, 2));
    writer.record(-70, 5, surface(1, -70, 5));
    assert!(writer.flush());
    assert_eq!(
        writer.file_regions().expect("list"),
        [
            RegionId::new(0, -2, 0).expect("region"),
            RegionId::new(0, 0, 0).expect("region")
        ]
    );
    let mut reader = cache(dir.path(), Access::ReadOnly, 4);
    let stored = reader
        .get(RegionPos::of_chunk(-70, 5))
        .expect("stored signature");
    assert_eq!(stored.chunk(-70, 5), Some(&surface(1, -70, 5)));
    assert!(reader.get(RegionPos::of_chunk(500, 500)).is_none());
    assert_eq!(reader.all().expect("all").len(), 2);
}

#[test]
fn eviction_writes_changes_and_read_only_caches_never_write() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut reader = cache(dir.path(), Access::ReadOnly, 1);
    reader.record(0, 0, surface(1, 0, 0));
    reader.record(200, 0, surface(1, 200, 0));
    assert!(reader.flush());
    assert!(reader.file_regions().expect("list").is_empty());

    let mut writer = cache(dir.path(), Access::ReadWrite, 1);
    writer.record(0, 0, surface(1, 0, 0));
    writer.record(200, 0, surface(1, 200, 0));
    assert_eq!(
        writer.file_regions().expect("list").len(),
        1,
        "evicted file written"
    );
    assert!(writer.get(RegionPos::of_chunk(0, 0)).is_some());
}

#[test]
fn damaged_files_read_as_empty_and_are_reported() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut writer = cache(dir.path(), Access::ReadWrite, 4);
    writer.record(3, 3, surface(2, 3, 3));
    assert!(writer.flush());
    let path = dir
        .path()
        .join(signature_file_name(RegionId::new(0, 0, 0).expect("region")));
    flip_byte(&path, 20);
    let mut reader = cache(dir.path(), Access::ReadOnly, 4);
    assert!(reader.get(RegionPos::of_chunk(3, 3)).is_none());
    assert!(matches!(
        reader.drain_events().as_slice(),
        [SignatureEvent::Unreadable { .. }]
    ));
}

#[test]
fn merging_keeps_both_sides_and_deleting_frees_the_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut target = cache(dir.path(), Access::ReadWrite, 4);
    target.record(0, 0, surface(1, 0, 0));
    let mut newer = RegionSignature::new(RegionPos::of_chunk(1, 0));
    newer.record(1, 0, surface(1, 1, 0));
    target.merge(&newer);
    let merged = target.get(RegionPos::of_chunk(0, 0)).expect("merged");
    assert_eq!(merged.recorded_chunks().count(), 2);
    assert!(target.flush());
    let region = RegionId::new(0, 0, 0).expect("region");
    assert!(target.delete(region).expect("delete") > 0);
    assert!(target.file_regions().expect("list").is_empty());
    assert!(target.get(RegionPos::of_chunk(0, 0)).is_none());
}

#[test]
fn file_names_round_trip_canonically() {
    let region = RegionId::new(0, -3, 7).expect("region");
    assert_eq!(signature_region(&signature_file_name(region)), Some(region));
    for name in ["s.1.2", "s.01.2.lods", "s.1.2.3.lods", "r.0.1.2.lodr"] {
        assert_eq!(signature_region(name), None, "{name}");
    }
}

#[test]
fn a_file_that_failed_to_read_is_merged_into_never_overwritten() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut writer = cache(dir.path(), Access::ReadWrite, 4);
    writer.record(0, 0, surface(1, 0, 0));
    assert!(writer.flush());
    let path = dir
        .path()
        .join(signature_file_name(file_region(RegionPos::of_chunk(0, 0))));
    let stored = std::fs::read(&path).expect("stored file");
    drop(writer);
    // A directory in its place makes every read fail, as a transient IO error would.
    std::fs::remove_file(&path).expect("remove");
    std::fs::create_dir(&path).expect("unreadable");

    let mut writer = cache(dir.path(), Access::ReadWrite, 4);
    writer.record(1, 0, surface(2, 1, 0));
    assert!(matches!(
        writer.drain_events().as_slice(),
        [SignatureEvent::ReadFailed { .. }]
    ));
    assert!(!writer.flush(), "nothing is written while the read fails");
    assert!(path.is_dir());

    std::fs::remove_dir(&path).expect("readable again");
    std::fs::write(&path, &stored).expect("stored file back");
    assert!(writer.flush());
    let mut reader = cache(dir.path(), Access::ReadOnly, 4);
    let signature = reader.get(RegionPos::of_chunk(0, 0)).expect("signature");
    assert_eq!(
        signature.chunk(0, 0),
        Some(&surface(1, 0, 0)),
        "stored kept"
    );
    assert_eq!(
        signature.chunk(1, 0),
        Some(&surface(2, 1, 0)),
        "new recorded"
    );
}

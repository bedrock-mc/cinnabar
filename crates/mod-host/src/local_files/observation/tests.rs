use super::*;
use std::io::Write;

#[test]
fn unchanged_observations_do_not_read_or_copy_source_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    std::fs::write(&path, b"original").unwrap();
    let mut cache = ObservationCache::default();
    let mut reads = 0;
    let now = Instant::now();
    let first = cache
        .read_at(&path, 8, now, Identity::of, |opened| {
            reads += 1;
            opened.read(8)
        })
        .unwrap();
    cache.retain_observed();
    for _ in 0..8 {
        let again = cache
            .read_at(&path, 8, now, Identity::of, |opened| {
                reads += 1;
                opened.read(8)
            })
            .unwrap();
        assert!(Arc::ptr_eq(&first.bytes, &again.bytes));
        assert_eq!(first.digest, again.digest);
        cache.retain_observed();
    }
    assert_eq!(reads, 1, "unchanged polls must not invoke the byte reader");
    assert!(
        cache.read(&path, 7).is_err(),
        "a cached read cannot bypass a smaller bound"
    );
}

#[test]
fn same_size_in_place_edits_with_restored_mtime_are_observed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    std::fs::write(&path, b"original").unwrap();
    let mut cache = ObservationCache::default();
    let now = Instant::now();
    let first = cache
        .read_at(&path, 8, now, Identity::of, |opened| opened.read(8))
        .unwrap();
    let mut writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    let modified = writer.metadata().unwrap().modified().unwrap();
    writer.write_all(b"modified").unwrap();
    writer
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    drop(writer);
    let changed = cache
        .read_at(&path, 8, now + CONFIRMATION_DELAY, Identity::of, |opened| {
            opened.read(8)
        })
        .unwrap();
    assert_eq!(changed.bytes.as_ref(), b"modified");
    assert_ne!(first.digest, changed.digest);
    assert_eq!(
        first.bytes.as_ref(),
        b"original",
        "captured snapshots stay immutable"
    );
}

#[test]
fn same_size_replacements_and_missing_sources_do_not_reuse_old_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    let replacement = root.path().join("replacement");
    std::fs::write(&path, b"old").unwrap();
    let mut cache = ObservationCache::default();
    let first = cache.read(&path, 3).unwrap();
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&replacement, b"new").unwrap();
    File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    assert_eq!(cache.read(&path, 3).unwrap().bytes.as_ref(), b"new");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        cache.read(&path, 3).err().unwrap().kind(),
        io::ErrorKind::NotFound
    );
    std::fs::write(&path, b"old").unwrap();
    assert!(!Arc::ptr_eq(
        &first.bytes,
        &cache.read(&path, 3).unwrap().bytes
    ));
}

#[test]
fn an_edit_during_read_is_retried_instead_of_caching_a_mixed_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    std::fs::write(&path, b"old").unwrap();
    let mut cache = ObservationCache::default();
    let result = cache.read_with(&path, 3, |opened| {
        let bytes = opened.read(3)?;
        let modified = opened.file.metadata()?.modified()?;
        std::fs::write(&path, b"new")?;
        File::options()
            .write(true)
            .open(&path)?
            .set_times(std::fs::FileTimes::new().set_modified(modified + Duration::from_secs(1)))?;
        Ok(bytes)
    });
    assert!(result.is_err());
    assert_eq!(cache.read(&path, 3).unwrap().bytes.as_ref(), b"new");
}

#[test]
fn unused_observations_release_their_retained_buffers() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    std::fs::write(&path, b"old").unwrap();
    let mut cache = ObservationCache::default();
    let first = cache.read(&path, 3).unwrap();
    cache.retain_observed();
    cache.retain_observed();
    assert_eq!(Arc::strong_count(&first.bytes), 1);
    assert!(!Arc::ptr_eq(
        &first.bytes,
        &cache.read(&path, 3).unwrap().bytes
    ));
}

#[test]
#[cfg(unix)]
fn directory_aliases_can_change_but_cached_leaf_links_are_refused() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    for (name, bytes) in [("first", b"old"), ("second", b"new")] {
        std::fs::create_dir(root.path().join(name)).unwrap();
        std::fs::write(root.path().join(name).join("source"), bytes).unwrap();
    }
    let alias = root.path().join("alias");
    symlink(root.path().join("first"), &alias).unwrap();
    let mut cache = ObservationCache::default();
    let path = alias.join("source");
    assert_eq!(cache.read(&path, 3).unwrap().bytes.as_ref(), b"old");
    std::fs::remove_file(&alias).unwrap();
    symlink(root.path().join("second"), &alias).unwrap();
    assert_eq!(cache.read(&path, 3).unwrap().bytes.as_ref(), b"new");
    std::fs::remove_file(&path).unwrap();
    symlink(root.path().join("first/source"), &path).unwrap();
    assert!(cache.read(&path, 3).is_err());
}

#[test]
fn unchanged_bounded_read_failures_do_not_reread_bytes_and_repairs_retry() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    std::fs::write(&path, b"large").unwrap();
    let mut cache = ObservationCache::default();
    let mut reads = 0;
    for _ in 0..8 {
        assert!(
            cache
                .read_with(&path, 3, |opened| {
                    reads += 1;
                    opened.read(3)
                })
                .is_err()
        );
    }
    assert_eq!(
        reads, 0,
        "known oversized files must not invoke the byte reader"
    );
    std::fs::write(&path, b"new").unwrap();
    assert_eq!(
        cache
            .read_with(&path, 3, |opened| {
                reads += 1;
                opened.read(3)
            })
            .unwrap()
            .bytes
            .as_ref(),
        b"new"
    );
    assert_eq!(reads, 1);
}

#[test]
fn transient_read_errors_retry_without_a_source_edit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    std::fs::write(&path, b"old").unwrap();
    let mut cache = ObservationCache::default();
    assert!(
        cache
            .read_with(&path, 3, |_| Err(io::Error::other(
                "temporary read failure"
            )))
            .is_err()
    );
    assert_eq!(cache.read(&path, 3).unwrap().bytes.as_ref(), b"old");
}

#[test]
fn same_stamp_edits_are_confirmed_once_without_periodic_source_reads() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    std::fs::write(&path, b"old").unwrap();
    let identity = Identity::of(&File::open(&path).unwrap()).unwrap().unwrap();
    let now = Instant::now();
    let mut cache = ObservationCache::default();
    let reads = std::cell::Cell::new(0);
    let observe = |cache: &mut ObservationCache, now| {
        cache
            .read_at(
                &path,
                3,
                now,
                |_| Ok(Some(identity.clone())),
                |opened| {
                    reads.set(reads.get() + 1);
                    opened.read(3)
                },
            )
            .unwrap()
    };
    let first = observe(&mut cache, now);
    std::fs::write(&path, b"new").unwrap();
    let confirmed = observe(&mut cache, now + CONFIRMATION_DELAY);
    assert_eq!(
        confirmed.bytes.as_ref(),
        b"new",
        "a repeated timestamp cannot retain old bytes forever"
    );
    assert_eq!(first.bytes.as_ref(), b"old");
    for seconds in [2, 10, 100, 1000] {
        assert!(Arc::ptr_eq(
            &confirmed.bytes,
            &observe(&mut cache, now + Duration::from_secs(seconds)).bytes
        ));
    }
    assert_eq!(
        reads.get(),
        2,
        "the confirmation is not a periodic reread loop"
    );
}

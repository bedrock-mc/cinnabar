use super::*;

/// Builds an original synthetic pack whose identity and bytes can change independently.
fn archive(id: Uuid, text: &str) -> Vec<u8> {
    let mut bytes = crate::test_support::archive_bytes(
        id,
        &[("texts/example.lang".into(), text.as_bytes().into())],
    );
    let length = bytes.len();
    let comment = b"original ZIP comment";
    bytes[length - 2..].copy_from_slice(&(comment.len() as u16).to_le_bytes());
    bytes.extend_from_slice(comment);
    bytes
}

#[test]
fn persistent_cache_verifies_original_bytes_recovers_corruption_and_bounds_storage() {
    let root = tempfile::tempdir().unwrap();
    let cancelled = AtomicBool::new(false);
    let first = archive(Uuid::from_u128(1), "first");
    let key = ArchiveCacheKey::new(Uuid::from_u128(1), "1.0.0", first.len() as u64).unwrap();
    let cache = ArchiveCache::open(root.path(), first.len() as u64).unwrap();
    cache.store(&key, &first, &cancelled).unwrap();
    assert_eq!(
        cache
            .load(&key, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap(),
        Some(first.clone())
    );
    assert!(ArchiveCache::open(root.path(), DEFAULT_CACHE_QUOTA).is_err());
    drop(cache);
    let cache = ArchiveCache::open(root.path(), first.len() as u64).unwrap();
    assert_eq!(
        cache
            .load(&key, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap(),
        Some(first.clone())
    );
    let object = cache.objects().unwrap().pop().unwrap();
    let mut corrupt = first.clone();
    corrupt[0] ^= 1;
    std::fs::write(root.path().join("native-v1").join(object.name), corrupt).unwrap();
    assert_eq!(
        cache
            .load(&key, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap(),
        None
    );
    cache.store(&key, &first, &cancelled).unwrap();
    let second = archive(Uuid::from_u128(2), "other");
    let other = ArchiveCacheKey::new(Uuid::from_u128(2), "1.0.0", second.len() as u64).unwrap();
    cache.store(&other, &second, &cancelled).unwrap();
    assert!(
        cache
            .load(&key, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        cache
            .load(&other, &cancelled, SystemTime::UNIX_EPOCH)
            .unwrap(),
        Some(second)
    );
    cancelled.store(true, Ordering::Relaxed);
    assert_eq!(
        cache.store(&key, &first, &cancelled).unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
    assert!(
        std::fs::read_dir(root.path().join("native-v1"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".partial-"))
    );
}

#[cfg(unix)]
#[test]
fn cache_refuses_symlinked_namespace_and_object() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join("native-v1")).unwrap();
    assert!(ArchiveCache::open(root.path(), DEFAULT_CACHE_QUOTA).is_err());
    std::fs::remove_file(root.path().join("native-v1")).unwrap();
    let cache = ArchiveCache::open(root.path(), DEFAULT_CACHE_QUOTA).unwrap();
    let bytes = archive(Uuid::from_u128(1), "protected");
    let key = ArchiveCacheKey::new(Uuid::from_u128(1), "1.0.0", bytes.len() as u64).unwrap();
    let target = outside.path().join("source");
    std::fs::write(&target, &bytes).unwrap();
    let name = format!(
        "{}-{:x}{OBJECT_SUFFIX}",
        key.digest(),
        Sha256::digest(&bytes)
    );
    symlink(&target, root.path().join("native-v1").join(name)).unwrap();
    assert!(
        cache
            .load(&key, &AtomicBool::new(false), SystemTime::UNIX_EPOCH)
            .unwrap()
            .is_none()
    );
    assert_eq!(std::fs::read(target).unwrap(), bytes);
}

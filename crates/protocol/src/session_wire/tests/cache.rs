use std::path::Path;

use sha2::{Digest, Sha256};

use super::handoff_with_sizes;
use crate::session_wire::{CachedArchive, HandoffPackReceiver, SessionHandoff};

/// Builds one cache reference to bytes written under the supplied trusted root.
fn cached_handoff(root: &Path, bytes: &[u8]) -> SessionHandoff {
    let mut handoff = handoff_with_sizes(&[bytes.len() as u64]);
    let path = root.join("archive.mcpack");
    std::fs::write(&path, bytes).unwrap();
    handoff.packs[0].cache = Some(CachedArchive {
        path,
        sha256: Sha256::digest(bytes).into(),
    });
    handoff
}

#[test]
fn cached_and_streamed_archives_keep_stack_order() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let mut handoff = cached_handoff(&root, b"first");
    handoff.packs.extend(handoff_with_sizes(&[3, 4]).packs);
    let last = root.join("last.mcpack");
    std::fs::write(&last, b"last").unwrap();
    handoff.packs[2].cache = Some(CachedArchive {
        path: last,
        sha256: Sha256::digest(b"last").into(),
    });
    let mut receiver = HandoffPackReceiver::new(&handoff, Some(&root)).unwrap();
    assert!(receiver.accept(0, b"first").is_err());
    assert!(receiver.accept(2, b"last").is_err());
    receiver.accept(1, b"mid").unwrap();
    assert!(receiver.is_complete());
    assert_eq!(
        receiver.into_archives().unwrap(),
        [b"first".to_vec(), b"mid".to_vec(), b"last".to_vec()]
    );
}

#[test]
fn cached_archive_needs_no_pack_data() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let handoff = cached_handoff(&root, b"archive");
    let receiver = HandoffPackReceiver::new(&handoff, Some(&root)).unwrap();
    assert!(receiver.is_complete());
    assert_eq!(receiver.into_archives().unwrap(), [b"archive".to_vec()]);
    assert!(HandoffPackReceiver::new(&handoff, None).is_err());
}

#[test]
fn cached_archive_rejects_missing_changed_and_non_file_content() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let mut handoff = cached_handoff(&root, b"archive");
    let path = handoff.packs[0].cache.as_ref().unwrap().path.clone();
    handoff.packs[0].size += 1;
    assert!(HandoffPackReceiver::new(&handoff, Some(&root)).is_err());
    handoff.packs[0].size -= 1;
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(HandoffPackReceiver::new(&handoff, Some(&root)).is_err());
    std::fs::remove_file(&path).unwrap();
    assert!(HandoffPackReceiver::new(&handoff, Some(&root)).is_err());
    std::fs::create_dir(&path).unwrap();
    assert!(HandoffPackReceiver::new(&handoff, Some(&root)).is_err());
}

#[test]
fn cached_archive_rejects_relative_outside_and_parent_paths() {
    let parent = tempfile::tempdir().unwrap();
    let parent = parent.path().canonicalize().unwrap();
    let root = parent.join("cache");
    std::fs::create_dir(&root).unwrap();
    let mut handoff = cached_handoff(&root, b"archive");
    let outside = parent.join("outside.mcpack");
    std::fs::write(&outside, b"archive").unwrap();
    for path in [
        "archive.mcpack".into(),
        outside,
        root.join("../outside.mcpack"),
        parent.join("cache-other/archive.mcpack"),
    ] {
        handoff.packs[0].cache.as_mut().unwrap().path = path;
        assert!(HandoffPackReceiver::new(&handoff, Some(&root)).is_err());
    }
}

#[cfg(unix)]
#[test]
fn cached_archive_rejects_links_outside_the_cache() {
    let parent = tempfile::tempdir().unwrap();
    let parent = parent.path().canonicalize().unwrap();
    let root = parent.join("cache");
    std::fs::create_dir(&root).unwrap();
    let mut handoff = cached_handoff(&root, b"archive");
    std::fs::write(parent.join("outside.mcpack"), b"archive").unwrap();
    std::os::unix::fs::symlink(parent.join("outside.mcpack"), root.join("link.mcpack")).unwrap();
    std::os::unix::fs::symlink(&parent, root.join("linked-dir")).unwrap();
    for path in [
        root.join("link.mcpack"),
        root.join("linked-dir/outside.mcpack"),
    ] {
        handoff.packs[0].cache.as_mut().unwrap().path = path;
        assert!(HandoffPackReceiver::new(&handoff, Some(&root)).is_err());
    }
}

#[cfg(windows)]
#[test]
fn cached_archive_accepts_the_go_caches_folded_windows_path() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let mut handoff = cached_handoff(&root, b"archive");
    let reference = handoff.packs[0].cache.as_mut().unwrap();
    reference.path = reference.path.to_string_lossy().to_lowercase().into();
    assert!(
        HandoffPackReceiver::new(&handoff, Some(&root))
            .unwrap()
            .is_complete()
    );
}

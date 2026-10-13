use super::*;

#[test]
fn regular_sources_obey_the_byte_ceiling() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    std::fs::write(&path, b"abcd").unwrap();
    assert_eq!(read(&path, 4).unwrap(), b"abcd");
    assert!(
        read(&path, 3)
            .unwrap_err()
            .to_string()
            .contains("byte limit")
    );
    assert!(read(root.path(), 4).is_err());
}

#[test]
fn directory_reads_refuse_nonrelative_paths() {
    let root = tempfile::tempdir().unwrap();
    let directory = Directory::open(root.path()).unwrap();
    for path in [Path::new("../source"), root.path(), Path::new("")] {
        assert!(directory.read(path, 4).is_err());
    }
}

/// Creates file and directory links or reports the Windows fixture privilege requirement.
#[cfg(any(unix, windows))]
fn link(target: &Path, path: &Path, directory: bool) -> bool {
    #[cfg(unix)]
    let result = {
        let _ = directory;
        std::os::unix::fs::symlink(target, path)
    };
    #[cfg(windows)]
    let result = if directory {
        std::os::windows::fs::symlink_dir(target, path)
    } else {
        std::os::windows::fs::symlink_file(target, path)
    };
    match result {
        Ok(()) => true,
        #[cfg(windows)]
        Err(error) if error.raw_os_error() == Some(1314) => {
            eprintln!("Missing fixture: Windows symbolic-link privilege is unavailable");
            false
        }
        Err(error) => panic!("create link fixture: {error}"),
    }
}

#[test]
#[cfg(any(unix, windows))]
fn linked_files_and_package_parents_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("source"), b"abcd").unwrap();
    if !link(
        &outside.path().join("source"),
        &root.path().join("leaf"),
        false,
    ) {
        return;
    }
    assert!(read(&root.path().join("leaf"), 4).is_err());
    if !link(outside.path(), &root.path().join("parent"), true) {
        return;
    }
    let directory = Directory::open(root.path()).unwrap();
    assert!(directory.read(Path::new("parent/source"), 4).is_err());
    assert!(Directory::open(&root.path().join("parent")).is_err());
}

#[test]
#[cfg(unix)]
fn directory_snapshot_survives_a_path_replacement() {
    let root = tempfile::tempdir().unwrap();
    let package = root.path().join("package");
    std::fs::create_dir(&package).unwrap();
    std::fs::write(package.join("source"), b"old").unwrap();
    let directory = Directory::open(&package).unwrap();
    std::fs::rename(&package, root.path().join("previous")).unwrap();
    std::fs::create_dir(&package).unwrap();
    std::fs::write(package.join("source"), b"new").unwrap();
    assert_eq!(directory.read(Path::new("source"), 4).unwrap(), b"old");
}

#[test]
#[cfg(windows)]
fn leaf_reads_allow_atomic_replacement() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source");
    let replacement = root.path().join("replacement");
    std::fs::write(&path, b"old").unwrap();
    std::fs::write(&replacement, b"new").unwrap();
    let directory = Directory::open(root.path()).unwrap();
    let mut original = open_windows(&path, false).unwrap();

    std::fs::rename(&replacement, &path).unwrap();
    let mut bytes = Vec::new();
    original.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"old");
    assert_eq!(directory.read(Path::new("source"), 3).unwrap(), b"new");
}

#[test]
#[cfg(windows)]
fn directory_handles_prevent_path_replacement() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("parent");
    let replacement = root.path().join("renamed");
    std::fs::create_dir(&parent).unwrap();
    std::fs::write(parent.join("source"), b"old").unwrap();
    let directory = Directory::open(&parent).unwrap();

    assert!(std::fs::rename(&parent, &replacement).is_err());
    assert_eq!(directory.read(Path::new("source"), 3).unwrap(), b"old");
    drop(directory);
    std::fs::rename(&parent, &replacement).unwrap();
    assert_eq!(std::fs::read(replacement.join("source")).unwrap(), b"old");
}

#[test]
#[cfg(unix)]
fn special_sources_are_refused_without_a_writer() {
    let root = tempfile::tempdir().unwrap();
    let fifo = root.path().join("source");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.is_ok_and(|status| status.success()) {
        eprintln!("Missing fixture: mkfifo is unavailable");
        return;
    }
    assert!(
        read(&fifo, 4)
            .unwrap_err()
            .to_string()
            .contains("regular file")
    );
}

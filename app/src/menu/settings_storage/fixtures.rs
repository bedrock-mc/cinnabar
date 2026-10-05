use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

/// Gives each test a private, canonical data directory.
pub(super) fn fixture() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    fixture_with_nonce(nonce)
}

fn fixture_with_nonce(nonce: u128) -> PathBuf {
    for attempt in 0..1024 {
        let path = std::env::temp_dir().join(format!(
            "cinnabar-storage-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return fs::canonicalize(path).unwrap(),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!(
                "cannot create fixture directory {}: {error}",
                path.display()
            ),
        }
    }
    panic!("cannot claim a unique storage fixture directory");
}

#[test]
fn another_fixture_cannot_remove_cached_packs_when_clock_samples_repeat() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let first = fixture_with_nonce(nonce);
    fs::write(first.join("cached-pack"), b"pack").unwrap();
    let second = fixture_with_nonce(nonce);
    fs::remove_dir_all(second).unwrap();

    let cached_pack = fs::read(first.join("cached-pack")).ok();
    let _ = fs::remove_dir_all(first);
    assert_eq!(cached_pack.as_deref(), Some(b"pack".as_slice()));
}

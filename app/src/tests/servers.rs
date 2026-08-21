use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::menu::SavedServer;
use crate::menu::servers::{load_servers, save_servers};

fn unique_directory(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "cinnabar-saved-servers-{label}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn sample_server(name: &str, address: &str) -> SavedServer {
    SavedServer {
        name: name.to_owned(),
        address: address.to_owned(),
        favorite: true,
        last_joined_unix: 1_700_000_000,
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

#[test]
fn saved_servers_survive_an_atomic_save_and_reload() {
    let directory = unique_directory("roundtrip");
    let path = directory.join("servers.json");
    let servers = vec![
        sample_server("Lifeboat", "play.lbsg.net:19132"),
        sample_server("Local", "127.0.0.1:19132"),
    ];

    save_servers(&path, &servers).expect("save servers");
    let loaded = load_servers(&path);

    assert!(loaded.recovery_message.is_none());
    assert_eq!(loaded.servers, servers);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn malformed_saved_servers_are_quarantined_and_reported() {
    let directory = unique_directory("malformed");
    let path = directory.join("servers.json");
    write_bytes(&path, b"{ this is not an array");

    let loaded = load_servers(&path);

    assert!(loaded.servers.is_empty());
    let message = loaded.recovery_message.expect("recovery message");
    assert!(message.contains(".invalid"), "message was {message}");
    assert!(!path.exists(), "original file should have moved aside");
    let quarantine_path = path.with_file_name("servers.json.invalid");
    assert_eq!(
        fs::read(&quarantine_path).unwrap(),
        b"{ this is not an array"
    );

    // The next save recovers the surface normally.
    save_servers(&path, &[sample_server("After", "a.example:19132")]).unwrap();
    assert_eq!(load_servers(&path).servers.len(), 1);
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn out_of_schema_entries_quarantine_the_whole_file() {
    let directory = unique_directory("schema");
    let path = directory.join("servers.json");
    let oversized = vec![sample_server(&"x".repeat(200), "a.example:19132")];
    write_bytes(&path, serde_json::to_vec(&oversized).unwrap().as_slice());

    let loaded = load_servers(&path);

    assert!(loaded.servers.is_empty());
    assert!(loaded.recovery_message.is_some());
    assert!(!path.exists());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn missing_file_loads_empty_without_quarantining_anything() {
    let directory = unique_directory("missing");
    let path = directory.join("nested").join("servers.json");

    let loaded = load_servers(&path);

    assert!(loaded.servers.is_empty());
    assert!(loaded.recovery_message.is_none());
    assert!(!path.with_file_name("servers.json.invalid").exists());
    fs::remove_dir_all(&directory).unwrap();
}

#[cfg(unix)]
#[test]
fn unreadable_read_is_reported_and_the_file_survives() {
    use std::os::unix::fs::PermissionsExt;

    let directory = unique_directory("unreadable");
    let path = directory.join("servers.json");
    write_bytes(&path, b"[{\"name\":\"A\",\"address\":\"a.example:1\"}]");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

    let loaded = load_servers(&path);

    assert!(loaded.servers.is_empty());
    let message = loaded.recovery_message.expect("read-failure message");
    assert!(
        message.contains("could not be read"),
        "message was {message}"
    );
    assert!(path.exists(), "an unreadable file must not move aside");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_stale_temp_from_an_earlier_crash_never_blocks_future_saves() {
    let directory = unique_directory("stale-temp");
    let path = directory.join("servers.json");
    let stale_temp = path.with_file_name(format!("servers.json.tmp-{}", std::process::id()));
    write_bytes(&stale_temp, b"torn bytes from a crashed run");

    save_servers(&path, &[sample_server("Fresh", "f.example:19132")])
        .expect("a leftover same-pid temp must not wedge saving");

    assert_eq!(load_servers(&path).servers.len(), 1);
    assert!(!stale_temp.exists());
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_new_quarantine_replaces_an_earlier_one() {
    let directory = unique_directory("re-quarantine");
    let path = directory.join("servers.json");
    let quarantine_path = path.with_file_name("servers.json.invalid");
    write_bytes(&quarantine_path, b"earlier garbage");
    write_bytes(&path, b"newer garbage");

    let loaded = load_servers(&path);

    assert!(loaded.recovery_message.is_some());
    assert_eq!(fs::read(&quarantine_path).unwrap(), b"newer garbage");
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_failed_publish_cleans_up_and_keeps_the_previous_file_intact() {
    let directory = unique_directory("failed-publish");
    let path = directory.join("servers.json");
    let original = vec![sample_server("Original", "o.example:19132")];
    save_servers(&path, &original).unwrap();

    // A directory at the target path makes the final rename fail after the
    // temp sibling was written.
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();

    let error = save_servers(&path, &[sample_server("New", "n.example:19132")])
        .expect_err("rename over a directory must fail");

    assert!(error.to_string().contains("publish"));
    let leftovers: Vec<_> = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "temp leftovers: {leftovers:?}");
    fs::remove_dir_all(&directory).unwrap();
}

#[cfg(unix)]
#[test]
fn unix_saved_server_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let directory = unique_directory("unix-perms");
    let path = directory.join("servers.json");
    save_servers(&path, &[sample_server("P", "p.example:19132")]).unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    fs::remove_dir_all(&directory).unwrap();
}

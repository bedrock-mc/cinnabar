//! Deterministic temp-dir witnesses for per-session runtime directory
//! ownership and startup reclamation.

use std::{
    fs,
    path::{Path, PathBuf},
    process,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{
    MARKER_FILE_NAME, ReleaseOutcome, ScopedSessionDirectory, SessionDirectoryError,
    SessionDirectoryGuard, StaleReclaimPolicy, StaleReclaimReport, parse_catalog_file_name,
    parse_session_dir_name, reclaim_stale_entries,
};

/// A unique temporary root removed on drop.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rust-mcbe-session-cleanup-{label}-{}-{nonce}",
            process::id()
        ));
        fs::create_dir_all(&path).expect("create temp root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_marker(
    directory: &Path,
    token: &str,
    pid: u32,
    kind: &str,
    generation: u64,
    created_unix: u64,
) {
    let payload = format!(
        r#"{{"token":"{token}","pid":{pid},"kind":"{kind}","generation":{generation},"created_unix":{created_unix}}}"#
    );
    fs::write(directory.join(MARKER_FILE_NAME), payload).expect("write owner marker");
}

fn read_marker_token(directory: &Path) -> Option<String> {
    let bytes = fs::read(directory.join(MARKER_FILE_NAME)).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("token")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn set_file_mtime(path: &Path, unix_secs: u64) {
    use std::fs::FileTimes;
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open file for mtime");
    file.set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(unix_secs)))
        .expect("set mtime");
}

/// Fixed "now" for tests whose ages live inside owner markers; markers
/// carry their own timestamps, so no filesystem clock is involved.
const FAKE_NOW: u64 = 2_000_000_000;
const DAY_SECS: u64 = 24 * 60 * 60;

fn policy_at(now_unix: u64, limit: usize) -> StaleReclaimPolicy {
    StaleReclaimPolicy {
        now_unix,
        max_age_secs: DAY_SECS,
        limit,
        current_pid: process::id(),
    }
}

fn real_now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[test]
fn session_name_and_catalog_grammar_are_strict() {
    assert_eq!(
        parse_session_dir_name("connect-41-3").map(|id| (id.pid, id.generation)),
        Some((41, 3))
    );
    assert_eq!(
        parse_session_dir_name("direct-41").map(|id| (id.pid, id.generation)),
        Some((41, 0))
    );
    for invalid in [
        "connect-41",
        "direct",
        "direct-41-3",
        "connect-41-3-9",
        "connect--3",
        "connect-41-",
        "direct-+4",
        "direct-99999999999",
        "catalog-41.json",
        "servers.json",
        "",
    ] {
        assert!(parse_session_dir_name(invalid).is_none(), "{invalid}");
    }
    assert_eq!(parse_catalog_file_name("catalog-41.json"), Some(41));
    for invalid in [
        "catalog.json",
        "catalog-41.json.bak",
        "catalog-.json",
        "catalog-+4.json",
        "catalog-99999999999.json",
        "connect-41-1",
    ] {
        assert!(parse_catalog_file_name(invalid).is_none(), "{invalid}");
    }
}

#[test]
fn bound_guard_writes_identity_and_removes_on_drop() {
    let root = TempRoot::new("bind-drop");
    let directory = root.join("connect-1-1");

    let token = {
        let guard = SessionDirectoryGuard::bind(directory.clone()).expect("bind");
        assert!(directory.is_dir(), "bound directory must exist");
        let on_disk = read_marker_token(&directory).expect("owner marker must exist");
        guard_drop_capture(guard, on_disk)
    };
    assert!(
        !directory.exists(),
        "dropping the only binding removes the session directory"
    );
    assert!(!root.path().exists() || read_marker_token(&directory).is_none());
    // The recorded token came from inside the guard.
    assert!(!token.is_empty());
}

/// Returns the on-disk token so the test can prove it was non-empty even
/// though the guard consumed it on drop.
fn guard_drop_capture(guard: SessionDirectoryGuard, on_disk: String) -> String {
    drop(guard);
    on_disk
}

#[test]
fn explicit_release_is_idempotent_and_later_drops_do_nothing() {
    let root = TempRoot::new("release-idempotent");
    let directory = root.join("connect-7-2");
    let mut guard = SessionDirectoryGuard::bind(directory.clone()).expect("bind");

    assert_eq!(guard.release(), ReleaseOutcome::Removed);
    assert!(!directory.exists());
    assert_eq!(guard.release(), ReleaseOutcome::AlreadyReleased);
    drop(guard);
    assert!(!directory.exists(), "no resurrection after double shutdown");
}

#[test]
fn scoped_holder_removes_on_scope_exit_and_is_safe_when_empty() {
    let root = TempRoot::new("scoped");
    let directory = root.join("direct-71");
    {
        let _holder = ScopedSessionDirectory::bind(directory.clone()).expect("bind scoped");
        assert!(directory.is_dir());
    }
    assert!(!directory.exists(), "scope exit removes the directory");

    // An empty holder owns nothing and must stay a silent no-op.
    {
        let _holder = ScopedSessionDirectory::none();
    }
    assert!(read_marker_token(&directory).is_none());
}

#[test]
fn failed_session_start_still_removes_the_directory_before_returning() {
    // Mirrors the launcher's failed-start shape: bind, then an early
    // return drops the guard instead of leaking the directory.
    let root = TempRoot::new("failed-start");
    let directory = root.join("direct-3");
    let start_result = (|| -> Result<(), &'static str> {
        let mut guard = SessionDirectoryGuard::bind(directory.clone()).expect("bind");
        assert!(directory.is_dir());
        let failure: Result<(), &'static str> = Err("core spawn failed");
        if failure.is_err() {
            guard.release();
            return Err("core spawn failed");
        }
        Ok(())
    })();
    assert_eq!(start_result, Err("core spawn failed"));
    assert!(
        !directory.exists(),
        "failed start must not leak the directory"
    );
}

#[test]
fn foreign_identity_marker_refuses_bind_and_preserves_the_owner() {
    let root = TempRoot::new("foreign-bind");
    let directory = root.join("direct-9");
    fs::create_dir_all(&directory).expect("seed foreign directory");
    write_marker(&directory, "a".repeat(32).trim(), 4_242, "direct", 0, 100);
    let before = fs::read_to_string(directory.join(MARKER_FILE_NAME)).expect("marker bytes");

    let error =
        SessionDirectoryGuard::bind(directory.clone()).expect_err("foreign identity must refuse");
    assert!(matches!(
        error,
        SessionDirectoryError::ForeignIdentity { pid: 4_242, .. }
    ));
    let after = fs::read_to_string(directory.join(MARKER_FILE_NAME)).expect("marker survives");
    assert_eq!(
        before, after,
        "refused bind must not touch the owner's data"
    );
    assert!(read_marker_token(&directory).is_some());
}

#[test]
fn tampered_marker_on_disk_refuses_removal() {
    let root = TempRoot::new("tampered-release");
    let directory = root.join("connect-5-5");
    let mut guard = SessionDirectoryGuard::bind(directory.clone()).expect("bind");
    // Someone else rewrites or replaces the identity marker after bind:
    // this binding may no longer prove ownership.
    write_marker(
        &directory,
        "f".repeat(32).trim(),
        process::id(),
        "connect",
        5,
        200,
    );

    assert_eq!(guard.release(), ReleaseOutcome::IdentityRefused);
    assert!(directory.exists(), "identity mismatch refuses deletion");
    assert!(
        read_marker_token(&directory).is_some(),
        "the surviving marker is left exactly as found"
    );

    // A missing marker is equally unprovable.
    fs::remove_file(directory.join(MARKER_FILE_NAME)).expect("strip marker");
    assert_eq!(guard.release(), ReleaseOutcome::AlreadyReleased);
    assert!(directory.exists());
    drop(guard);
    assert!(directory.exists(), "drop after refusal still never deletes");
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn dead_same_pid_predecessor_is_taken_over_cleanly() {
    let root = TempRoot::new("same-pid-takeover");
    let directory = root.join("connect-11-1");
    fs::create_dir_all(&directory).expect("seed dead predecessor");
    write_marker(
        &directory,
        "b".repeat(32).trim(),
        process::id(),
        "connect",
        1,
        300,
    );
    fs::write(directory.join("bridge.endpoint"), "stale publication").expect("stale endpoint");

    let mut guard =
        SessionDirectoryGuard::bind(directory.clone()).expect("dead predecessor takeover");
    let token = read_marker_token(&directory).expect("fresh marker");
    assert_ne!(
        token,
        "b".repeat(32),
        "takeover must install a new binding token"
    );
    assert_eq!(guard.release(), ReleaseOutcome::Removed);
    assert!(!directory.exists());
}

#[test]
fn malformed_marker_refuses_bind_instead_of_guessing() {
    let root = TempRoot::new("malformed-bind");
    let directory = root.join("direct-13");
    fs::create_dir_all(&directory).expect("seed malformed directory");
    fs::write(directory.join(MARKER_FILE_NAME), b"{not json").expect("corrupt marker");

    let error =
        SessionDirectoryGuard::bind(directory.clone()).expect_err("unreadable marker must refuse");
    assert!(matches!(
        error,
        SessionDirectoryError::MalformedIdentity { .. }
    ));
    assert!(
        fs::read_to_string(directory.join(MARKER_FILE_NAME))
            .expect("malformed marker untouched")
            .starts_with('{')
    );
}

#[test]
fn concurrent_looking_bindings_do_not_cross_delete() {
    let root = TempRoot::new("concurrent");
    let first_directory = root.join("connect-21-1");
    let second_directory = root.join("connect-21-2");
    let mut first = SessionDirectoryGuard::bind(first_directory.clone()).expect("bind first");
    let mut second = SessionDirectoryGuard::bind(second_directory.clone()).expect("bind second");

    assert_ne!(
        read_marker_token(&first_directory),
        read_marker_token(&second_directory),
        "each binding owns its own random identity"
    );
    assert_eq!(first.release(), ReleaseOutcome::Removed);
    assert!(!first_directory.exists());
    assert!(second_directory.exists(), "sibling session stays untouched");
    assert_eq!(second.release(), ReleaseOutcome::Removed);

    // Reclamation with everything fresh protects both names anyway.
    fs::create_dir_all(&first_directory).expect("recreate");
    write_marker(
        &first_directory,
        "c".repeat(32).trim(),
        process::id(),
        "connect",
        1,
        FAKE_NOW - 100_000,
    );
    let report = reclaim_stale_entries(root.path(), &policy_at(FAKE_NOW, 8));
    assert_eq!(
        report,
        StaleReclaimReport {
            skipped_protected_identity: 1,
            ..StaleReclaimReport::default()
        },
        "current-process markers are protected from reclamation"
    );
    assert!(first_directory.exists());
}

#[test]
fn reclaim_respects_age_and_count_bounds_and_skips_unprovable_entries() {
    let root = TempRoot::new("reclaim-bounds");
    let old_created = FAKE_NOW - DAY_SECS - 3_600;
    // Three clearly stale directories; the oldest two must win the budget.
    for (name, created, token_seed) in [
        ("connect-31-1", old_created - 20, "d"),
        ("connect-31-2", old_created - 10, "e"),
        ("direct-32", old_created, "f"),
    ] {
        let directory = root.join(name);
        fs::create_dir_all(&directory).expect("seed stale dir");
        write_marker(
            &directory,
            token_seed.repeat(32).trim(),
            777,
            "connect",
            1,
            created,
        );
    }
    // Fresh foreign session: right family, proven identity, too young.
    let fresh = root.join("connect-41-1");
    fs::create_dir_all(&fresh).expect("seed fresh dir");
    write_marker(
        &fresh,
        "0".repeat(32).trim(),
        41,
        "connect",
        1,
        FAKE_NOW - 60,
    );
    // Unmarked legacy leftover: identity unprovable, never touched.
    let unmarked = root.join("direct-42");
    fs::create_dir_all(&unmarked).expect("seed legacy dir");
    // Corrupt marker: identity unprovable, never touched.
    let corrupt = root.join("connect-43-1");
    fs::create_dir_all(&corrupt).expect("seed corrupt dir");
    fs::write(corrupt.join(MARKER_FILE_NAME), b"}not-json{").expect("corrupt marker");
    // Unrelated user data shares the root.
    fs::write(root.join("notes.txt"), b"user data").expect("seed unrelated file");
    fs::create_dir_all(root.join("saved-worlds")).expect("seed unrelated dir");

    let report = reclaim_stale_entries(root.path(), &policy_at(FAKE_NOW, 2));

    assert_eq!(report.reclaimed_directories, 2, "budget caps removals");
    assert_eq!(report.failed, 0);
    assert_eq!(report.skipped_fresh, 1);
    assert_eq!(report.skipped_protected_identity, 0);
    assert_eq!(report.skipped_unmarked, 1);
    assert_eq!(report.skipped_unreadable_identity, 1);
    assert_eq!(report.skipped_unrelated, 2, "one file plus one directory");
    assert!(!root.join("connect-31-1").exists(), "oldest goes first");
    assert!(!root.join("connect-31-2").exists());
    assert!(root.join("direct-32").exists(), "third stale entry waits");
    assert!(fresh.exists());
    assert!(unmarked.exists());
    assert!(corrupt.exists());
    assert!(root.join("notes.txt").exists());
    assert!(root.join("saved-worlds").exists());

    // Next startup finishes the remainder under a fresh budget.
    let followup = reclaim_stale_entries(root.path(), &policy_at(FAKE_NOW, 64));
    assert_eq!(followup.reclaimed_directories, 1);
    assert!(!root.join("direct-32").exists());
}

#[test]
fn reclaim_removes_only_old_leaked_catalog_files_by_mtime() {
    // Catalog artifacts carry no marker, so their age comes from the real
    // filesystem clock; the policy now must be the same clock.
    let now = real_now_unix();
    let root = TempRoot::new("reclaim-catalog");
    let stale_path = root.join("catalog-51.json");
    fs::write(&stale_path, b"{}").expect("seed stale catalog");
    set_file_mtime(&stale_path, now - DAY_SECS - 3_600);
    let fresh_path = root.join("catalog-52.json");
    fs::write(&fresh_path, b"{}").expect("seed fresh catalog");

    let report = reclaim_stale_entries(root.path(), &policy_at(now, 8));

    assert_eq!(report.reclaimed_catalog_files, 1);
    assert!(!stale_path.exists());
    assert!(fresh_path.exists(), "young catalog files stay");
    assert_eq!(report.skipped_fresh, 1);
}

#[test]
fn reclaim_handles_missing_root_and_non_directories() {
    let missing = std::env::temp_dir().join(format!(
        "rust-mcbe-session-cleanup-absent-{}",
        process::id()
    ));
    let _ = fs::remove_dir_all(&missing);
    assert_eq!(
        reclaim_stale_entries(&missing, &policy_at(real_now_unix(), 8)),
        StaleReclaimReport::default()
    );
    assert!(!missing.exists(), "absent roots are never created");
}

#[test]
fn windows_path_shapes_round_trip_through_the_guard() {
    // Windows accepts forward slashes; guards must not canonicalize the
    // stored path into `\\?\` form and must clean up through the exact
    // bound spelling, including spaces in the root.
    let root = TempRoot::new("windows shapes ok");
    let directory = PathBuf::from(format!("{}/connect-61-1", root.path().display()));
    let mut guard =
        SessionDirectoryGuard::bind(directory.clone()).expect("bind forward-slash path");
    assert!(directory.is_dir());

    let reclaimed = reclaim_stale_entries(root.path(), &policy_at(real_now_unix(), 8));
    assert_eq!(reclaimed.reclaimed_directories, 0, "fresh binding stays");

    assert_eq!(guard.release(), ReleaseOutcome::Removed);
    assert!(!Path::new(&directory).exists());
}

fn development_layout_in(root: &Path) -> crate::install_layout::InstallLayout {
    crate::install_layout::InstallLayout::resolve(
        crate::install_layout::Platform::Linux,
        &crate::install_layout::InstallEnvironment {
            executable: root.join("target/debug/bedrock-client"),
            home: Some(root.join("home")),
            local_app_data: None,
            xdg_config_home: None,
            xdg_data_home: None,
            xdg_runtime_dir: None,
        },
    )
    .expect("temp development layout")
}

#[test]
fn menu_runtime_binding_replaces_releases_and_drops_cleanly() {
    let root = TempRoot::new("menu-wiring");
    let layout = development_layout_in(root.path());
    let mut menu =
        crate::menu::MenuRuntime::new_with_layout(true, 2, "Player".to_owned(), layout.clone());

    let first = SessionDirectoryGuard::bind(layout.connect_socket_dir(process::id(), 1))
        .expect("bind first");
    let first_directory = first.directory.clone();
    menu.bind_session_directory(first);
    assert!(first_directory.is_dir());

    // Binding a replacement releases the superseded session directory.
    let second = SessionDirectoryGuard::bind(layout.connect_socket_dir(process::id(), 2))
        .expect("bind second");
    let second_directory = second.directory.clone();
    menu.bind_session_directory(second);
    assert!(
        !first_directory.exists(),
        "replaced binding removes the old session directory"
    );
    assert!(second_directory.exists());

    menu.release_session_directory();
    assert!(!second_directory.exists());

    let third = SessionDirectoryGuard::bind(layout.connect_socket_dir(process::id(), 3))
        .expect("bind third");
    let third_directory = third.directory.clone();
    menu.bind_session_directory(third);
    drop(menu);
    assert!(
        !third_directory.exists(),
        "dropping the menu runtime removes its session directory"
    );
    let _ = fs::remove_dir_all(root.path().join(".local"));
}

#[test]
fn reclamation_never_follows_a_seeded_link_shaped_session_name() {
    // A grammar-named entry that is really a symlink or Windows junction is
    // never classified as a reclaimable session directory, even when its
    // target carries a provably stale foreign marker that would otherwise
    // qualify for removal.
    let root = TempRoot::new("link-shaped-session");
    // The link target deliberately carries no session-like name so the only
    // counted entry in this root is the seeded link itself.
    let target = root.join("junction-target");
    fs::create_dir_all(&target).expect("seed link target");
    write_marker(
        &target,
        "9".repeat(32).trim(),
        999_999,
        "connect",
        1,
        FAKE_NOW - 2 * DAY_SECS,
    );
    let link = root.join("connect-91-1");

    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .status()
            .expect("spawn mklink for the junction witness");
        assert!(status.success(), "junction creation must succeed");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).expect("seed session-name symlink");

    let report = reclaim_stale_entries(root.path(), &policy_at(real_now_unix(), 8));

    assert_eq!(
        report.reclaimed_directories, 0,
        "reclamation must never delete through a link"
    );
    // Two unrelated entries: the plain target directory (no session-like
    // name) plus the seeded link itself, whose reparse-point file type
    // fails the real-directory gate before its marker is ever read.
    assert_eq!(report.skipped_unrelated, 2);
    assert!(
        fs::symlink_metadata(&link).is_ok(),
        "the seeded link itself stays untouched"
    );
    assert!(target.is_dir(), "the linked target stays untouched");
}

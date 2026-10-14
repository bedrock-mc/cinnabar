use std::{fs, path::Path};

use super::*;
use crate::renderer_startup::{Stage, StartupGuard};
use launcher::graphics_backend::Backend;

const NOW_MS: u64 = 1_760_443_200_000;
const CLIENT_LOG: &str = "CLIENT_SESSION_START session=old\nprevious launch\nCLIENT_SESSION_START session=0123456789abcdef\nclient line\n";
const CORE_LOG: &str = "CORE_SESSION_START timestamp_ms=1\nstale core\nCORE_SESSION_START timestamp_ms=1760443100000\ncore line\n";

fn identity() -> Identity {
    Identity {
        session_id: "0123456789abcdef".into(),
        started_at_ms: NOW_MS - 120_000,
        release: "1.2.3".into(),
        commit: "abc123".into(),
        build_profile: "debug".into(),
    }
}

fn layout(root: &Path) -> InstallLayout {
    let mut layout = launcher::test_support::checkout();
    layout.user_data_root = root.to_owned();
    layout
}

fn write_logs(root: &Path) {
    let logs = root.join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("client.log"), CLIENT_LOG).unwrap();
    fs::write(logs.join("core.log"), CORE_LOG).unwrap();
}

fn only_folder(root: &Path) -> PathBuf {
    let folders = folder::list(&root.join("crashes"));
    assert_eq!(folders.len(), 1, "{folders:?}");
    folders.into_iter().next().unwrap()
}

fn gpu_attempt(name: &str, stage: Stage) -> Attempt {
    Attempt {
        backend: Backend::Dx12,
        adapter: Some(Adapter {
            name: name.into(),
            vendor: 1,
            device: 2,
            driver: "driver".into(),
            driver_info: "version".into(),
            driver_version: String::new(),
            integrated: false,
        }),
        stage,
        explicit: false,
    }
}

#[test]
fn panic_report_copies_the_client_session_not_core_log() {
    let dir = tempfile::tempdir().unwrap();
    write_logs(dir.path());
    let context = context(&layout(dir.path()), identity());
    let thread = Thread {
        name: Some("worker".into()),
        id: "ThreadId(9)".into(),
    };
    let folder = write_panic(
        &context,
        "boom".into(),
        thread.clone(),
        "0: frame".into(),
        NOW_MS,
    )
    .unwrap();
    assert_eq!(folder, only_folder(dir.path()));
    let client = fs::read_to_string(folder.join(folder::CLIENT_LOG_FILE)).unwrap();
    assert_eq!(
        client,
        "CLIENT_SESSION_START session=0123456789abcdef\nclient line\n"
    );
    assert!(!client.contains("core line"));
    assert_eq!(
        fs::read_to_string(folder.join(folder::CORE_LOG_FILE)).unwrap(),
        "CORE_SESSION_START timestamp_ms=1760443100000\ncore line\n"
    );
    let report = folder::read(&folder).unwrap();
    assert_eq!(report.kind, Kind::Panic);
    assert_eq!(report.message, "boom");
    assert_eq!(report.thread, thread);
    assert_eq!(report.backtrace, "0: frame");
    assert_eq!(report.session_id, "0123456789abcdef");
    assert_eq!(report.commit, "abc123");
    assert_eq!(report.uptime_ms, 120_000);
    assert_eq!(report.renderer, None);
}

#[test]
fn core_sections_from_before_this_session_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    write_logs(dir.path());
    fs::write(
        dir.path().join("logs/core.log"),
        "CORE_SESSION_START timestamp_ms=5\nprevious launch core\n",
    )
    .unwrap();
    let context = context(&layout(dir.path()), identity());
    let folder = write_panic(
        &context,
        "boom".into(),
        Thread::current(),
        String::new(),
        NOW_MS,
    )
    .unwrap();
    assert!(
        fs::read(folder.join(folder::CORE_LOG_FILE))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn panic_report_retains_the_pending_renderer_stage_and_gpu() {
    let dir = tempfile::tempdir().unwrap();
    let mut guard = StartupGuard::open(dir.path()).unwrap();
    let attempt = gpu_attempt("test GPU", Stage::FirstPresentation);
    guard.begin(attempt.clone()).unwrap();
    let context = context(&layout(dir.path()), identity());
    let folder = write_panic(
        &context,
        "boom".into(),
        Thread::current(),
        String::new(),
        NOW_MS,
    )
    .unwrap();
    let report = folder::read(&folder).unwrap();
    assert_eq!(
        report.renderer,
        Some(serde_json::to_value(attempt).unwrap())
    );
    assert_eq!(report.gpu.as_deref(), Some("test GPU"));
    assert_eq!(report.driver.as_deref(), Some("driver version"));
}

#[test]
fn renderer_context_fixture() {
    let Some(root) = std::env::var_os("CINNABAR_RENDERER_CONTEXT_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    assert!(renderer_context(&root).is_none());
    let mut guard = StartupGuard::open(&root).unwrap();
    guard
        .begin(Attempt {
            backend: Backend::Gl,
            adapter: None,
            stage: Stage::FirstPresentation,
            explicit: false,
        })
        .unwrap();
    guard.presented().unwrap();
}

#[test]
fn completed_renderer_context_survives_another_clients_saved_choice() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(dir.path()).unwrap();
    let mut attempt = gpu_attempt("first process GPU", Stage::FirstPresentation);
    first.begin(attempt.clone()).unwrap();
    first.presented().unwrap();
    attempt.stage = Stage::Ready;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lifecycle::crash::tests::renderer_context_fixture",
        ])
        .env("CINNABAR_RENDERER_CONTEXT_FIXTURE", dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        StartupGuard::saved(dir.path())
            .unwrap()
            .working
            .unwrap()
            .backend,
        Backend::Gl
    );
    assert_eq!(renderer_context(dir.path()), Some(attempt.clone()));
    let context = context(&layout(dir.path()), identity());
    let folder = write_panic(
        &context,
        "boom".into(),
        Thread::current(),
        String::new(),
        NOW_MS,
    )
    .unwrap();
    assert_eq!(
        folder::read(&folder).unwrap().renderer,
        Some(serde_json::to_value(attempt.clone()).unwrap())
    );
    let mut retry = StartupGuard::open(dir.path()).unwrap();
    attempt.stage = Stage::CreatingDevice;
    retry.begin(attempt).unwrap();
    retry.failed().unwrap();
    assert!(renderer_context(dir.path()).is_none());
}

/// Appends the hook's panic text to the fixture's client log, as the app's file log would.
fn log_fixture_panic(panic: &str) {
    use std::io::Write;
    let Some(root) = std::env::var_os("CINNABAR_PANIC_HOOK_FIXTURE") else {
        return;
    };
    if let Ok(mut log) = fs::OpenOptions::new()
        .append(true)
        .open(Path::new(&root).join("logs/client.log"))
    {
        let _ = log.write_all(panic.as_bytes());
    }
}

/// Installs the real hook in a disposable process and panics on a named thread.
#[test]
fn panic_hook_fixture() {
    let Some(root) = std::env::var_os("CINNABAR_PANIC_HOOK_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    let mut identity = identity();
    identity.started_at_ms = 0;
    install_panic_hook(&layout(&root), identity, log_fixture_panic, || {}, || {});
    let worker = std::thread::Builder::new()
        .name("crash-fixture".into())
        .spawn(|| panic!("fixture panic"))
        .unwrap();
    assert!(worker.join().is_err());
}

#[test]
fn installed_hook_writes_one_folder_per_panic_with_the_client_log() {
    let dir = tempfile::tempdir().unwrap();
    write_logs(dir.path());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lifecycle::crash::tests::panic_hook_fixture",
            "--nocapture",
        ])
        .env("CINNABAR_PANIC_HOOK_FIXTURE", dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let folder = only_folder(dir.path());
    let report = folder::read(&folder).unwrap();
    assert_eq!(report.kind, Kind::Panic);
    assert!(
        report.message.contains("fixture panic"),
        "{}",
        report.message
    );
    assert_eq!(report.thread.name.as_deref(), Some("crash-fixture"));
    assert!(!report.thread.id.is_empty());
    assert!(!report.backtrace.is_empty());
    assert_eq!(report.session_id, "0123456789abcdef");
    assert!(report.uptime_ms > 0);
    let client = fs::read_to_string(folder.join(folder::CLIENT_LOG_FILE)).unwrap();
    let logged = client
        .strip_prefix("CLIENT_SESSION_START session=0123456789abcdef\nclient line\n")
        .unwrap_or_else(|| panic!("client section of another session: {client}"));
    // The log line carries the report's own backtrace: one capture per panic.
    assert!(logged.contains("fixture panic"), "{logged}");
    assert!(logged.contains(&report.backtrace), "{logged}");
    assert!(
        fs::read_to_string(folder.join(folder::CORE_LOG_FILE))
            .unwrap()
            .contains("core line")
    );
}

#[test]
fn startup_prune_removes_legacy_reports_and_keeps_folders() {
    let dir = tempfile::tempdir().unwrap();
    let crash_dir = dir.path().join("crashes");
    let context = context(&layout(dir.path()), identity());
    let folder = write_panic(
        &context,
        "boom".into(),
        Thread::current(),
        String::new(),
        NOW_MS,
    )
    .unwrap();
    fs::write(crash_dir.join("crash-1-00000000000000000000.json"), "{}").unwrap();
    fs::write(crash_dir.join("crash-2-native.json"), "{}").unwrap();
    prune_reports(&layout(dir.path()));
    let remaining: Vec<PathBuf> = fs::read_dir(&crash_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(remaining, [folder]);
}

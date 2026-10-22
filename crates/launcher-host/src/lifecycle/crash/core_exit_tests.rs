use std::{
    fs,
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crash_report::report::Identity;
use tracing::level_filters::LevelFilter;

use super::*;
use crate::lifecycle::core_output::{CoreOutput, CoreRole, Reporter, STAGING_DIR, fixture};

const NOW_MS: u64 = 1_760_443_200_000;
const STARTED_MS: u64 = NOW_MS - 60_000;

fn identity() -> Identity {
    Identity {
        session_id: "0123456789abcdef".into(),
        started_at_ms: STARTED_MS,
        release: "1.2.3".into(),
        commit: "abc123".into(),
        build_profile: "debug".into(),
    }
}

/// A crash context in `root` whose `core.log` starts this session, and core output appending
/// to it that writes each report there and counts them.
fn harness(root: &Path) -> (CoreOutput, Arc<AtomicU64>) {
    let mut layout = launcher::test_support::checkout();
    layout.user_data_root = root.to_owned();
    layout.user_config_root = root.join("config");
    let context = super::super::context(&layout, identity());
    fs::create_dir_all(layout.log_dir()).unwrap();
    fs::write(
        layout.log_dir().join("client.log"),
        "CLIENT_SESSION_START session=0123456789abcdef\nclient line\n",
    )
    .unwrap();
    let mut core_log = fs::File::create(layout.log_dir().join("core.log")).unwrap();
    writeln!(
        core_log,
        "{} timestamp_ms={STARTED_MS}",
        crash_report::section::CORE_SESSION_START
    )
    .unwrap();
    let reports = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&reports);
    let report: Reporter = Arc::new(move |crash| {
        counted.fetch_add(1, Ordering::SeqCst);
        Some(write(&context, crash, NOW_MS).unwrap())
    });
    let output = CoreOutput::new(
        || LevelFilter::INFO,
        Some(Box::new(core_log)),
        Some(layout.crash_dir().join(STAGING_DIR)),
        Some(report),
    )
    .unwrap();
    (output, reports)
}

/// Waits until the watcher deleted the staged crash output, its last step for an exit.
fn wait_for_exit_handled(root: &Path, pid: u32) {
    let core_log = root.join("logs").join("core.log");
    let exited = format!("pid={pid} msg=\"core exited\"");
    let staging = root.join("crashes").join(STAGING_DIR);
    test_time::eventually("the core's exit to be handled", || {
        fs::read_to_string(&core_log).is_ok_and(|log| log.contains(&exited))
            && fs::read_dir(&staging).is_ok_and(|mut entries| entries.next().is_none())
    });
}

#[test]
fn a_panicking_core_writes_one_core_folder_with_its_go_traceback() {
    let Some(core) = fixture::crash_core() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (output, reports) = harness(dir.path());
    let child = output
        .spawn(&mut fixture::command(core, "panic"), CoreRole::Game)
        .unwrap();
    let pid = child.id();
    assert_eq!(child.wait().and_then(|status| status.code()), Some(2));
    wait_for_exit_handled(dir.path(), pid);
    assert_eq!(reports.load(Ordering::SeqCst), 1);
    let folders = folder::list(&dir.path().join("crashes"));
    assert_eq!(folders.len(), 1, "{folders:?}");
    let report = folder::read(&folders[0]).unwrap();
    assert_eq!(report.kind, Kind::Core);
    assert!(
        report.message.starts_with("game core crashed (")
            && report.message.ends_with("): panic: fixture core panic"),
        "{}",
        report.message
    );
    assert_eq!(report.thread.id, pid.to_string());
    let traceback = fs::read_to_string(folders[0].join(folder::CORE_CRASH_FILE)).unwrap();
    assert!(
        traceback.starts_with("panic: fixture core panic"),
        "{traceback}"
    );
    assert!(
        traceback.contains("goroutine 1 ["),
        "every goroutine:\n{traceback}"
    );
    let core_section = fs::read_to_string(folders[0].join(folder::CORE_LOG_FILE)).unwrap();
    assert!(core_section.starts_with(crash_report::section::CORE_SESSION_START));
    assert!(core_section.contains("msg=\"fixture started\" process=fixture"));
    assert!(core_section.contains(&format!("process=game pid={pid} panic: fixture core panic")));
    let client_section = fs::read_to_string(folders[0].join(folder::CLIENT_LOG_FILE)).unwrap();
    assert!(client_section.contains("client line"));
}

#[test]
fn clean_and_failed_core_exits_write_no_folder() {
    let Some(core) = fixture::crash_core() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (output, reports) = harness(dir.path());
    for (mode, code) in [("clean", 0), ("fail", 1)] {
        let child = output
            .spawn(&mut fixture::command(core, mode), CoreRole::Account)
            .unwrap();
        let pid = child.id();
        assert_eq!(child.wait().and_then(|status| status.code()), Some(code));
        wait_for_exit_handled(dir.path(), pid);
    }
    assert_eq!(reports.load(Ordering::SeqCst), 0);
    assert!(folder::list(&dir.path().join("crashes")).is_empty());
}

#[test]
fn a_core_this_client_kills_writes_no_folder() {
    let Some(core) = fixture::crash_core() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (output, reports) = harness(dir.path());
    let mut command = fixture::command(core, "hold");
    command.stdin(std::process::Stdio::piped());
    let child = output.spawn(&mut command, CoreRole::Game).unwrap();
    let pid = child.id();
    child.kill();
    child.wait();
    wait_for_exit_handled(dir.path(), pid);
    assert_eq!(reports.load(Ordering::SeqCst), 0);
}

#[test]
fn automatic_upload_consent_follows_completed_core_attachments() {
    use launcher::menu::settings_options::{SETTINGS_FILE, SettingId, SettingsOptions};
    let dir = tempfile::tempdir().unwrap();
    let mut layout = launcher::test_support::checkout();
    layout.user_data_root = dir.path().join("data");
    layout.user_config_root = dir.path().join("config");
    let mut settings = SettingsOptions::default();
    settings.set(SettingId::SendCrashReports.index(), 1);
    settings
        .save(&layout.user_config_root.join(SETTINGS_FILE))
        .unwrap();
    let context = super::super::context(&layout, identity());
    // Obtain a portable status; the synthetic crash supplies its own traceback.
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    let folder = write(
        &context,
        &CoreCrash {
            role: CoreRole::Game,
            pid: 123,
            status,
            crash_output: b"panic: fixture\ntraceback".to_vec(),
        },
        NOW_MS,
    )
    .unwrap();
    assert!(
        fs::read_to_string(folder.join(folder::CORE_CRASH_FILE))
            .unwrap()
            .contains("traceback")
    );
    assert!(matches!(
        crash_report::upload::state::read(&folder),
        Some(crash_report::upload::state::Upload::Pending {
            consent: crash_report::upload::state::Consent::Auto,
            ..
        })
    ));
}

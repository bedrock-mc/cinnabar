use std::{fs, path::Path};

use super::*;
use crate::renderer_startup::{Stage, StartupGuard};
use fallback::write_panic;
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
    assert!(state::renderer(&root).is_none());
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
    assert_eq!(state::renderer(dir.path()), Some(attempt.clone()));
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
    assert!(state::renderer(dir.path()).is_none());
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

const NO_HOOKS: LogHooks = LogHooks {
    log_panic: |_| {},
    flush: || {},
    drain_on_signal: || {},
};

/// Installs the real hook without a monitor in a disposable process and panics on a named
/// thread.
#[test]
fn panic_hook_fixture() {
    let Some(root) = std::env::var_os("CINNABAR_PANIC_HOOK_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    let mut identity = identity();
    identity.started_at_ms = 0;
    let hooks = LogHooks {
        log_panic: log_fixture_panic,
        ..NO_HOOKS
    };
    install(&layout(&root), identity, hooks, None);
    let worker = std::thread::Builder::new()
        .name("crash-fixture".into())
        .spawn(|| panic!("fixture panic"))
        .unwrap();
    assert!(worker.join().is_err());
}

#[test]
fn without_a_monitor_the_hook_writes_the_panic_folder_with_the_client_log() {
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

/// Models a previous panic hook whose stderr text is captured by the installed log writer.
#[test]
fn previous_hook_fixture() {
    let Some(root) = std::env::var_os("CINNABAR_PREVIOUS_HOOK_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    std::panic::set_hook(Box::new(|_| {
        log_fixture_panic("previous hook panic text\n")
    }));
    install(&layout(&root), identity(), NO_HOOKS, None);
    assert!(
        std::thread::spawn(|| panic!("fixture panic"))
            .join()
            .is_err()
    );
}

#[test]
fn panic_snapshot_includes_the_previous_hooks_output() {
    let dir = tempfile::tempdir().unwrap();
    write_logs(dir.path());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lifecycle::crash::tests::previous_hook_fixture",
            "--nocapture",
        ])
        .env("CINNABAR_PANIC_HOOK_FIXTURE", dir.path())
        .env("CINNABAR_PREVIOUS_HOOK_FIXTURE", dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let client = fs::read_to_string(only_folder(dir.path()).join(folder::CLIENT_LOG_FILE)).unwrap();
    assert!(
        client.contains("previous hook panic text"),
        "panic output was copied too early: {client}"
    );
}

/// Startup prune drops legacy files and staging directories a dead client left, but keeps
/// folders and this live client's staging directory.
#[test]
fn startup_prune_removes_legacy_reports_and_dead_staging_and_keeps_folders() {
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
    drop(crash_report::staging::Staging::create(&crash_dir, "dead").unwrap());
    prune_reports(&layout(dir.path()));
    let mut remaining: Vec<PathBuf> = fs::read_dir(&crash_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    remaining.sort();
    let live = crash_report::staging::path(&crash_dir, &identity().session_id);
    assert_eq!(remaining, [live, folder]);
}

/// Linux and Android only: macOS crash capture uses Mach exception ports, whose forwarding of
/// a raised `SIGABRT` differs.
#[cfg(any(target_os = "linux", target_os = "android"))]
mod signals {
    use std::sync::atomic::AtomicPtr;

    use super::*;

    const CORE_LOG: &str = "CORE_SESSION_START timestamp_ms=1\nearlier core run\nCORE_SESSION_START timestamp_ms=2\ncore line\n";
    static FAULT_ADDRESS: AtomicPtr<libc::c_void> = AtomicPtr::new(std::ptr::null_mut());

    /// Checks that chaining retains the kernel's fault address, cause and machine context.
    unsafe extern "C" fn previous_siginfo_handler(
        signal: i32,
        info: *mut libc::siginfo_t,
        context: *mut libc::c_void,
    ) {
        let original = signal == libc::SIGSEGV
            && !info.is_null()
            && !context.is_null()
            && unsafe {
                (*info).si_code > 0 && (*info).si_addr() == FAULT_ADDRESS.load(Ordering::Relaxed)
            };
        unsafe {
            libc::_exit(if original { 77 } else { 78 });
        }
    }

    /// Exits through the handler installed before capture, proving it remains in control.
    unsafe extern "C" fn previous_handler(_signal: i32) {
        unsafe {
            libc::_exit(77);
        }
    }

    #[test]
    fn signal_fixture() {
        let Some(root) = std::env::var_os("CINNABAR_SIGNAL_FIXTURE") else {
            return;
        };
        let root = PathBuf::from(root);
        let hardware = std::env::var_os("CINNABAR_SIGNAL_HARDWARE").is_some();
        let software_fault = std::env::var_os("CINNABAR_SIGNAL_SOFTWARE_FAULT").is_some();
        let signal = if hardware || software_fault {
            libc::SIGSEGV
        } else {
            libc::SIGABRT
        };
        // SAFETY: This test runs in a disposable subprocess; disable core files and install its handler.
        unsafe {
            let limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &limit);
            if std::env::var_os("CINNABAR_SIGNAL_CHAIN").is_some() {
                if hardware {
                    let mut action: libc::sigaction = std::mem::zeroed();
                    action.sa_sigaction =
                        previous_siginfo_handler as *const () as libc::sighandler_t;
                    action.sa_flags = libc::SA_SIGINFO;
                    libc::sigaction(libc::SIGSEGV, &action, std::ptr::null_mut());
                } else {
                    libc::signal(signal, previous_handler as *const () as libc::sighandler_t);
                }
            } else {
                libc::signal(signal, libc::SIG_DFL);
            }
        }
        let mut identity = identity();
        identity.started_at_ms = 2;
        install(&layout(&root), identity, NO_HOOKS, None);
        let attempt = Attempt {
            backend: Backend::Metal,
            adapter: None,
            stage: Stage::CreatingDevice,
            explicit: false,
        };
        state::set_renderer(&root, Some(&attempt));
        unsafe {
            if hardware {
                let page_size = libc::sysconf(libc::_SC_PAGESIZE);
                assert!(page_size > 0);
                let address = libc::mmap(
                    std::ptr::null_mut(),
                    page_size as usize,
                    libc::PROT_NONE,
                    libc::MAP_PRIVATE | libc::MAP_ANON,
                    -1,
                    0,
                );
                assert_ne!(address, libc::MAP_FAILED);
                FAULT_ADDRESS.store(address, Ordering::Relaxed);
                std::ptr::read_volatile(address.cast::<u8>());
            } else {
                libc::raise(signal);
            }
        }
        panic!("fatal signal unexpectedly returned");
    }

    #[test]
    fn without_a_monitor_signals_write_a_folder_and_preserve_previous_handling() {
        use std::{os::unix::process::ExitStatusExt, process::Command};
        for (chain, hardware, software_fault) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, false, true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let logs = directory.path().join("logs");
            fs::create_dir_all(&logs).unwrap();
            fs::write(logs.join("client.log"), CLIENT_LOG).unwrap();
            fs::write(logs.join("core.log"), CORE_LOG).unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "lifecycle::crash::tests::signals::signal_fixture",
                    "--nocapture",
                ])
                .env("CINNABAR_SIGNAL_FIXTURE", directory.path());
            if chain {
                command.env("CINNABAR_SIGNAL_CHAIN", "1");
            }
            if hardware {
                command.env("CINNABAR_SIGNAL_HARDWARE", "1");
            }
            if software_fault {
                command.env("CINNABAR_SIGNAL_SOFTWARE_FAULT", "1");
            }
            let signal = if hardware || software_fault {
                libc::SIGSEGV
            } else {
                libc::SIGABRT
            };
            let mut child = command.spawn().unwrap();
            let mut status = None;
            let complete = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || {
                status = child.try_wait().unwrap();
                status.is_some()
            });
            if !complete {
                let _ = child.kill();
                let _ = child.wait();
            }
            assert!(complete, "signal fixture must terminate");
            let status = status.unwrap();
            if chain {
                assert_eq!(status.code(), Some(77));
            } else {
                assert_eq!(status.signal(), Some(signal));
            }
            let folders = folder::list(&directory.path().join("crashes"));
            assert_eq!(folders.len(), 1);
            assert!(
                folders[0]
                    .to_str()
                    .unwrap()
                    .ends_with("-0123456789abcdef-signal")
            );
            let report = folder::read(&folders[0]).unwrap();
            assert_eq!(report.kind, Kind::Native);
            assert_eq!(report.session_id, "0123456789abcdef");
            assert!(report.uptime_ms > 0);
            assert_eq!(report.thread.name, None);
            assert!(report.thread.id.parse::<u64>().is_ok());
            let native = report.native.unwrap();
            assert_eq!(native["signal"], signal);
            if hardware {
                assert!(native["signal_code"].as_i64().unwrap() > 0);
                assert!(native["fault_address"].as_u64().unwrap() > 0);
            }
            assert_eq!(report.renderer.unwrap()["stage"], "creating_device");
            assert_eq!(
                fs::read_to_string(folders[0].join(folder::CLIENT_LOG_FILE)).unwrap(),
                "CLIENT_SESSION_START session=0123456789abcdef\nclient line\n"
            );
            assert_eq!(
                fs::read_to_string(folders[0].join(folder::CORE_LOG_FILE)).unwrap(),
                "CORE_SESSION_START timestamp_ms=2\ncore line\n"
            );
        }
    }
}

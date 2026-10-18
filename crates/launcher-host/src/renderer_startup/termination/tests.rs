use super::*;
use crate::renderer_startup::{Attempt, Stage, StartupGuard};
use launcher::graphics_backend::Backend;

/// Creates an unfinished renderer without loading a graphics driver.
fn attempt() -> Attempt {
    Attempt {
        backend: Backend::Vulkan,
        adapter: None,
        stage: Stage::CreatingDevice,
        explicit: false,
    }
}

#[test]
fn termination_fixture() {
    let Some(root) = std::env::var_os("CINNABAR_TERMINATION_FIXTURE") else {
        return;
    };
    crate::lifecycle::children::install_exit_hooks(|| {});
    let mut guard = StartupGuard::open(Path::new(&root)).unwrap();
    guard.begin(attempt()).unwrap();
    match std::env::var("CINNABAR_TERMINATION_MODE").unwrap().as_str() {
        "panic" => panic!("renderer fixture panic must retain its sentinel"),
        "cancel" => {
            cancel_on_termination();
            assert!(guard.begin(attempt()).is_err());
            assert!(guard.presented().is_err());
            assert!(StartupGuard::pending(Path::new(&root)).unwrap().is_none());
            assert!(StartupGuard::open(Path::new(&root)).is_err());
        }
        mode => {
            let signal = match mode {
                "int" => libc::SIGINT,
                "term" => libc::SIGTERM,
                _ => panic!("unknown fixture mode"),
            };
            // SAFETY: The handler is installed, and this signal targets only this fixture.
            assert_eq!(unsafe { libc::raise(signal) }, 0);
            // Keep the guard alive until the handled-termination thread exits the process.
            test_time::eventually("handled signal exits the fixture", || false);
        }
    }
}

#[test]
fn handled_termination_cancels_startup_but_panics_remain_recoverable() {
    for ignored in [false, true] {
        for mode in ["int", "term", "cancel", "panic"] {
            let root = tempfile::tempdir().unwrap();
            // Repeated cancellations must never accumulate failed APIs.
            for _ in 0..2 {
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        "renderer_startup::termination::tests::termination_fixture",
                        "--nocapture",
                    ])
                    .env("CINNABAR_TERMINATION_FIXTURE", root.path())
                    .env("CINNABAR_TERMINATION_MODE", mode);
                // SAFETY: Only async-signal-safe signal setup runs between fork and exec.
                unsafe {
                    use std::os::unix::process::CommandExt;
                    command.pre_exec(move || {
                        let disposition = if ignored {
                            libc::SIG_IGN
                        } else {
                            libc::SIG_DFL
                        };
                        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                            if libc::signal(signal, disposition) == libc::SIG_ERR {
                                return Err(std::io::Error::last_os_error());
                            }
                        }
                        Ok(())
                    });
                }
                let child = crate::lifecycle::children::spawn(&mut command).unwrap();
                let mut status = None;
                let complete = test_time::wait_until(test_time::DEFAULT_TIMEOUT, || {
                    status = child.try_wait().unwrap();
                    status.is_some()
                });
                if !complete {
                    child.stop(std::time::Duration::ZERO);
                }
                assert!(complete, "{mode} fixture must terminate");
                assert_eq!(
                    status.unwrap().code(),
                    Some(match mode {
                        "int" | "term" => 130,
                        "cancel" => 0,
                        _ => 101,
                    })
                );
                let next = StartupGuard::open(root.path()).unwrap();
                if mode == "panic" {
                    assert_eq!(next.recovery.failed_backends(), [Backend::Vulkan]);
                } else {
                    assert!(next.interrupted.is_none());
                    assert!(next.recovery.failed_backends().is_empty());
                    assert!(next.recovery.working.is_none());
                }
            }
        }
    }
}

#[test]
fn completed_leases_cannot_cancel_another_startup() {
    let root = tempfile::tempdir().unwrap();
    let mut first = StartupGuard::open(root.path()).unwrap();
    first.begin(attempt()).unwrap();
    let mut active = Active::default();
    active.register(root.path(), first.lock.as_ref().unwrap());
    // A child can retain this descriptor between fork and exec.
    let inherited_descriptor = first.lock.as_ref().unwrap().0.try_clone().unwrap();
    first.presented().unwrap();
    let mut next = StartupGuard::open(root.path()).unwrap();
    next.begin(attempt()).unwrap();
    active.cancel();
    assert_eq!(StartupGuard::pending(root.path()).unwrap(), Some(attempt()));
    drop(inherited_descriptor);
}

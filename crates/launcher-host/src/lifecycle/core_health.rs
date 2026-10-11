//! Process supervision helpers: core crash-loop backoff and the bounded core and client logs.

use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    time::Duration,
};

use launcher::install_layout::InstallLayout;

const BASE_DELAY: Duration = Duration::from_millis(500);
const MAX_DELAY: Duration = Duration::from_secs(8);
const MAX_CONSECUTIVE_FAILURES: u32 = 5;
const STABLE_RUN: Duration = Duration::from_secs(60);
const MAX_LOG_BYTES: u64 = 1 << 20;
#[cfg(unix)]
const MAX_CLIENT_LOG_BYTES: u64 = 8 << 20;
#[cfg(unix)]
const CLIENT_LOG_CHECK: Duration = Duration::from_secs(30);

/// Exponential restart delay that resets once a core stays up for a stable interval.
#[derive(Debug, Default)]
pub struct RestartBackoff {
    failures: u32,
}

impl RestartBackoff {
    /// Delay before the next restart after a core that ran for `ran_for` died; `None` means stop retrying.
    // Consumed by the session reconnect path once the core is restarted mid-session.
    #[allow(dead_code)]
    pub fn next_delay(&mut self, ran_for: Duration) -> Option<Duration> {
        if ran_for >= STABLE_RUN {
            self.failures = 0;
        }
        self.failures += 1;
        if self.failures > MAX_CONSECUTIVE_FAILURES {
            return None;
        }
        Some((BASE_DELAY * 2u32.pow(self.failures - 1)).min(MAX_DELAY))
    }
}

/// Opens the core's append-only stderr log, rotating one previous generation past the size bound.
pub fn open_core_log(layout: &InstallLayout) -> Option<File> {
    open_log(&layout.log_dir(), "core.log", MAX_LOG_BYTES)
}

/// Sends stderr to `logs/client.log` when no terminal is attached, as in a Finder launch. Each launch,
/// and each overflow of the size bound, moves the current file to `client.log.1`.
#[cfg(unix)]
pub fn capture_client_stderr(layout: &InstallLayout) {
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() {
        return;
    }
    let dir = layout.log_dir();
    let redirect = move |rotate_past| {
        open_log(&dir, "client.log", rotate_past)
            .is_some_and(|log| rustix::stdio::dup2_stderr(&log).is_ok())
    };
    if !redirect(0) {
        return;
    }
    let path = layout.log_dir().join("client.log");
    let _ = std::thread::Builder::new()
        .name("client-log-rotate".to_owned())
        .spawn(move || {
            loop {
                std::thread::sleep(CLIENT_LOG_CHECK);
                if fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_CLIENT_LOG_BYTES) {
                    redirect(MAX_CLIENT_LOG_BYTES);
                }
            }
        });
}

#[cfg(not(unix))]
pub fn capture_client_stderr(_layout: &InstallLayout) {}

fn open_log(dir: &Path, name: &str, rotate_past: u64) -> Option<File> {
    // Tests never append to or rotate a real install's logs, such as a worktree's shared `.local`.
    #[cfg(any(test, feature = "test-support"))]
    if !dir.starts_with(std::env::temp_dir()) {
        return None;
    }
    fs::create_dir_all(dir).ok()?;
    let path = dir.join(name);
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > rotate_past) {
        let _ = fs::rename(&path, dir.join(format!("{name}.1")));
    }
    OpenOptions::new().create(true).append(true).open(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_doubles_caps_and_gives_up() {
        let mut backoff = RestartBackoff::default();
        let quick = Duration::from_secs(1);
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_millis(500)));
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_secs(1)));
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_secs(2)));
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_secs(4)));
        assert_eq!(backoff.next_delay(quick), Some(Duration::from_secs(8)));
        assert_eq!(backoff.next_delay(quick), None);
    }

    #[test]
    fn tests_cannot_open_the_discovered_install_logs() {
        let layout = launcher::test_support::checkout();
        assert!(open_core_log(&layout).is_none());
        let scratch = launcher::test_support::scratch("core-log");
        assert!(open_core_log(&scratch).is_some());
        assert!(scratch.log_dir().join("core.log").is_file());
    }

    #[test]
    fn a_stable_run_resets_the_failure_count() {
        let mut backoff = RestartBackoff::default();
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            backoff.next_delay(Duration::from_secs(1));
        }
        assert_eq!(backoff.next_delay(STABLE_RUN), Some(BASE_DELAY));
    }
}

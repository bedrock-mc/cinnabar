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
const MAX_CLIENT_LOG_BYTES: u64 = 8 << 20;

/// Exponential restart delay that resets once a core stays up for a stable interval.
#[derive(Debug, Default)]
pub(crate) struct RestartBackoff {
    failures: u32,
}

impl RestartBackoff {
    /// Delay before the next restart after a core that ran for `ran_for` died; `None` means stop retrying.
    // Consumed by the session reconnect path once the core is restarted mid-session.
    #[allow(dead_code)]
    pub(crate) fn next_delay(&mut self, ran_for: Duration) -> Option<Duration> {
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
pub(crate) fn open_core_log(layout: &InstallLayout) -> Option<File> {
    let mut log = open_log(&layout.log_dir(), "core.log", MAX_LOG_BYTES)?;
    use std::io::Write;
    let _ = writeln!(
        log,
        "CORE_SESSION_START timestamp_ms={}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    Some(log)
}

/// Mirrors terminal diagnostics and captures raw stderr into the rotating log without a terminal.
pub(crate) fn capture_client_logs(layout: &InstallLayout) {
    #[cfg(test)]
    if !layout.log_dir().starts_with(std::env::temp_dir()) {
        return;
    }
    if let Err(error) = diagnostics::console::initialize_log(
        &layout.log_dir().join("client.log"),
        MAX_CLIENT_LOG_BYTES,
    ) {
        eprintln!("client file logging unavailable: {error}");
        return;
    }
    use std::io::Write;
    let _ = writeln!(
        diagnostics::console::stderr(),
        "CLIENT_SESSION_START version={} commit={} timestamp_ms={}",
        env!("CARGO_PKG_VERSION"),
        option_env!("RUST_MCBE_BUILD_COMMIT").unwrap_or("unknown"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
}

/// Opens the bounded append log, preserving one previous generation.
fn open_log(dir: &Path, name: &str, rotate_past: u64) -> Option<File> {
    // Tests never append to or rotate a real install's logs, such as a worktree's shared `.local`.
    #[cfg(test)]
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
        let layout = crate::install_layout::checkout();
        assert!(open_core_log(&layout).is_none());
        let scratch = crate::install_layout::scratch("core-log");
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

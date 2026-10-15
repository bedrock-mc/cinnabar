//! Client diagnostics stay at the app boundary, outside engine-free launcher services.
use crash_report::section::{CLIENT_SESSION_START, CORE_SESSION_START};
use diagnostics::{
    console::SessionFiles,
    logging::Category,
    session,
    session_log::{Family, Retention, SessionLog},
};
use launcher::install_layout::InstallLayout;

/// The client's session files: `logs/client.log`, earlier sessions as `client-<UTC time>.log`.
const CLIENT_LOG_STEM: &str = "client";
/// Every core's output this session: `logs/core.log`, earlier sessions as `core-<UTC time>.log`.
const CORE_LOG_STEM: &str = "core";

/// Starts this session's `client.log`, archiving the previous session's, before startup can
/// fail; output logged since the subscriber was installed follows the session header.
pub(super) fn capture(layout: &InstallLayout) {
    #[cfg(test)]
    if !layout.log_dir().starts_with(std::env::temp_dir()) {
        return;
    }
    let dir = layout.log_dir();
    let header = session_header();
    let files = SessionFiles {
        dir: &dir,
        stem: CLIENT_LOG_STEM,
        header: &header,
        json: diagnostics::logging::json_enabled(),
        retention: Retention::DEFAULT,
    };
    if let Err(error) = diagnostics::console::initialize_log(&files) {
        bevy::log::warn!(
            "client file logging unavailable for {}: {error}",
            Family::new(files.dir, files.stem, "log")
                .current()
                .display()
        );
    }
}

/// The first line of each client session file; devtool and crash reports find sessions by it.
fn session_header() -> String {
    format!(
        "{CLIENT_SESSION_START} version={} commit={} profile={} session={} timestamp_ms={}",
        env!("CARGO_PKG_VERSION"),
        session::COMMIT,
        session::BUILD_PROFILE,
        session::id(),
        session::started_at_ms()
    )
}

/// Starts this session's `core.log`, archiving the previous session's, and routes every core
/// spawned afterwards into it at the `core` log category's level, with crash reports for cores
/// that crash.
pub(super) fn capture_core(layout: &InstallLayout) {
    #[cfg(test)]
    if !layout.log_dir().starts_with(std::env::temp_dir()) {
        return;
    }
    let family = Family::new(layout.log_dir(), CORE_LOG_STEM, "log");
    // Crash reports and `devtool diag` match sections to this session by exactly this header.
    let header = format!(
        "{CORE_SESSION_START} timestamp_ms={} session={}\n",
        session::started_at_ms(),
        session::id()
    );
    let log = match SessionLog::open(family.clone(), Retention::DEFAULT, header.into_bytes()) {
        Ok(log) => log,
        Err(error) => {
            bevy::log::warn!(
                "core file logging unavailable for {}: {error}",
                family.current().display()
            );
            return;
        }
    };
    let config = launcher_host::lifecycle::core_output::Config {
        level: || diagnostics::logging::category_filter(Category::Core),
        log: Box::new(log),
    };
    if let Err(error) = launcher_host::lifecycle::core_output::install(config) {
        bevy::log::warn!("core output routing unavailable: {error}");
    }
}

/// Captures every panic and native fault, flushing the client log around it; `monitor` starts
/// the crash monitor, without which reports are written in-process. The packet log and the log
/// flight recorder then move into the crash staging directory, where reports read them.
///
/// When raw stderr is not captured, the default hook's panic text never reaches the client
/// log, so it is recorded there with the report's backtrace.
pub(super) fn install_crash_capture(
    layout: &InstallLayout,
    monitor: Option<&launcher_host::lifecycle::crash::monitor::MonitorCommand>,
) {
    let identity = crash_report::report::Identity {
        session_id: session::id().to_owned(),
        started_at_ms: session::started_at_ms(),
        release: env!("CARGO_PKG_VERSION").to_owned(),
        commit: session::COMMIT.to_owned(),
        build_profile: session::BUILD_PROFILE.to_owned(),
    };
    let hooks = launcher_host::lifecycle::crash::LogHooks {
        log_panic: |panic| {
            let console = diagnostics::console::console();
            if !console.captures_raw_stderr() {
                console.write(
                    diagnostics::console::Stream::FileOnly,
                    panic.as_bytes().to_vec(),
                );
            }
        },
        flush: || {
            diagnostics::console::flush_before_exit();
        },
        drain_on_signal: diagnostics::console::drain_raw_stderr_on_signal,
    };
    launcher_host::lifecycle::crash::install(layout, identity, hooks, monitor);
    if let Some(staging) = launcher_host::lifecycle::crash::staging_dir() {
        if let Err(error) = client_session::packet_log::global().attach(staging) {
            bevy::log::warn!(%error, "packet ring stays out of crash reports");
        }
        if let Err(error) = diagnostics::logging::flight::attach(staging) {
            bevy::log::warn!(%error, "flight recorder stays out of crash reports");
        }
    }
}

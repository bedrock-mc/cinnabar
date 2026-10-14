//! Client diagnostics stay at the app boundary, outside engine-free launcher services.
use launcher::install_layout::InstallLayout;
use std::io::{IsTerminal, Write};

const MAX_CLIENT_LOG_BYTES: u64 = 8 << 20;

/// Mirrors diagnostics to the rotating client log before startup can fail.
pub(super) fn capture(layout: &InstallLayout) {
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
    let _ = writeln!(
        diagnostics::console::stderr(),
        "CLIENT_SESSION_START version={} commit={} timestamp_ms={}",
        env!("CARGO_PKG_VERSION"),
        option_env!("CINNABAR_BUILD_COMMIT").unwrap_or("unknown"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
}

/// Writes a crash folder for each panic or native fault, flushing the client log around it.
///
/// The default hook prints terminal panics only to the terminal, so they are also recorded in
/// the client log, with the report's backtrace.
pub(super) fn install_panic_hook(layout: &InstallLayout) {
    let identity = crash_report::report::Identity {
        session_id: diagnostics::session::id().to_owned(),
        started_at_ms: diagnostics::session::started_at_ms(),
        release: env!("CARGO_PKG_VERSION").to_owned(),
        commit: diagnostics::session::COMMIT.to_owned(),
        build_profile: diagnostics::session::BUILD_PROFILE.to_owned(),
    };
    launcher_host::lifecycle::crash::install_panic_hook(
        layout,
        identity,
        |panic| {
            if std::io::stderr().is_terminal() {
                diagnostics::console::console().write(
                    diagnostics::console::Stream::FileOnly,
                    panic.as_bytes().to_vec(),
                );
            }
        },
        || {
            diagnostics::console::flush_before_exit();
        },
    );
}

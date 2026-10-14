//! Client diagnostics stay at the app boundary, outside engine-free launcher services.
use launcher::install_layout::InstallLayout;
use std::io::Write;

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
pub(super) fn install_panic_hook(layout: &InstallLayout) {
    let identity = crash_report::report::Identity {
        session_id: diagnostics::session::id().to_owned(),
        started_at_ms: diagnostics::session::started_at_ms(),
        release: env!("CARGO_PKG_VERSION").to_owned(),
        commit: diagnostics::session::COMMIT.to_owned(),
        build_profile: diagnostics::session::BUILD_PROFILE.to_owned(),
    };
    launcher_host::lifecycle::crash::install_panic_hook(layout, identity, || {
        diagnostics::console::flush_before_exit();
    });
}

//! Client diagnostics stay at the app boundary, outside engine-free launcher services.
use launcher::install_layout::InstallLayout;
use std::{
    backtrace::Backtrace,
    io::{IsTerminal, Write},
};

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
        option_env!("RUST_MCBE_BUILD_COMMIT").unwrap_or("unknown"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
}

/// Records terminal panics in the client log and flushes around the launcher panic hook.
pub(super) fn install_panic_hook(layout: &InstallLayout) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::io::stderr().is_terminal() {
            diagnostics::console::console().write(
                diagnostics::console::Stream::FileOnly,
                format!("{info}\n{}\n", Backtrace::force_capture()).into_bytes(),
            );
        }
        previous(info);
    }));
    launcher_host::lifecycle::crash::install_panic_hook(layout, || {
        diagnostics::console::flush_before_exit();
    });
}

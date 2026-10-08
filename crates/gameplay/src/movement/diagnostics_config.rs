//! Diagnostic switches supplied by the application marker catalog.
use std::sync::OnceLock;

/// Diagnostic environment names owned by the application.
#[derive(Debug, Clone, Copy)]
pub struct MovementDiagnostics {
    pub movement_trace: &'static str,
    pub teleport_ack: &'static str,
    pub anchor_probe: &'static str,
}

static DIAGNOSTICS: OnceLock<MovementDiagnostics> = OnceLock::new();

/// Installs marker names before gameplay resources are constructed.
pub fn configure(diagnostics: MovementDiagnostics) {
    let _ = DIAGNOSTICS.set(diagnostics);
}

/// Returns the configured environment name's current value, if configured.
pub(super) fn value(
    name: impl FnOnce(&MovementDiagnostics) -> &'static str,
) -> Option<std::ffi::OsString> {
    DIAGNOSTICS
        .get()
        .and_then(|config| std::env::var_os(name(config)))
}

//! Configures client logs and their ordered console writer.

/// Keeps the existing log filter and profiling layer while queuing console writes.
pub(super) fn plugin() -> bevy::log::LogPlugin {
    // Rich presence reports a closed Discord client once instead of on every retry.
    let filter = format!(
        "{}discord_presence::connection=off",
        bevy::log::DEFAULT_FILTER
    );
    #[cfg(feature = "tracy")]
    let filter = filter + crate::tracy::WGPU_SCOPE_FILTER;
    bevy::log::LogPlugin {
        filter,
        #[cfg(feature = "tracy")]
        custom_layer: crate::tracy::layer,
        fmt_layer: |_| {
            Some(Box::new(
                bevy::log::tracing_subscriber::fmt::Layer::default()
                    .with_writer(diagnostics::console::stderr),
            ))
        },
        ..Default::default()
    }
}

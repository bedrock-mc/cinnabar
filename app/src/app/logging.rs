//! Client logging setup and ordered startup carrier summaries.

use assets::carriers::startup::{LoadedAssetKind, LoadedAssets};

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

/// Reports the selected core carriers in startup order, preserving the first carrier notice.
pub(super) fn loaded_assets(loaded: &LoadedAssets) {
    if let Some(notice) = &loaded.notice {
        diagnostics::log_stderr!("{notice}");
    } else if loaded.kind == LoadedAssetKind::CompiledBlob {
        diagnostics::log_stderr!(
            "loaded compiled block assets from {} (sha256 {})",
            loaded.selected_path.display(),
            loaded.metrics.blob_sha256
        );
    }
    diagnostics::log_stderr!(
        "loaded required atmosphere assets from {}",
        loaded.atmosphere.selected_path().display()
    );
    diagnostics::log_stderr!(
        "{}",
        crate::asset_bindings::AtmosphereEvidence::startup_summary(&loaded.atmosphere)
    );
    diagnostics::log_stderr!(
        "loaded required entity assets from {}",
        loaded.entities.selected_path().display()
    );
    diagnostics::log_stderr!("{}", loaded.entities.startup_summary());
    diagnostics::log_stderr!("{}", loaded.fonts.startup_summary());
}

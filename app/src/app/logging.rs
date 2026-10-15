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
/// The `ATMOSPHERE_EVIDENCE` and `ENTITY_ASSET_EVIDENCE` marker lines stay raw console lines.
pub(super) fn loaded_assets(loaded: &LoadedAssets) {
    if let Some(notice) = &loaded.notice {
        bevy::log::warn!("{notice}");
    } else if loaded.kind == LoadedAssetKind::CompiledBlob {
        bevy::log::info!(
            "loaded compiled block assets from {} (sha256 {})",
            loaded.selected_path.display(),
            loaded.metrics.blob_sha256
        );
    }
    bevy::log::info!(
        "loaded required atmosphere assets from {}",
        loaded.atmosphere.selected_path().display()
    );
    diagnostics::log_stderr!(
        "{}",
        crate::asset_bindings::AtmosphereEvidence::startup_summary(&loaded.atmosphere)
    );
    bevy::log::info!(
        "loaded required entity assets from {}",
        loaded.entities.selected_path().display()
    );
    diagnostics::log_stderr!("{}", loaded.entities.startup_summary());
    if loaded.fonts.is_diagnostic() {
        bevy::log::warn!("{}", loaded.fonts.startup_summary());
    } else {
        bevy::log::info!("{}", loaded.fonts.startup_summary());
    }
}

/// Creates the client app before startup workers can emit diagnostics.
pub(super) fn startup_app() -> bevy::app::App {
    let mut app = bevy::app::App::new();
    app.add_plugins(plugin());
    app
}

#[cfg(test)]
mod tests {
    #[test]
    fn startup_workers_have_a_log_subscriber() {
        const CHILD: &str = "CINNABAR_STARTUP_LOG_TEST";
        const MESSAGE: &str = "startup worker diagnostic captured";
        if std::env::var_os(CHILD).is_some() {
            let _app = super::startup_app();
            std::thread::spawn(|| bevy::log::info!("{MESSAGE}"))
                .join()
                .unwrap();
            diagnostics::console::flush_before_exit();
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::logging::tests::startup_workers_have_a_log_subscriber",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("RUST_LOG", "info")
            .output()
            .unwrap();
        assert!(output.status.success(), "child test failed: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(MESSAGE),
            "startup worker diagnostic was lost: {output:?}"
        );
    }
}

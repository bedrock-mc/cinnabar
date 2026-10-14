//! Client logging setup and ordered startup carrier summaries.

use assets::carriers::startup::{LoadedAssetKind, LoadedAssets};
use bevy::{app::PluginGroupBuilder, prelude::App};
use diagnostics::logging::CallerLayer;

/// Which process installs logging: the client adds the build's profiler layers, the
/// first-run setup window and the crash monitor do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Process {
    Client,
    Setup,
    Monitor,
}

/// Keeps wgpu's encoding and submission scopes for Tracy.
#[cfg(feature = "tracy")]
const PROFILER_FILTER: &str = crate::tracy::WGPU_SCOPE_FILTER;
#[cfg(not(feature = "tracy"))]
const PROFILER_FILTER: &str = "";

/// Bevy's default filter at `info`, which `LogPlugin` applied before this subscriber replaced
/// it, without the closed-Discord retry noise.
fn base_filter() -> String {
    // Rich presence reports a closed Discord client once instead of on every retry.
    format!(
        "info,{}discord_presence::connection=off{PROFILER_FILTER}",
        bevy::log::DEFAULT_FILTER
    )
}

/// Installs the process `tracing` subscriber before anything logs; `LogPlugin` stays
/// disabled, so this is the only subscriber.
pub(crate) fn install(process: Process) {
    let layers = match process {
        Process::Client => profiler_layers(),
        Process::Setup | Process::Monitor => Vec::new(),
    };
    let installed = diagnostics::logging::install(diagnostics::logging::Options {
        base_filter: base_filter(),
        omit_frame_marks: cfg!(feature = "tracy"),
        layers,
    });
    if let Err(error) = &installed {
        bevy::log::warn!("{error}");
    }
    #[cfg(any(feature = "tracy", feature = "frame-trace"))]
    if process == Process::Client && installed.is_ok() {
        span_trace_on_panic();
    }
}

/// The profiler layers this build enables.
#[cfg(any(feature = "tracy", feature = "frame-trace"))]
#[allow(
    clippy::vec_init_then_push,
    reason = "each push depends on its own feature"
)]
fn profiler_layers() -> Vec<CallerLayer> {
    let mut layers = Vec::new();
    #[cfg(feature = "tracy")]
    layers.push(crate::tracy::layer());
    #[cfg(feature = "frame-trace")]
    layers.push(chrome::layer());
    layers
}

#[cfg(not(any(feature = "tracy", feature = "frame-trace")))]
fn profiler_layers() -> Vec<CallerLayer> {
    Vec::new()
}

/// Writes the panicking thread's span trace before the panic report, as Bevy's `trace`
/// feature did while `LogPlugin` installed the subscriber.
#[cfg(any(feature = "tracy", feature = "frame-trace"))]
fn span_trace_on_panic() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(trace) = diagnostics::logging::span_trace() {
            diagnostics::log_stderr!("{trace}");
        }
        previous(info);
    }));
}

/// Adds Bevy's default plugins without `LogPlugin`, whose subscriber [`install`] replaced
/// before startup, and keeps the Chrome trace writer for the app's lifetime.
pub(super) fn add_default_plugins(app: &mut App, plugins: PluginGroupBuilder) {
    app.add_plugins(plugins.disable::<bevy::log::LogPlugin>());
    #[cfg(feature = "frame-trace")]
    chrome::keep_writer(app);
}

/// Bevy's Chrome trace layer under `frame-trace`.
#[cfg(feature = "frame-trace")]
mod chrome {
    use bevy::{
        log::tracing_subscriber::{
            Registry,
            fmt::{FormattedFields, format::DefaultFields},
        },
        platform::cell::SyncCell,
        prelude::*,
    };
    use diagnostics::logging::CallerLayer;
    use std::sync::{Mutex, PoisonError};
    use tracing_chrome::{ChromeLayerBuilder, EventOrSpan, FlushGuard};

    /// The trace writer until the app holds it.
    static WRITER: Mutex<Option<FlushGuard>> = Mutex::new(None);

    /// Keeps the trace writer running; dropping it with the app finishes the trace file.
    #[derive(Resource)]
    #[expect(
        dead_code,
        reason = "held only so dropping the app finishes the trace file"
    )]
    struct TraceWriter(SyncCell<FlushGuard>);

    /// Writes to `TRACE_CHROME`, or a timestamped file in the working directory, naming spans
    /// with their recorded fields.
    pub(super) fn layer() -> CallerLayer {
        let mut builder = ChromeLayerBuilder::<Registry>::new();
        if let Ok(path) = std::env::var("TRACE_CHROME") {
            builder = builder.file(path);
        }
        let (layer, writer) = builder
            .name_fn(Box::new(|event_or_span| match event_or_span {
                EventOrSpan::Event(event) => event.metadata().name().into(),
                EventOrSpan::Span(span) => span
                    .extensions()
                    .get::<FormattedFields<DefaultFields>>()
                    .map_or_else(
                        || span.metadata().name().into(),
                        |fields| format!("{}: {}", span.metadata().name(), fields.fields.as_str()),
                    ),
            }))
            .build();
        *WRITER.lock().unwrap_or_else(PoisonError::into_inner) = Some(writer);
        Box::new(layer)
    }

    /// Hands the trace writer to `app`, which finishes the trace file when it is dropped.
    pub(super) fn keep_writer(app: &mut App) {
        if let Some(writer) = WRITER.lock().unwrap_or_else(PoisonError::into_inner).take() {
            app.insert_resource(TraceWriter(SyncCell::new(writer)));
        }
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

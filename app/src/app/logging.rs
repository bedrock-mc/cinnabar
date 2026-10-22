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
    /// The crash monitor, which iOS cannot start.
    #[cfg(not(target_os = "ios"))]
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
        Process::Setup => Vec::new(),
        #[cfg(not(target_os = "ios"))]
        Process::Monitor => Vec::new(),
    };
    let installed = diagnostics::logging::install(diagnostics::logging::Options {
        base_filter: base_filter(),
        omit_frame_marks: cfg!(feature = "tracy"),
        layers,
        flight_recorder: process == Process::Client,
    });
    if let Err(error) = &installed {
        bevy::log::warn!("{error}");
    }
    #[cfg(any(feature = "tracy", feature = "frame-trace"))]
    if process == Process::Client && installed.is_ok() {
        span_trace_on_panic();
    }
}

/// The spec the Verbose logging setting layers over the defaults.
const VERBOSE_SPEC: &str = "net=debug,session=debug,core=debug,packs=debug";

/// The spec the Verbose logging setting asks for: [`VERBOSE_SPEC`] when on, the defaults when
/// off, and `None` when `CINNABAR_LOG` or `RUST_LOG` chose this process's filter, which the
/// setting never overrides.
fn verbose_setting_spec(verbose: bool, environment_chose: bool) -> Option<&'static str> {
    (!environment_chose).then_some(if verbose { VERBOSE_SPEC } else { "" })
}

/// Whether `CINNABAR_LOG` or `RUST_LOG` is set to a non-empty filter.
fn environment_chose_filter() -> bool {
    let set = |value: Option<std::ffi::OsString>| value.is_some_and(|value| !value.is_empty());
    set(env_compat::var_os(diagnostics::markers::LOG)) || set(std::env::var_os("RUST_LOG"))
}

/// Applies the Verbose logging setting to the installed filter, unless the environment chose
/// it.
fn apply_verbose_setting(verbose: bool) {
    let Some(spec) = verbose_setting_spec(verbose, environment_chose_filter()) else {
        return;
    };
    if let Err(error) = diagnostics::logging::set_filter(spec) {
        bevy::log::warn!("verbose logging setting not applied: {error}");
    }
}

/// Applies the saved Verbose logging setting at launch, once the install layout names the
/// settings file. Off leaves the installed filter as it is.
pub(crate) fn apply_saved_verbose_setting(layout: &launcher::install_layout::InstallLayout) {
    let saved = launcher::menu::settings_options::SettingsOptions::load(
        &layout
            .server_file()
            .with_file_name(launcher::menu::settings_options::SETTINGS_FILE),
    );
    if saved.verbose_logging() {
        apply_verbose_setting(true);
    }
}

/// Applies Verbose logging changes made in Settings as they happen. The first run records the
/// value [`apply_saved_verbose_setting`] already applied.
pub(super) fn apply_verbose_setting_changes(
    menu: Option<bevy::prelude::Res<client_shell::menu::MenuRuntime>>,
    mut applied: bevy::prelude::Local<Option<bool>>,
) {
    let Some(menu) = menu else {
        return;
    };
    let verbose = menu.settings_snapshot().0.verbose_logging();
    if applied
        .replace(verbose)
        .is_some_and(|previous| previous != verbose)
    {
        apply_verbose_setting(verbose);
    }
}

/// The profiler layers this build enables.
fn profiler_layers() -> Vec<CallerLayer> {
    Vec::from([
        #[cfg(feature = "tracy")]
        crate::tracy::layer(),
        #[cfg(feature = "frame-trace")]
        chrome::layer(),
    ])
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

#[cfg(test)]
mod verbose_setting_tests {
    use super::{VERBOSE_SPEC, verbose_setting_spec};

    #[test]
    fn the_setting_layers_verbose_categories_unless_the_environment_chose_the_filter() {
        assert_eq!(verbose_setting_spec(true, false), Some(VERBOSE_SPEC));
        assert_eq!(verbose_setting_spec(false, false), Some(""));
        assert_eq!(verbose_setting_spec(true, true), None);
        assert_eq!(verbose_setting_spec(false, true), None);
    }

    #[test]
    fn the_verbose_spec_is_valid_category_directives() {
        for directive in VERBOSE_SPEC.split(',') {
            let (name, level) = directive.split_once('=').unwrap();
            assert!(diagnostics::logging::Category::from_name(name).is_some());
            assert_eq!(level, "debug");
        }
    }
}

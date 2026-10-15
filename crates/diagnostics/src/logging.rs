//! The process `tracing` subscriber.
//!
//! [`install`] builds one subscriber for the whole process. The session-file stack, under a
//! reloadable `EnvFilter`, holds caller layers (such as a profiler), span traces for errors, a
//! formatter writing through [`crate::console`], an optional JSON Lines record
//! (`CINNABAR_LOG_JSON=1`), and on Android a logcat copy. Beside it, under a filter of its own,
//! the [`flight`] recorder keeps the last events in a ring crash reports read. Both filters are
//! per-layer, so neither hides events from the other. `log`-crate records are forwarded in.
//!
//! The session-file filter is the caller's base directives, then `RUST_LOG`, then
//! `CINNABAR_LOG`, each overriding earlier directives for the same target. `CINNABAR_LOG` and
//! [`set_filter`] accept [`Category`] directives (`net=debug`) as well as raw `EnvFilter`
//! directives.

#[cfg(target_os = "android")]
mod android;
mod category;
pub mod flight;
mod json;
#[cfg(test)]
mod tests;

pub use category::{Category, InvalidDirective};

use crate::{
    console::{self, Priority, Queued, Stream},
    markers,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tracing::{Metadata, Subscriber};
use tracing_subscriber::{
    EnvFilter, Layer, Registry,
    filter::{Directive, FilterFn, LevelFilter},
    fmt::{self, MakeWriter},
    layer::SubscriberExt,
    registry::LookupSpan,
    reload,
};

/// A layer the caller adds beneath the session-file filter, such as a profiler.
pub type CallerLayer = Box<dyn Layer<Registry> + Send + Sync>;

/// What the caller supplies to [`install`].
#[derive(Default)]
pub struct Options {
    /// Directives every filter starts from, including the default level, e.g. `info,wgpu=error`.
    pub base_filter: String,
    /// Keeps events carrying a `tracy.frame_mark` field out of the formatted output.
    pub omit_frame_marks: bool,
    /// Layers that see every span and event the session-file filter enables.
    pub layers: Vec<CallerLayer>,
    /// Adds the [`flight`] recorder.
    pub flight_recorder: bool,
}

/// Why [`install`] did not install the subscriber.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallError {
    /// A global subscriber already exists.
    AlreadyInstalled,
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a global tracing subscriber is already installed")
    }
}

impl std::error::Error for InstallError {}

/// Why [`set_filter`] left the filter unchanged.
#[derive(Debug, PartialEq, Eq)]
pub enum FilterError {
    NotInstalled,
    Invalid(InvalidDirective),
    Reload(String),
}

impl std::fmt::Display for FilterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled => formatter.write_str("logging is not installed"),
            Self::Invalid(invalid) => write!(
                formatter,
                "invalid log directive `{}`: {}",
                invalid.directive, invalid.reason
            ),
            Self::Reload(reason) => write!(formatter, "log filter reload failed: {reason}"),
        }
    }
}

impl std::error::Error for FilterError {}

/// The active filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterStatus {
    /// The `CINNABAR_LOG`-style spec layered over the defaults; empty when none.
    pub spec: String,
    /// Every active directive, as `EnvFilter` displays them.
    pub effective: String,
}

struct Spec {
    text: String,
    directives: Vec<String>,
}

struct Logging {
    handle: reload::Handle<EnvFilter, Registry>,
    base: String,
    rust_log: Vec<String>,
    spec: Mutex<Spec>,
}

impl Logging {
    fn spec(&self) -> std::sync::MutexGuard<'_, Spec> {
        self.spec.lock().unwrap_or_else(|error| error.into_inner())
    }
}

static LOGGING: OnceLock<Logging> = OnceLock::new();
/// Counts filter changes; see [`filter_generation`].
static FILTER_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Builds the filter: `base`, then `rust_log`, then `spec`, later directives overriding earlier
/// ones for the same target. Every directive in `rust_log` and `spec` must already be valid.
fn build_filter(base: &str, rust_log: &[String], spec: &[String]) -> EnvFilter {
    rust_log
        .iter()
        .chain(spec)
        .filter_map(|directive| directive.parse::<Directive>().ok())
        .fold(
            EnvFilter::builder().parse_lossy(base),
            EnvFilter::add_directive,
        )
}

/// `RUST_LOG`'s directives, all or none: a malformed variable is ignored entirely, as Bevy did.
fn parse_rust_log(value: &str) -> Result<Vec<String>, InvalidDirective> {
    category::split(value)
        .map(|directive| {
            directive
                .parse::<Directive>()
                .map(|_| directive.to_owned())
                .map_err(|error| InvalidDirective {
                    directive: directive.to_owned(),
                    reason: error.to_string(),
                })
        })
        .collect()
}

/// The console writer for formatted events; warnings and errors get the queue's reserve.
#[derive(Clone, Copy, Debug, Default)]
struct LogWriter;

impl<'a> MakeWriter<'a> for LogWriter {
    type Writer = Queued;

    fn make_writer(&'a self) -> Queued {
        console::stderr()
    }

    fn make_writer_for(&'a self, metadata: &Metadata<'_>) -> Queued {
        Queued::new(Stream::Stderr, Priority::of(metadata.level()))
    }
}

/// The session-file format: timestamp, level, thread name, span context, target, message,
/// then fields. The console strips its colours from the file copy.
fn formatter<S>() -> fmt::Layer<S, fmt::format::DefaultFields, fmt::format::Format, LogWriter>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fmt::layer().with_thread_names(true).with_writer(LogWriter)
}

/// Whether `CINNABAR_LOG_JSON=1` asks for the JSON Lines record.
pub fn json_enabled() -> bool {
    env_compat::var(markers::LOG_JSON).is_ok_and(|value| value == "1")
}

/// Installs the global subscriber and forwards `log` records into it. Invalid `RUST_LOG` or
/// `CINNABAR_LOG` directives are skipped and reported as warnings once installed.
pub fn install(options: Options) -> Result<(), InstallError> {
    if LOGGING.get().is_some() {
        return Err(InstallError::AlreadyInstalled);
    }
    let mut rejected = Vec::new();
    let rust_log = match parse_rust_log(&std::env::var(EnvFilter::DEFAULT_ENV).unwrap_or_default())
    {
        Ok(directives) => directives,
        Err(invalid) => {
            rejected.push((EnvFilter::DEFAULT_ENV, invalid));
            Vec::new()
        }
    };
    let spec_text = env_compat::var(markers::LOG).unwrap_or_default();
    let (directives, invalid) = category::expand_lossy(&spec_text);
    rejected.extend(invalid.into_iter().map(|invalid| (markers::LOG, invalid)));
    let (filter, handle) =
        reload::Layer::new(build_filter(&options.base_filter, &rust_log, &directives));
    let (formatted, frame_filtered) = if options.omit_frame_marks {
        let without_marks = FilterFn::new(|metadata: &Metadata<'_>| {
            metadata.fields().field("tracy.frame_mark").is_none()
        });
        (None, Some(formatter().with_filter(without_marks)))
    } else {
        (Some(formatter()), None)
    };
    let json = json_enabled().then(|| {
        json::JsonLayer::new(|priority, line| {
            console::console().write_at(Stream::Json, priority, line);
        })
    });
    let output = tracing_error::ErrorLayer::default()
        .and_then(formatted)
        .and_then(frame_filtered)
        .and_then(json);
    #[cfg(target_os = "android")]
    let output = output.and_then(android::AndroidLayer);
    // Caller layers sit in an `Option` because an empty `Vec` layer reports `Interest::never`
    // for every callsite, which would disable all events beneath the filter.
    let callers = (!options.layers.is_empty()).then_some(options.layers);
    let session = Layer::<Registry>::and_then(callers, output).with_filter(filter);
    let recorder = options
        .flight_recorder
        .then(|| flight::FlightLayer.with_filter(flight::filter(&options.base_filter)));
    let subscriber = Registry::default().with(session).with(recorder);
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| InstallError::AlreadyInstalled)?;
    let logger = tracing_log::LogTracer::init();
    let _ = LOGGING.set(Logging {
        handle,
        base: options.base_filter,
        rust_log,
        spec: Mutex::new(Spec {
            text: spec_text.trim().to_owned(),
            directives,
        }),
    });
    FILTER_GENERATION.fetch_add(1, Ordering::Release);
    if logger.is_err() {
        tracing::warn!("another `log` logger is installed; its records bypass the log file");
    }
    for (variable, invalid) in rejected {
        tracing::warn!(
            "ignored {variable} directive `{}`: {}",
            invalid.directive,
            invalid.reason
        );
    }
    Ok(())
}

/// Replaces the spec layered over the defaults and `RUST_LOG`; an empty spec removes it.
/// Accepts the same directives as `CINNABAR_LOG` and changes nothing when one is invalid.
pub fn set_filter(spec: &str) -> Result<(), FilterError> {
    let logging = LOGGING.get().ok_or(FilterError::NotInstalled)?;
    let directives = category::expand(spec).map_err(FilterError::Invalid)?;
    let filter = build_filter(&logging.base, &logging.rust_log, &directives);
    let mut current = logging.spec();
    logging
        .handle
        .reload(filter)
        .map_err(|error| FilterError::Reload(error.to_string()))?;
    *current = Spec {
        text: spec.trim().to_owned(),
        directives,
    };
    FILTER_GENERATION.fetch_add(1, Ordering::Release);
    Ok(())
}

/// The active filter, or `None` before [`install`].
pub fn filter() -> Option<FilterStatus> {
    let logging = LOGGING.get()?;
    let spec = logging.spec();
    let effective = logging.handle.with_current(ToString::to_string).ok()?;
    Some(FilterStatus {
        spec: spec.text.clone(),
        effective,
    })
}

/// Counts changes to the active filter: 0 before [`install`], then one more for the install
/// and for each [`set_filter`] that succeeds. Reading it is one atomic load, so a per-frame
/// caller can re-read [`filter`] and [`category_filter`] only when it changes.
pub fn filter_generation() -> u64 {
    FILTER_GENERATION.load(Ordering::Acquire)
}

/// The most verbose level the active filter enables for any of `category`'s targets outside
/// spans, or [`LevelFilter::OFF`] before [`install`].
pub fn category_filter(category: Category) -> LevelFilter {
    let Some(logging) = LOGGING.get() else {
        return LevelFilter::OFF;
    };
    let spec = logging.spec();
    let directives = category::split(&logging.base)
        .chain(logging.rust_log.iter().map(String::as_str))
        .chain(spec.directives.iter().map(String::as_str));
    category::category_level(category, directives)
}

/// The spans entered on this thread, innermost first, with their fields and locations; `None`
/// outside any span or before [`install`].
pub fn span_trace() -> Option<String> {
    let trace = tracing_error::SpanTrace::capture();
    (trace.status() == tracing_error::SpanTraceStatus::CAPTURED).then(|| trace.to_string())
}

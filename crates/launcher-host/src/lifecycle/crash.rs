//! Local panic and native crash reports written as crash folders; nothing is uploaded.

#[cfg(windows)]
mod native;
#[cfg(any(windows, test))]
mod native_address;
#[cfg(unix)]
pub(crate) mod unix;

use std::{
    backtrace::Backtrace,
    path::PathBuf,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use crash_report::{
    folder,
    report::{Identity, Kind, Report, Thread},
    section,
};
use launcher::{graphics_backend::Adapter, install_layout::InstallLayout};

use crate::renderer_startup::Attempt;

/// Where this process writes crash folders and which logs and identity they record.
struct Context {
    crash_dir: PathBuf,
    client_log: PathBuf,
    core_log: PathBuf,
    data_root: PathBuf,
    identity: Identity,
}

static CONTEXT: OnceLock<Context> = OnceLock::new();

/// Writes a crash folder for every panic and native fault of this process.
///
/// Each panic captures one backtrace, hands `log_panic` the message and that backtrace, and
/// flushes the caller's logs before copying this session's log sections; the previous hook
/// then runs as before. `identity` names the process in every report; a second call keeps the
/// first identity.
pub fn install_panic_hook(
    layout: &InstallLayout,
    identity: Identity,
    log_panic: fn(&str),
    flush_logs: fn(),
) {
    let context = configure(layout, identity);
    #[cfg(windows)]
    native::install(context, flush_logs);
    #[cfg(unix)]
    unix::install(context);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info.to_string();
        let backtrace = Backtrace::force_capture().to_string();
        log_panic(&format!("{message}\n{backtrace}\n"));
        // Queued lines must reach `client.log` before its section is copied.
        flush_logs();
        let _ = write_panic(context, message, Thread::current(), backtrace, now_ms());
        previous(info);
        flush_logs();
    }));
}

/// Drops legacy report files and the oldest crash folders beyond the bound.
pub fn prune_reports(layout: &InstallLayout) {
    folder::prune(&layout.crash_dir(), folder::MAX_REPORTS);
}

fn configure(layout: &InstallLayout, identity: Identity) -> &'static Context {
    CONTEXT.get_or_init(|| context(layout, identity))
}

fn context(layout: &InstallLayout, identity: Identity) -> Context {
    Context {
        crash_dir: layout.crash_dir(),
        client_log: layout.log_dir().join("client.log"),
        core_log: layout.log_dir().join("core.log"),
        data_root: layout.user_data_root.clone(),
        identity,
    }
}

/// Starts a report for this process, attributing it to `attempt` and its GPU.
fn report(
    context: &Context,
    kind: Kind,
    message: String,
    attempt: Option<&Attempt>,
    now_ms: u64,
) -> Report {
    let mut report = Report::new(kind, message, &context.identity, now_ms);
    if let Some(adapter) = attempt.and_then(|attempt| attempt.adapter.as_ref()) {
        report.gpu = Some(adapter.name.clone());
        report.driver = driver(adapter);
    }
    report.renderer = attempt.and_then(|attempt| serde_json::to_value(attempt).ok());
    report
}

/// Joins the adapter's non-empty driver fields.
fn driver(adapter: &Adapter) -> Option<String> {
    let parts: Vec<&str> = [
        &adapter.driver,
        &adapter.driver_info,
        &adapter.driver_version,
    ]
    .into_iter()
    .map(String::as_str)
    .filter(|part| !part.is_empty())
    .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Writes `report` with this session's client and core log sections beside it.
fn write(context: &Context, report: &Report, now_ms: u64) -> std::io::Result<PathBuf> {
    let logs = folder::Logs {
        client: section::read(
            &context.client_log,
            section::CLIENT_SESSION_START,
            section::MAX_SECTION_BYTES,
        )
        .unwrap_or_default(),
        core: section::read_since(
            &context.core_log,
            section::CORE_SESSION_START,
            section::MAX_SECTION_BYTES,
            context.identity.started_at_ms,
        )
        .unwrap_or_default(),
    };
    folder::write(&context.crash_dir, report, &logs, now_ms)
}

fn write_panic(
    context: &Context,
    message: String,
    thread: Thread,
    backtrace: String,
    now_ms: u64,
) -> std::io::Result<PathBuf> {
    let attempt = renderer_context(&context.data_root);
    let mut report = report(context, Kind::Panic, message, attempt.as_ref(), now_ms);
    report.thread = thread;
    report.backtrace = backtrace;
    write(context, &report, now_ms)
}

/// Attributes a crash only to a renderer used by this process, never another client's saved choice.
fn renderer_context(data_root: &std::path::Path) -> Option<Attempt> {
    crate::renderer_startup::context::current(data_root)
}

/// Unix milliseconds now; crash handlers are the edge that reads the clock.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests;

//! Crash capture for the client; nothing is uploaded.
//!
//! The client starts an out-of-process monitor (`<client> --crash-monitor`) first. Faults and
//! panics only ask it for a minidump; it writes the crash folder once this process is gone and
//! tells the player. Until it is connected, or without one, the process writes the folder
//! itself.

mod fallback;
mod handler;
pub mod monitor;
pub mod protocol;
pub mod state;

use std::{
    backtrace::Backtrace,
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crash_report::{
    folder::{self, LogSources},
    report::{Identity, Kind, Report, Thread},
};
use launcher::{graphics_backend::Adapter, install_layout::InstallLayout};

use crate::renderer_startup::Attempt;

/// Runs the executable as the crash monitor: `<client> --crash-monitor <ipc name> <parent pid>
/// <crash dir>`.
pub const MONITOR_FLAG: &str = "--crash-monitor";
/// `1` makes dumps hold all process memory, for developers; Windows only.
pub const FULL_DUMP_ENV: &str = "CINNABAR_FULL_DUMP";
/// How long a failing `main` waits for the monitor to show its error.
const STARTUP_FAILURE_WAIT: Duration = Duration::from_secs(5);

/// The caller's logging, which crash capture flushes so the crash's log section is complete.
#[derive(Clone, Copy)]
pub struct LogHooks {
    /// Records the panic message and its backtrace in the client log.
    pub log_panic: fn(&str),
    /// Writes queued log lines.
    pub flush: fn(),
    /// Async-signal-safe: moves raw stderr still in its capture pipe into the client log.
    pub drain_on_signal: fn(),
}

/// Where this process writes crash folders and which logs and identity they record.
struct Context {
    crash_dir: PathBuf,
    client_log: PathBuf,
    core_log: PathBuf,
    data_root: PathBuf,
    identity: Identity,
    full_dump: bool,
}

impl Context {
    fn log_sources(&self) -> LogSources<'_> {
        LogSources {
            client: &self.client_log,
            core: &self.core_log,
            started_at_ms: self.identity.started_at_ms,
        }
    }
}

static CONTEXT: OnceLock<Context> = OnceLock::new();
/// One report per process: the first panic or fault claims it.
static REPORTED: AtomicBool = AtomicBool::new(false);

/// Captures every panic and native fault of this process.
///
/// Starts the monitor from `monitor` when given, attaches the native crash handler and wraps the
/// panic hook. Each panic captures one backtrace, hands `hooks.log_panic` the message and that
/// backtrace and flushes; the first panic or fault then asks the monitor for a dump, or writes
/// its folder in-process without one. `identity` names the process in every report. Later calls
/// change nothing.
pub fn install(
    layout: &InstallLayout,
    identity: Identity,
    hooks: LogHooks,
    monitor: Option<&monitor::MonitorCommand>,
) {
    let mut installed = false;
    let context = CONTEXT.get_or_init(|| {
        installed = true;
        context(layout, identity)
    });
    if !installed {
        return;
    }
    handler::install(context, hooks);
    #[cfg(unix)]
    {
        fallback::prepare(context);
        announce_exit_on_return();
    }
    if let Some(command) = monitor {
        monitor::start(command, &context.crash_dir, hello(context));
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info.to_string();
        let backtrace = Backtrace::force_capture().to_string();
        (hooks.log_panic)(&format!("{message}\n{backtrace}\n"));
        // Queued lines must reach `client.log` before its section is copied.
        (hooks.flush)();
        if !REPORTED.swap(true, Ordering::AcqRel) {
            let thread = Thread::current();
            let now_ms = now_ms();
            if !report_panic(context, &message, &thread, &backtrace, now_ms) {
                let _ = fallback::write_panic(context, message, thread, backtrace, now_ms);
            }
        }
        previous(info);
        (hooks.flush)();
    }));
}

/// Hands a panic to the monitor and dumps the panicking thread; false without a monitor.
fn report_panic(
    context: &Context,
    message: &str,
    thread: &Thread,
    backtrace: &str,
    now_ms: u64,
) -> bool {
    let panic = protocol::Panic {
        message: message.to_owned(),
        thread: thread.clone(),
        backtrace: backtrace.to_owned(),
        uptime_ms: now_ms.saturating_sub(context.identity.started_at_ms),
    };
    let sent = serde_json::to_vec(&panic).is_ok_and(|panic| monitor::send(protocol::PANIC, &panic));
    if sent {
        handler::dump_this_thread();
    }
    sent
}

/// Tells the monitor this process is about to exit with `code` on purpose, so it reports
/// nothing.
pub fn announce_exit(code: i32) {
    monitor::send(protocol::EXIT, &code.to_le_bytes());
}

/// Hands the error `main` failed with to the monitor, which shows it once this process exits;
/// false when no monitor is connected, so the caller must show it itself.
pub fn report_startup_failure(error: &str) -> bool {
    monitor::wait_for_connection(STARTUP_FAILURE_WAIT)
        && monitor::send(protocol::STARTUP_FAILURE, error.as_bytes())
}

/// Any other `exit()` or return from `main` is deliberate too; only signals and aborts bypass
/// this.
#[cfg(unix)]
fn announce_exit_on_return() {
    extern "C" fn announce() {
        announce_exit(i32::MIN);
    }
    // SAFETY: `announce` has the C ABI and only sends on an established socket.
    unsafe {
        libc::atexit(announce);
    }
}

/// Drops legacy report files and the oldest crash folders beyond the bound.
pub fn prune_reports(layout: &InstallLayout) {
    folder::prune(&layout.crash_dir(), folder::MAX_REPORTS);
}

fn context(layout: &InstallLayout, identity: Identity) -> Context {
    Context {
        crash_dir: layout.crash_dir(),
        client_log: layout.log_dir().join("client.log"),
        core_log: layout.log_dir().join("core.log"),
        data_root: layout.user_data_root.clone(),
        identity,
        full_dump: std::env::var_os(FULL_DUMP_ENV).is_some_and(|value| value == "1"),
    }
}

/// Introduces this client to its monitor.
fn hello(context: &Context) -> protocol::Hello {
    protocol::Hello {
        identity: context.identity.clone(),
        client_log: context.client_log.clone(),
        core_log: context.core_log.clone(),
        restart: protocol::Restart {
            program: std::env::current_exe().unwrap_or_default(),
            args: std::env::args_os().skip(1).collect(),
            env_remove: vec![update_installer::startup::STARTUP_ENV.to_owned()],
        },
        full_dump: context.full_dump,
    }
}

/// Pushes the current state to the monitor, or prepares the in-process report without one.
fn state_changed() {
    let Some(context) = CONTEXT.get() else {
        return;
    };
    if monitor::connection().is_some() {
        if let Ok(state) = serde_json::to_vec(&snapshot(context)) {
            monitor::send(protocol::STATE, &state);
        }
    } else {
        #[cfg(unix)]
        fallback::prepare(context);
    }
}

/// This process's renderer attempt and game state, as reports record them.
fn snapshot(context: &Context) -> protocol::State {
    state_of(state::renderer(&context.data_root), state::game())
}

/// [`snapshot`] without waiting on another thread.
#[cfg(windows)]
fn try_snapshot(context: &Context) -> Option<protocol::State> {
    state::try_snapshot(&context.data_root).map(|(attempt, game)| state_of(attempt, game))
}

fn state_of(attempt: Option<Attempt>, game: state::GameState) -> protocol::State {
    let adapter = attempt
        .as_ref()
        .and_then(|attempt| attempt.adapter.as_ref());
    protocol::State {
        gpu: adapter.map(|adapter| adapter.name.clone()),
        driver: adapter.and_then(driver),
        renderer: attempt.and_then(|attempt| serde_json::to_value(attempt).ok()),
        game_state: game.entries(),
    }
}

/// Starts a report for this process with `state`.
fn report(
    context: &Context,
    kind: Kind,
    message: String,
    state: &protocol::State,
    now_ms: u64,
) -> Report {
    let mut report = Report::new(kind, message, &context.identity, now_ms);
    state.apply_to(&mut report);
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

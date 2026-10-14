//! The `--crash-monitor` process: serve the client's dump requests until it is gone, then report
//! how it ended.

use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use launcher::install_layout::InstallLayout;
use launcher_host::lifecycle::crash::protocol;

use crate::{
    actions, describe,
    outcome::{self, Ending},
    parent::Parent,
    server::{Handler, Observed},
    window::{self, Choice, CrashSummary},
};

/// `0` writes reports without showing the crash window, for tests and headless runs.
pub const CRASH_UI_ENV: &str = "CINNABAR_CRASH_UI";
/// How long the server may take to read what the client sent before it went away.
const DRAIN: Duration = Duration::from_secs(2);
/// The monitor's session files: `logs/crash-monitor.log`, earlier sessions archived beside it.
const LOG_STEM: &str = "crash-monitor";
/// The first line of each monitor session file.
const SESSION_START: &str = "CRASH_MONITOR_SESSION_START";

/// The monitor's command line after the flag: `<ipc name> <parent pid> <crash dir>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Args {
    pub ipc_name: String,
    pub parent: u32,
    pub crash_dir: PathBuf,
}

impl Args {
    /// Parses the three arguments; extra leading arguments, such as a test harness's, are
    /// ignored.
    pub fn parse(args: &[OsString]) -> Option<Self> {
        let [name, parent, crash_dir] = args.get(args.len().checked_sub(3)?..)? else {
            return None;
        };
        Some(Self {
            ipc_name: name.to_str()?.to_owned(),
            parent: parent.to_str()?.parse().ok()?,
            crash_dir: PathBuf::from(crash_dir),
        })
    }
}

/// Runs the monitor for `args`, logging to and reading preferences and the UI font from
/// `layout`; returns the process exit code.
pub fn run(args: &Args, layout: Option<&InstallLayout>) -> i32 {
    if let Some(layout) = layout {
        start_log(layout, args);
    }
    let parent = Parent::open(args.parent);
    let mut server = match minidumper::Server::with_name(protocol::socket_name(&args.ipc_name)) {
        Ok(server) => server,
        Err(error) => {
            tracing::error!("crash monitor could not listen: {error}");
            return 1;
        }
    };
    let observed = Arc::new(Mutex::new(Observed::default()));
    let handler = Handler::new(Arc::clone(&observed), args.crash_dir.clone(), now_ms);
    let shutdown = Arc::new(AtomicBool::new(false));
    let (finished, server_finished) = mpsc::channel();
    let serving = {
        let shutdown = Arc::clone(&shutdown);
        std::thread::Builder::new()
            .name("crash-server".into())
            .spawn(move || {
                if let Err(error) = server.run(Box::new(handler), &shutdown, None) {
                    tracing::error!("crash monitor server stopped: {error}");
                }
                let _ = finished.send(());
            })
    };
    let Ok(serving) = serving else {
        return 1;
    };
    let mut stdout = std::io::stdout();
    if writeln!(stdout, "{}", protocol::READY)
        .and_then(|()| stdout.flush())
        .is_err()
    {
        tracing::error!("crash monitor could not signal readiness");
    }
    // Only the client holds our stdin open: its end means the client is gone.
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
    let exit_code = parent.exit_code();
    if server_finished.recv_timeout(DRAIN).is_err() {
        shutdown.store(true, Ordering::Relaxed);
    }
    let _ = serving.join();
    let observed = observed
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let ending = outcome::ending(&observed, exit_code);
    tracing::info!("client ended: {ending:?} (exit code {exit_code:?})");
    report(args, layout, &observed, &ending);
    0
}

/// Starts this session's `crash-monitor.log`, archiving the previous one.
fn start_log(layout: &InstallLayout, args: &Args) {
    let dir = layout.log_dir();
    let header = format!(
        "{SESSION_START} session={} parent={} timestamp_ms={}",
        diagnostics::session::id(),
        args.parent,
        diagnostics::session::started_at_ms()
    );
    let files = diagnostics::console::SessionFiles {
        dir: &dir,
        stem: LOG_STEM,
        header: &header,
        json: diagnostics::logging::json_enabled(),
        retention: diagnostics::session_log::Retention::DEFAULT,
    };
    if let Err(error) = diagnostics::console::initialize_log(&files) {
        tracing::warn!("crash monitor file logging unavailable: {error}");
    }
}

/// Writes the report for `ending`, then shows it unless [`CRASH_UI_ENV`] is `0`.
fn report(args: &Args, layout: Option<&InstallLayout>, observed: &Observed, ending: &Ending) {
    let now_ms = now_ms();
    let Some(report) = outcome::report(observed, ending, now_ms) else {
        return;
    };
    let (folder, logs) = match outcome::write(&args.crash_dir, observed, &report, now_ms) {
        Ok(written) => written,
        Err(error) => {
            tracing::error!("crash report could not be written: {error}");
            return;
        }
    };
    tracing::info!("crash report saved to {}", folder.display());
    if std::env::var_os(CRASH_UI_ENV).is_some_and(|value| value == "0") {
        return;
    }
    show(layout, observed, describe::summary(&report, &logs, folder));
}

/// Shows the crash window, or the native dialog when it cannot open, and restarts the client
/// when the player asks.
fn show(layout: Option<&InstallLayout>, observed: &Observed, summary: CrashSummary) {
    let choice = match layout {
        Some(layout) => window::show(summary.clone(), layout),
        None => Choice::Unavailable,
    };
    let restart = match choice {
        Choice::Restart => true,
        Choice::Close => false,
        Choice::Unavailable => actions::native_dialog(&summary),
    };
    if let (true, Some(hello)) = (restart, &observed.hello)
        && let Err(error) = actions::restart(&hello.restart)
    {
        tracing::error!("restart failed: {error}");
    }
}

/// Unix milliseconds now; the monitor is the edge that reads the clock.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_the_last_three() {
        let args: Vec<OsString> = ["--exact", "monitor", "name", "42", "crashes"]
            .into_iter()
            .map(OsString::from)
            .collect();
        assert_eq!(
            Args::parse(&args),
            Some(Args {
                ipc_name: "name".into(),
                parent: 42,
                crash_dir: PathBuf::from("crashes"),
            })
        );
        assert_eq!(Args::parse(&args[3..]), None);
        let bad_pid: Vec<OsString> = ["name", "pid", "crashes"]
            .into_iter()
            .map(OsString::from)
            .collect();
        assert_eq!(Args::parse(&bad_pid), None);
    }
}

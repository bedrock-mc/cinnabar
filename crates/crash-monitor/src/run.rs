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

use crash_report::{folder::Logs, report::Report};
use launcher::install_layout::InstallLayout;
use launcher_host::lifecycle::crash::{
    heartbeat::{Heartbeat, Region},
    protocol::{self, HangWatch, Launch, Restart},
};

use crate::{
    actions, describe,
    hang::{self, Stall, Watchdog},
    outcome::{self, Ending},
    panic_history,
    parent::Parent,
    server::{Event, Handler, Liveness, Observed},
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

/// The monitor's command line after the flag: `<socket> <parent pid> <crash dir>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Args {
    pub socket: PathBuf,
    pub parent: u32,
    pub crash_dir: PathBuf,
}

impl Args {
    /// Parses the three arguments; extra leading arguments, such as a test harness's, are
    /// ignored.
    pub fn parse(args: &[OsString]) -> Option<Self> {
        let [socket, parent, crash_dir] = args.get(args.len().checked_sub(3)?..)? else {
            return None;
        };
        Some(Self {
            socket: PathBuf::from(socket),
            parent: parent.to_str()?.parse().ok()?,
            crash_dir: PathBuf::from(crash_dir),
        })
    }
}

/// A report folder written this run, with what the window shows of it.
struct Written {
    report: Report,
    logs: Logs,
    folder: PathBuf,
}

/// Runs the monitor for `args`, logging to and reading preferences and the UI font from
/// `layout`. Returns the report folder the crash window showed, or would have shown with
/// [`CRASH_UI_ENV`] `0`; `None` when the client ended without one to show.
pub fn run(args: &Args, layout: Option<&InstallLayout>) -> Result<Option<PathBuf>, String> {
    if let Some(layout) = layout {
        start_log(layout, args);
    }
    let launch = read_launch()?;
    // The healthy monitor owns all desktop network work, including retries from earlier runs.
    let previous_uploads = layout
        .cloned()
        .and_then(launcher_host::lifecycle::crash::upload::spawn_send_due);
    // Retain before READY: any introduced client has a monitor protecting its attachments.
    let staging = launch
        .staging
        .as_deref()
        .map(crash_report::staging::Staging::retain)
        .transpose()
        .map_err(|error| format!("crash staging could not be retained: {error}"))?;
    let parent = Parent::open(args.parent);
    let mut server = minidumper::Server::with_name(protocol::socket_name(&args.socket))
        .map_err(|error| format!("crash monitor could not listen: {error}"))?;
    let observed = Arc::new(Mutex::new(Observed::default()));
    let liveness = Arc::new(Liveness::default());
    let (events, received) = mpsc::sync_channel(1);
    let handler = Handler::new(
        Arc::clone(&observed),
        args.crash_dir.clone(),
        now_ms,
        launch.token,
        events.clone(),
        Arc::clone(&liveness),
    );
    #[cfg(target_os = "macos")]
    let handler = handler.with_dump_receipts(launch.hang.as_ref().and_then(|hang| {
        Heartbeat::open(&hang.heartbeat)
            .map_err(|error| tracing::warn!("dump receipts unavailable: {error}"))
            .ok()
    }));
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
            .map_err(|error| error.to_string())?
    };
    let mut stdout = std::io::stdout();
    if writeln!(stdout, "{}", protocol::READY)
        .and_then(|()| stdout.flush())
        .is_err()
    {
        tracing::error!("crash monitor could not signal readiness");
    }
    // Only the client holds our stdin open: its end means the client is gone.
    std::thread::Builder::new()
        .name("crash-lifeline".into())
        .spawn(move || {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            let _ = events.send(Event::ClientGone);
        })
        .map_err(|error| error.to_string())?;
    let watchdog = launch
        .hang
        .as_ref()
        .and_then(|hang| watch_hangs(args, hang, &observed, layout));
    let mut panicked = false;
    for event in &received {
        match event {
            Event::PanicDumped => write_panics(args, layout, &observed, &mut panicked),
            Event::ClientGone => break,
        }
    }
    let hung = watchdog.and_then(Watchdog::stop);
    let exit_code = parent.exit_code();
    if let Some(hang) = &launch.hang
        && let Err(error) = std::fs::remove_file(&hang.heartbeat)
    {
        tracing::warn!("heartbeat file not removed: {error}");
    }
    if liveness.client_gone() || server_finished.recv_timeout(DRAIN).is_err() {
        shutdown.store(true, Ordering::Relaxed);
    }
    let _ = serving.join();
    write_panics(args, layout, &observed, &mut panicked);
    let observed = observed
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let ending = outcome::ending(&observed, panicked, exit_code);
    tracing::info!("client ended: {ending:?} (exit code {exit_code:?})");
    let shown = match ending {
        Ending::Panicked => observed
            .hello
            .as_ref()
            .and_then(|hello| saved_panic(&args.crash_dir, &hello.identity.session_id)),
        // Killed while not responding: the hang's report already tells what happened.
        Ending::Abort(_) if hung.is_some() => hung,
        _ => report(args, &observed, &ending),
    };
    drop(staging);
    if let (Some(layout), Some(written)) = (layout, &shown) {
        launcher_host::lifecycle::crash::upload::completed(layout, &written.folder);
    }
    let current_uploads = layout
        .cloned()
        .and_then(launcher_host::lifecycle::crash::upload::spawn_send_due);
    // Network retries must not delay opening the crash window or launching a requested restart.
    let shown = shown.map(|written| {
        show(layout, &launch.restart, &written);
        written.folder
    });
    for worker in [previous_uploads, current_uploads].into_iter().flatten() {
        let _ = worker.join();
    }
    Ok(shown)
}

/// Reads the client's [`Launch`] line from stdin.
fn read_launch() -> Result<Launch, String> {
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|error| format!("crash monitor could not read its launch: {error}"))?;
    serde_json::from_str(&line).map_err(|error| format!("unreadable crash monitor launch: {error}"))
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

/// Writes the folders of panics dumped since the last call, while the client may live on.
fn write_panics(
    args: &Args,
    layout: Option<&InstallLayout>,
    observed: &Mutex<Observed>,
    panicked: &mut bool,
) {
    let (hello, dumped) = {
        let mut observed = observed.lock().unwrap_or_else(PoisonError::into_inner);
        (
            observed.hello.clone(),
            std::mem::take(&mut observed.dumped_panics),
        )
    };
    let Some(hello) = hello else {
        return;
    };
    for dumped in &dumped {
        let now_ms = now_ms();
        let report = outcome::dumped_panic_report(&hello, dumped, now_ms);
        let dump = dumped.dump.path.as_deref();
        match outcome::write(&args.crash_dir, &hello, &report, dump, now_ms) {
            Ok((folder, _)) => {
                tracing::info!("panic report saved to {}", folder.display());
                if let Some(layout) = layout {
                    launcher_host::lifecycle::crash::upload::completed(layout, &folder);
                }
                *panicked = true;
            }
            Err(error) => tracing::error!("panic report could not be written: {error}"),
        }
    }
}

/// Loads the selected panic and its saved logs after report writers have stopped.
fn saved_panic(crash_dir: &std::path::Path, session_id: &str) -> Option<Written> {
    let (folder, report) = panic_history::select(crash_dir, session_id)?;
    let logs = Logs {
        client: saved_log(&folder, crash_report::folder::CLIENT_LOG_FILE),
        core: saved_log(&folder, crash_report::folder::CORE_LOG_FILE),
    };
    Some(Written {
        report,
        logs,
        folder,
    })
}

/// Reads one saved log with the same byte limit as crash-time collection.
fn saved_log(folder: &std::path::Path, member: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Ok(file) = std::fs::File::open(folder.join(member)) {
        let _ = file
            .take(crash_report::section::MAX_SECTION_BYTES)
            .read_to_end(&mut bytes);
    }
    bytes
}

/// Watches the client's heartbeat for hangs, each written by [`write_hang`]; `None` when the
/// heartbeat cannot be read, so hangs go unreported.
fn watch_hangs(
    args: &Args,
    hang: &HangWatch,
    observed: &Arc<Mutex<Observed>>,
    layout: Option<&InstallLayout>,
) -> Option<Watchdog<Written>> {
    let heartbeat = Heartbeat::open(&hang.heartbeat)
        .map_err(|error| tracing::warn!("hang detection unavailable: {error}"))
        .ok()?;
    let args = args.clone();
    let observed = Arc::clone(observed);
    let layout = layout.cloned();
    hang::spawn(
        heartbeat,
        hang.stall_after_ms,
        now_ms,
        move |region, stall| {
            let written = write_hang(&args, &observed, region, stall)?;
            if let Some(layout) = &layout {
                launcher_host::lifecycle::crash::upload::completed(layout, &written.folder);
            }
            Some(written)
        },
    )
    .map_err(|error| tracing::warn!("hang watchdog did not start: {error}"))
    .ok()
}

/// Writes the report of `stall` while the client keeps running, with a dump of all its threads
/// where the platform allows one. Nothing is written for a client that never introduced itself,
/// one being dumped for a crash, or one paused in a debugger.
fn write_hang(
    args: &Args,
    observed: &Mutex<Observed>,
    region: &Region,
    stall: Stall,
) -> Option<Written> {
    let (hello, state, crashing) = {
        let observed = observed.lock().unwrap_or_else(PoisonError::into_inner);
        (
            observed.hello.clone(),
            observed.state.clone(),
            observed.dump_requested || observed.panic.is_some() || observed.dump.is_some(),
        )
    };
    let Some(hello) = hello else {
        tracing::warn!("no hang report: the client never introduced itself");
        return None;
    };
    if crashing {
        tracing::info!("no hang report: the client is being dumped for a crash");
        return None;
    }
    if hang::debugger_attached(args.parent) {
        tracing::info!("no hang report: a debugger is attached to the client");
        return None;
    }
    let thread = region.thread();
    let pending = args
        .crash_dir
        .join(format!(".pending-hang-{}.dmp", std::process::id()));
    let missing_dump = std::fs::create_dir_all(&args.crash_dir)
        .map_err(|error| error.to_string())
        .and_then(|()| hang::dump_live(args.parent, thread, hello.full_dump, &pending))
        .err();
    if let Some(reason) = &missing_dump {
        tracing::warn!("hang minidump failed: {reason}");
        let _ = std::fs::remove_file(&pending);
    }
    let now_ms = now_ms();
    let report = hang::report(
        &hello.identity,
        &state,
        stall,
        thread,
        missing_dump.as_deref(),
        now_ms,
    );
    let dump = missing_dump.is_none().then_some(pending.as_path());
    match outcome::write(&args.crash_dir, &hello, &report, dump, now_ms) {
        Ok((folder, logs)) => {
            tracing::info!("hang report saved to {}", folder.display());
            Some(Written {
                report,
                logs,
                folder,
            })
        }
        Err(error) => {
            tracing::error!("hang report could not be written: {error}");
            let _ = std::fs::remove_file(&pending);
            None
        }
    }
}

/// Writes the report for `ending`, when it needs a new one.
fn report(args: &Args, observed: &Observed, ending: &Ending) -> Option<Written> {
    let now_ms = now_ms();
    let report = outcome::report(observed, ending, now_ms)?;
    let hello = observed.hello.as_ref()?;
    let dump = observed.dump.as_ref().and_then(|dump| dump.path.as_deref());
    match outcome::write(&args.crash_dir, hello, &report, dump, now_ms) {
        Ok((folder, logs)) => {
            tracing::info!("crash report saved to {}", folder.display());
            Some(Written {
                report,
                logs,
                folder,
            })
        }
        Err(error) => {
            tracing::error!("crash report could not be written: {error}");
            None
        }
    }
}

/// Shows `written` in the crash window, or the native dialog when it cannot open, unless
/// [`CRASH_UI_ENV`] is `0`, and restarts the client when the player asks.
fn show(layout: Option<&InstallLayout>, restart: &Restart, written: &Written) {
    if std::env::var_os(CRASH_UI_ENV).is_some_and(|value| value == "0") {
        return;
    }
    let summary: CrashSummary =
        describe::summary(&written.report, &written.logs, written.folder.clone());
    match layout {
        Some(layout) => {
            window::show(summary.clone(), layout, |choice| {
                apply_choice(restart, &summary, choice)
            });
        }
        None => apply_choice(restart, &summary, Choice::Unavailable),
    }
}

/// Starts a requested replacement process before the crash window waits for pending uploads.
fn apply_choice(restart: &Restart, summary: &CrashSummary, choice: Choice) {
    let restarting = match choice {
        Choice::Restart => true,
        Choice::Close => false,
        Choice::Unavailable => actions::native_dialog(summary),
    };
    if restarting && let Err(error) = actions::restart(restart) {
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
    fn panic_selection_survives_cascades_longer_than_folder_retention() {
        use crash_report::report::{Identity, Thread};
        use launcher_host::lifecycle::crash::protocol::{Hello, Panic, State};

        let directory = tempfile::tempdir().unwrap();
        let args = Args {
            socket: directory.path().join("socket"),
            parent: std::process::id(),
            crash_dir: directory.path().join("crashes"),
        };
        let observed = Mutex::new(Observed {
            hello: Some(Hello {
                identity: Identity {
                    session_id: "0123456789abcdef".into(),
                    started_at_ms: 0,
                    release: "test".into(),
                    commit: "test".into(),
                    build_profile: "test".into(),
                },
                client_log: directory.path().join("client.log"),
                core_log: directory.path().join("core.log"),
                full_dump: false,
                staging: None,
            }),
            ..Observed::default()
        });
        let mut panicked = false;
        for index in 0..crash_report::folder::MAX_REPORTS * 3 {
            let message = format!("panic {index}");
            std::fs::write(
                directory.path().join("client.log"),
                format!(
                    "CLIENT_SESSION_START session=0123456789abcdef timestamp_ms=0\n{message}\n"
                ),
            )
            .unwrap();
            let dump = directory.path().join("pending.dmp");
            std::fs::write(&dump, &message).unwrap();
            observed
                .lock()
                .unwrap()
                .dumped_panics
                .push(crate::server::DumpedPanic {
                    panic: Panic {
                        message,
                        thread: Thread::default(),
                        backtrace: String::new(),
                        uptime_ms: index as u64 * 900,
                    },
                    dump: crate::server::Dump {
                        path: Some(dump),
                        at_ms: 0,
                    },
                    state: State::default(),
                });
            write_panics(&args, None, &observed, &mut panicked);
        }
        assert!(panicked);
        let shown = saved_panic(&args.crash_dir, "0123456789abcdef").unwrap();
        assert_eq!(
            crash_report::folder::read(&shown.folder).unwrap(),
            shown.report
        );
        assert!(
            String::from_utf8(shown.logs.client)
                .unwrap()
                .contains(&shown.report.message)
        );
        assert_eq!(
            std::fs::read_to_string(shown.folder.join(crash_report::folder::DUMP_FILE)).unwrap(),
            shown.report.message
        );
        assert_eq!(shown.folder, crash_report::folder::list(&args.crash_dir)[0]);
        assert_eq!(
            crash_report::folder::list(&args.crash_dir).len(),
            crash_report::folder::MAX_REPORTS
        );
    }

    #[test]
    fn arguments_are_the_last_three() {
        let args: Vec<OsString> = ["--exact", "monitor", "socket", "42", "crashes"]
            .into_iter()
            .map(OsString::from)
            .collect();
        assert_eq!(
            Args::parse(&args),
            Some(Args {
                socket: PathBuf::from("socket"),
                parent: 42,
                crash_dir: PathBuf::from("crashes"),
            })
        );
        assert_eq!(Args::parse(&args[3..]), None);
        let bad_pid: Vec<OsString> = ["socket", "pid", "crashes"]
            .into_iter()
            .map(OsString::from)
            .collect();
        assert_eq!(Args::parse(&bad_pid), None);
    }
}

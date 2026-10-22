//! Diagnostics of every `bedrock-core` process: the log level and crash-output file each spawn
//! is given, its stderr routed into the session's `core.log`, and a report for an exit it was
//! not asked for.
//!
//! The client [`install`]s the routes once. Each spawn through [`spawn`] then gets
//! `-log-level` from the core log category and `-crash-output` naming a per-process file in
//! the [`STAGING_DIR`] of this session's crash staging directory, which startup prunes once its
//! client is gone. A reader thread per process labels its stderr lines and queues them
//! for one `core.log` writer, so a slow disk never blocks a core; a full queue drops lines and
//! counts them. Once stderr closes the thread collects the exit status: a crash writes a
//! `kind: core` report with the Go runtime's crash output attached, and the staged file is
//! deleted either way.

use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{ChildStderr, Command, ExitStatus},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, bounded};
use tracing::level_filters::LevelFilter;

use crate::lifecycle::children::{self, ExitObserver, Spawned};

/// Lines queued for the `core.log` writer before further lines are dropped.
const LOG_QUEUE_LINES: usize = 4096;
/// Longest stderr line kept whole; longer output continues on the next line.
const MAX_LINE_BYTES: usize = 16 << 10;
/// Most crash output attached to a report, from its start: the panic and the crashing stack.
const MAX_CRASH_OUTPUT_BYTES: u64 = crash_report::section::MAX_SECTION_BYTES;
/// How long an exiting core's thread waits for its lines to reach `core.log`.
const FLUSH_WAIT: Duration = Duration::from_secs(2);
/// How often an exited core's status is collected once its stderr closes.
const EXIT_POLL: Duration = Duration::from_millis(10);
/// Subdirectory of the session's crash staging directory holding each core's crash output.
pub const STAGING_DIR: &str = "core";

/// What a core process is for; names it in `core.log` and in its crash-output file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreRole {
    /// A game session's proxy core.
    Game,
    /// The launcher's account core.
    Account,
    /// The one-shot sign-in helper (`-auth-events`).
    SignIn,
    /// The one-shot account catalog helper (`-catalog-file`).
    Catalog,
}

impl CoreRole {
    /// The name bedrock-core gives itself for the same flags in its `process=` attribute.
    pub fn label(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Account => "account",
            Self::SignIn => "auth-events",
            Self::Catalog => "catalog",
        }
    }

    /// The sign-in helper reports on stdout and stops only when killed; every other core
    /// writes nothing to stdout and stops once its stdin closes.
    fn signs_in(self) -> bool {
        self == Self::SignIn
    }
}

/// bedrock-core's `-log-level` name for a log category's level. Go has no trace level, and a
/// category that is off still gets errors, so a core never fails silently.
pub fn go_log_level(level: LevelFilter) -> &'static str {
    match level.into_level() {
        Some(tracing::Level::TRACE | tracing::Level::DEBUG) => "debug",
        Some(tracing::Level::INFO) => "info",
        Some(tracing::Level::WARN) => "warn",
        Some(tracing::Level::ERROR) | None => "error",
    }
}

/// A core that ended in a crash rather than on request or through its own error path.
#[derive(Clone, Debug)]
pub struct CoreCrash {
    pub role: CoreRole,
    pub pid: u32,
    pub status: ExitStatus,
    /// The start of the Go runtime's crash output, the panic or fatal error with every
    /// goroutine's stack; empty when it wrote none.
    pub crash_output: Vec<u8>,
}

impl CoreCrash {
    /// One line for the report: the role, the exit status and the Go panic or fatal error.
    pub fn message(&self) -> String {
        let cause = String::from_utf8_lossy(&self.crash_output)
            .lines()
            .find(|line| line.starts_with("panic: ") || line.starts_with("fatal error: "))
            .map(str::to_owned);
        let summary = format!("{} core crashed ({})", self.role.label(), self.status);
        match cause {
            Some(cause) => format!("{summary}: {cause}"),
            None => summary,
        }
    }
}

/// Writes the crash report for a [`CoreCrash`], returning its folder when one was written.
pub type Reporter = Arc<dyn Fn(&CoreCrash) -> Option<PathBuf> + Send + Sync>;

/// Whether an exit with `code` (`None` for a signal) is a crash. Crash output from the Go
/// runtime always is; otherwise an exit this client asked for is not, and neither are success
/// and status 1, the core's own logged failure path (`core failed`).
pub fn crashed(code: Option<i32>, stop_requested: bool, wrote_crash_output: bool) -> bool {
    wrote_crash_output || (!stop_requested && !matches!(code, Some(0 | 1)))
}

/// What the client supplies to [`install`].
pub struct Config {
    /// The core log category's level, read at each spawn so filter changes reach later cores.
    pub level: fn() -> LevelFilter,
    /// The session's `core.log`; it receives every core's labelled stderr lines.
    pub log: Box<dyn Write + Send>,
}

static INSTALLED: OnceLock<CoreOutput> = OnceLock::new();

/// Routes every later core spawn's stderr into `config.log`, stages crash output in the crash
/// staging directory (none without one) and reports core crashes through crash capture, which
/// must be installed first. Fails when the log writer cannot start or routes are already
/// installed; spawns then run without them.
pub fn install(config: Config) -> io::Result<()> {
    let report: Reporter = Arc::new(super::crash::core_exit::report_core);
    let output = CoreOutput::new(
        config.level,
        Some(config.log),
        super::crash::staging_dir().map(|dir| dir.join(STAGING_DIR)),
        Some(report),
    )?;
    INSTALLED
        .set(output)
        .map_err(|_| io::Error::new(io::ErrorKind::AlreadyExists, "core output installed"))
}

/// Spawns a core-family process as a tracked child (see [`children::spawn`]) with `-log-level`,
/// and with the installed routes also `-crash-output`, its stderr in `core.log` and its exit
/// watched. Without them stderr is discarded and the level is the core's default, info. The
/// role decides stdin and stdout, so `command` carries only the program and arguments.
pub fn spawn(command: &mut Command, role: CoreRole) -> io::Result<Spawned> {
    match INSTALLED.get() {
        Some(output) => output.spawn(command, role),
        None => CoreOutput::uninstalled().spawn(command, role),
    }
}

/// The routes core spawns use.
#[derive(Clone)]
pub(crate) struct CoreOutput {
    level: fn() -> LevelFilter,
    log: Option<CoreLog>,
    staging: Option<PathBuf>,
    report: Option<Reporter>,
}

impl CoreOutput {
    /// Starts the log writer for `log`; crash output is staged only when it is routed, since
    /// only a watched core's staged file is collected.
    pub(crate) fn new(
        level: fn() -> LevelFilter,
        log: Option<Box<dyn Write + Send>>,
        staging: Option<PathBuf>,
        report: Option<Reporter>,
    ) -> io::Result<Self> {
        let log = log.map(CoreLog::start).transpose()?;
        Ok(Self {
            level,
            staging: staging.filter(|_| log.is_some()),
            log,
            report,
        })
    }

    fn uninstalled() -> Self {
        Self {
            level: || LevelFilter::INFO,
            log: None,
            staging: None,
            report: None,
        }
    }

    pub(crate) fn spawn(&self, command: &mut Command, role: CoreRole) -> io::Result<Spawned> {
        command.arg("-log-level").arg(go_log_level((self.level)()));
        let crash_output = self
            .staging
            .as_deref()
            .and_then(|dir| stage_crash_output(dir, role));
        if let Some(path) = &crash_output {
            command.arg("-crash-output").arg(path);
        }
        let child = start(command, role, self.log.is_some())?;
        let (Some(log), Some(stderr)) = (&self.log, child.take_stderr()) else {
            return Ok(child);
        };
        let watch = Watch {
            role,
            pid: child.id(),
            log: log.clone(),
            observer: child.observer(),
            crash_output,
            report: self.report.clone(),
        };
        let started = thread::Builder::new()
            .name(format!("core-{}-stderr", role.label()))
            .spawn(move || watch.run(stderr));
        if let Err(error) = started {
            // Nothing would drain the pipe, and a core blocked on stderr never exits.
            child.kill();
            child.wait();
            return Err(error);
        }
        Ok(child)
    }
}

/// Starts a core with stdin piped (null for the sign-in helper), stdout piped only for the
/// sign-in helper's events and stderr piped when `stderr` is routed. iOS cannot start processes,
/// so there the core runs in this one with pipes for the same streams.
fn start(command: &mut Command, role: CoreRole, stderr: bool) -> io::Result<Spawned> {
    let (stdin, stdout) = (!role.signs_in(), role.signs_in());
    #[cfg(target_os = "ios")]
    {
        children::spawn_embedded_core(command, stdin, stdout, stderr)
    }
    #[cfg(not(target_os = "ios"))]
    {
        use std::process::Stdio;
        let pipe = |piped: bool| if piped { Stdio::piped() } else { Stdio::null() };
        command
            .stdin(pipe(stdin))
            .stdout(pipe(stdout))
            .stderr(pipe(stderr));
        children::spawn(command)
    }
}

/// A fresh crash-output path for one process; `None`, with a warning, when the staging
/// directory cannot be created.
fn stage_crash_output(dir: &Path, role: CoreRole) -> Option<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if let Err(error) = fs::create_dir_all(dir) {
        tracing::warn!("core crash output unavailable: {error}");
        return None;
    }
    let path = dir.join(format!(
        "{}-{}.txt",
        NEXT.fetch_add(1, Ordering::Relaxed),
        role.label()
    ));
    Some(path)
}

/// One core's stderr reader and exit watcher.
struct Watch {
    role: CoreRole,
    pid: u32,
    log: CoreLog,
    observer: ExitObserver,
    crash_output: Option<PathBuf>,
    report: Option<Reporter>,
}

impl Watch {
    fn run(self, stderr: ChildStderr) {
        let label = format!("process={} pid={}", self.role.label(), self.pid);
        self.log
            .line(format!("{label} msg=\"core started\"\n").into_bytes());
        for_each_line(stderr, |line| self.log.line(labelled(&label, line)));
        let status = wait(&self.observer);
        let stop_requested = self.observer.stop_requested();
        let crash_output = self
            .crash_output
            .as_deref()
            .map(read_crash_output)
            .unwrap_or_default();
        let status_text = status.map_or_else(|| "unknown".to_owned(), |status| status.to_string());
        self.log.line(
            format!(
                "{label} msg=\"core exited\" status=\"{status_text}\" requested={stop_requested}\n"
            )
            .into_bytes(),
        );
        // The report's core.log section must hold this core's final lines.
        if !self.log.flush() {
            tracing::warn!("core.log did not catch up before the {label} exit was handled");
        }
        if let Some(status) = status {
            self.handle_exit(status, stop_requested, crash_output);
        }
        if let Some(path) = &self.crash_output
            && let Err(error) = fs::remove_file(path)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!("remove staged core crash output: {error}");
        }
    }

    fn handle_exit(&self, status: ExitStatus, stop_requested: bool, crash_output: Vec<u8>) {
        let role = self.role.label();
        if crashed(status.code(), stop_requested, !crash_output.is_empty()) {
            let crash = CoreCrash {
                role: self.role,
                pid: self.pid,
                status,
                crash_output,
            };
            match self.report.as_ref().and_then(|report| report(&crash)) {
                Some(folder) => tracing::warn!(
                    "{}; crash report written to {}",
                    crash.message(),
                    folder.display()
                ),
                None => tracing::warn!("{}; no crash report was written", crash.message()),
            }
        } else if !stop_requested && !status.success() {
            tracing::warn!(
                "{role} core {} failed ({status}); core.log has its error",
                self.pid
            );
        }
    }
}

/// `line` with its newline, prefixed by `label` unless bedrock-core already named its process.
fn labelled(label: &str, line: &[u8]) -> Vec<u8> {
    let named = line.starts_with(b"process=") || line.windows(9).any(|w| w == b" process=");
    let mut out = Vec::with_capacity(label.len() + line.len() + 2);
    if !named {
        out.extend_from_slice(label.as_bytes());
        out.push(b' ');
    }
    out.extend_from_slice(line);
    out.push(b'\n');
    out
}

/// Calls `line` with each line of `reader` without its line ending, splitting lines longer
/// than [`MAX_LINE_BYTES`].
fn for_each_line(reader: impl Read, mut line: impl FnMut(&[u8])) {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match (&mut reader)
            .take(MAX_LINE_BYTES as u64)
            .read_until(b'\n', &mut buffer)
        {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let end = buffer
                    .iter()
                    .rposition(|byte| !matches!(byte, b'\n' | b'\r'))
                    .map_or(0, |last| last + 1);
                line(&buffer[..end]);
            }
        }
    }
}

/// Polls until the child exits; `None` when its status cannot be read.
fn wait(observer: &ExitObserver) -> Option<ExitStatus> {
    loop {
        match observer.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => thread::sleep(EXIT_POLL),
            Err(_) => return None,
        }
    }
}

/// The start of the staged crash output; empty when the core wrote none.
fn read_crash_output(path: &Path) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Ok(file) = fs::File::open(path) {
        let _ = file.take(MAX_CRASH_OUTPUT_BYTES).read_to_end(&mut bytes);
    }
    bytes
}

/// The one writer of `core.log`: callers queue whole lines without blocking, and a full queue
/// drops a line and counts it; the writer logs the count before the next entry it takes.
#[derive(Clone)]
struct CoreLog {
    queue: Sender<Entry>,
    dropped: Arc<AtomicU64>,
}

enum Entry {
    Line(Vec<u8>),
    Flush(Sender<()>),
}

impl CoreLog {
    fn start(sink: Box<dyn Write + Send>) -> io::Result<Self> {
        Self::start_with(sink, LOG_QUEUE_LINES)
    }

    fn start_with(sink: Box<dyn Write + Send>, capacity: usize) -> io::Result<Self> {
        let (queue, entries) = bounded(capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&dropped);
        thread::Builder::new()
            .name("core-log".to_owned())
            .spawn(move || write_entries(sink, &entries, &counted))?;
        Ok(Self { queue, dropped })
    }

    fn line(&self, line: Vec<u8>) {
        if self.queue.try_send(Entry::Line(line)).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Waits, bounded by [`FLUSH_WAIT`] at each step, until every line queued before this call
    /// is written and the sink flushed; false when that did not happen in time.
    fn flush(&self) -> bool {
        let (ack, done) = bounded(1);
        self.queue
            .send_timeout(Entry::Flush(ack), FLUSH_WAIT)
            .is_ok()
            && done.recv_timeout(FLUSH_WAIT).is_ok()
    }
}

fn write_entries(mut sink: Box<dyn Write + Send>, entries: &Receiver<Entry>, dropped: &AtomicU64) {
    for entry in entries {
        let lost = dropped.swap(0, Ordering::Relaxed);
        if lost > 0 {
            let _ = writeln!(sink, "msg=\"core log lines dropped\" count={lost}");
        }
        match entry {
            Entry::Line(line) => {
                let _ = sink.write_all(&line);
            }
            Entry::Flush(ack) => {
                let _ = sink.flush();
                let _ = ack.send(());
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;

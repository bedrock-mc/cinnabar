//! Console output written in order by one background thread, so a slow or paused console (a
//! terminal selection, a full pipe) never stalls the frame thread that logs or emits a marker.
//!
//! Every write carries a [`Priority`]. Normal writes are dropped (and counted) once the shared
//! queue capacity is full; warnings and errors also use a reserve normal writes cannot reach,
//! and an error waits a bounded time when even the reserve is full. Once session files are
//! attached, every terminal write is mirrored to them, and raw writes to the process stderr
//! (`eprintln!`, the default panic hook) are captured into the same ordered stream.

mod queue;
mod raw_stderr;
#[cfg(unix)]
pub(crate) mod signal_drain;
mod synchronized;
mod terminal;
#[cfg(test)]
mod tests;

use crate::session_log::{Family, Retention, SessionLog};
use queue::{Queue, Rejected};
use std::{
    io::{self, Write},
    path::Path,
    sync::{
        Arc, LazyLock, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    time::{Duration, Instant},
};

static CONSOLE: LazyLock<Console> =
    LazyLock::new(|| Console::spawn(std::io::stdout(), std::io::stderr()));
/// Large owned snapshots have a separate bound inside the console queue.
const QUEUED_REPORTS: u64 = 8;
/// Longest a panic or forced exit waits for queued output.
const EXIT_FLUSH_TIMEOUT: Duration = Duration::from_millis(500);
/// Longest attaching session files waits for the writer to switch its stderr destination.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(2);
/// Most plain-text output kept for the session log from before it attaches.
const EARLY_BYTES: usize = 256 << 10;

/// Where a queued write goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
    /// Records a message in the file while another hook handles its terminal output.
    FileOnly,
    /// A structured record for the JSON Lines session file only.
    Json,
}

/// How hard the console tries to keep a write when its queue is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Dropped and counted once the shared capacity is full.
    Normal,
    /// May also use the reserved capacity; dropped and counted when that is full too.
    Warn,
    /// Like [`Priority::Warn`], then waits up to 50 ms for space before it is dropped.
    Error,
}

impl Priority {
    /// The priority of a log event at `level`.
    pub fn of(level: &tracing::Level) -> Self {
        match *level {
            tracing::Level::ERROR => Self::Error,
            tracing::Level::WARN => Self::Warn,
            _ => Self::Normal,
        }
    }
}

enum Message {
    Write {
        stream: Stream,
        priority: Priority,
        bytes: Vec<u8>,
    },
    /// A bounded report formatted only when the writer reaches it.
    Report(Stream, DeferredReport),
    /// Terminal output whose file copy has already been synchronized.
    WriteConsole(Stream, Vec<u8>),
    /// Replaces the stderr destination, as when fd 2 becomes the capture pipe.
    Redirect(Box<dyn Write + Send>),
    /// Acknowledged once every earlier write has reached its stream.
    Flush(SyncSender<()>),
}

impl Message {
    /// Heap byte capacity retained by an ordinary queued write.
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Write { bytes, .. } | Self::WriteConsole(_, bytes) => bytes.capacity(),
            Self::Report(..) | Self::Redirect(_) | Self::Flush(_) => 0,
        }
    }

    /// The priority the queue admits this message at.
    fn priority(&self) -> Priority {
        match self {
            Self::Write { priority, .. } => *priority,
            Self::Report(..) | Self::WriteConsole(..) | Self::Redirect(_) | Self::Flush(_) => {
                Priority::Normal
            }
        }
    }
}

/// The session files the writer mirrors output into.
struct Files {
    log: Option<SessionLog>,
    json: Option<SessionLog>,
    plain: AnsiFilter,
    /// Plain-text output from before the session log attached, written after its header;
    /// `None` once a log has attached.
    early: Option<Vec<u8>>,
    /// Bytes [`Files::early`] had no room for.
    early_lost: u64,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            log: None,
            json: None,
            plain: AnsiFilter::default(),
            early: Some(Vec::new()),
            early_lost: 0,
        }
    }
}

impl Files {
    /// Writes `bytes` to the file for `stream`; the plain-text file drops terminal colours.
    fn record(&mut self, stream: Stream, bytes: &[u8]) {
        match stream {
            Stream::Json => {
                if let Some(json) = self.json.as_mut() {
                    let _ = json.write_all(bytes).and_then(|()| json.flush());
                }
            }
            Stream::Stdout | Stream::Stderr | Stream::FileOnly => {
                let text = self.plain.strip(bytes);
                self.write_plain(&text);
            }
        }
    }

    /// Writes plain text to the session log, or keeps what fits for the log until it attaches.
    fn write_plain(&mut self, text: &[u8]) {
        if let Some(log) = self.log.as_mut() {
            let _ = log.write_all(text).and_then(|()| log.flush());
        } else if let Some(early) = self.early.as_mut() {
            let kept = text.len().min(EARLY_BYTES.saturating_sub(early.len()));
            early.extend_from_slice(&text[..kept]);
            self.early_lost += (text.len() - kept) as u64;
        }
    }

    /// Starts mirroring into `log`, first copying the output kept from before it attached; a
    /// failed copy loses only that early output, never the log itself.
    fn attach_log(&mut self, mut log: SessionLog) {
        if let Some(early) = self.early.take() {
            let _ = log.write_all(&early);
            if self.early_lost > 0 {
                let _ = writeln!(
                    log,
                    "console: {} bytes written before the log opened were not kept",
                    self.early_lost
                );
            }
            let _ = log.flush();
        }
        self.log = Some(log);
    }
}

fn lock(files: &Mutex<Files>) -> MutexGuard<'_, Files> {
    files.lock().unwrap_or_else(|error| error.into_inner())
}

struct DeferredReport {
    value: Box<dyn std::fmt::Display + Send>,
    pending: Arc<AtomicU64>,
}

impl Drop for DeferredReport {
    /// Releases the report budget whether the message was written or dropped.
    fn drop(&mut self) {
        self.pending.fetch_sub(1, Ordering::Relaxed);
    }
}

/// An ordered queue in front of two output streams, drained by its own thread.
pub struct Console {
    queue: Option<Arc<Queue>>,
    files: Arc<Mutex<Files>>,
    dropped: Arc<AtomicU64>,
    reports: Arc<AtomicU64>,
    dropped_reports: Arc<AtomicU64>,
    raw_stderr: OnceLock<Arc<raw_stderr::RawStderr>>,
}

impl Console {
    /// Starts the writer thread over `stdout` and `stderr`; without a thread, writes go inline.
    pub fn spawn(stdout: impl Write + Send + 'static, stderr: impl Write + Send + 'static) -> Self {
        let queue = Arc::new(Queue::new(Box::new(queue::SystemClock)));
        let files = Arc::new(Mutex::new(Files::default()));
        let dropped = Arc::new(AtomicU64::new(0));
        let dropped_reports = Arc::new(AtomicU64::new(0));
        let writer = Writer {
            queue: Arc::clone(&queue),
            files: Arc::clone(&files),
            dropped: Arc::clone(&dropped),
            dropped_reports: Arc::clone(&dropped_reports),
            stdout: Box::new(stdout),
            stderr: Box::new(stderr),
        };
        let spawned = std::thread::Builder::new()
            .name("console-writer".to_owned())
            .spawn(move || writer.run());
        Self {
            queue: spawned.is_ok().then_some(queue),
            files,
            dropped,
            reports: Arc::new(AtomicU64::new(0)),
            dropped_reports,
            raw_stderr: OnceLock::new(),
        }
    }

    /// Queues bytes at [`Priority::Normal`].
    pub fn write(&self, stream: Stream, bytes: Vec<u8>) {
        self.write_at(stream, Priority::Normal, bytes);
    }

    /// Queues bytes; a full queue drops this write and reports the loss later, except that an
    /// error first waits a bounded time for space.
    pub fn write_at(&self, stream: Stream, priority: Priority, bytes: Vec<u8>) {
        let Some(queue) = &self.queue else {
            write_inline(stream, &bytes);
            return;
        };
        match queue.push(Message::Write {
            stream,
            priority,
            bytes,
        }) {
            Ok(()) => {}
            Err(Rejected::Full) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(Rejected::Closed(Message::Write { stream, bytes, .. })) => {
                write_inline(stream, &bytes);
            }
            Err(Rejected::Closed(_)) => {}
        }
    }

    /// Queues a report without formatting or waiting; unavailable writers drop it too.
    pub fn report(&self, stream: Stream, report: Box<dyn std::fmt::Display + Send>) {
        if self
            .reports
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < QUEUED_REPORTS).then_some(count + 1)
            })
            .is_err()
        {
            self.dropped_reports.fetch_add(1, Ordering::Relaxed);
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let report = DeferredReport {
            value: report,
            pending: Arc::clone(&self.reports),
        };
        if self
            .queue
            .as_ref()
            .is_none_or(|queue| queue.push(Message::Report(stream, report)).is_err())
        {
            self.dropped_reports.fetch_add(1, Ordering::Relaxed);
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Waits for queued output unless its bounded flush request cannot be admitted.
    pub fn flush(&self) {
        let Some(queue) = &self.queue else {
            return;
        };
        if let Some(raw) = self.raw_stderr.get() {
            raw.drain(queue, None);
        }
        let (done, flushed) = sync_channel(1);
        if queue.push_control(Message::Flush(done)).is_ok() {
            let _ = flushed.recv();
        }
    }

    /// Like [`Console::flush`], but gives up after `timeout`, as a panic or forced exit must
    /// not hang on a stalled console. Returns whether every earlier write reached its stream.
    pub fn flush_within(&self, timeout: Duration) -> bool {
        let Some(queue) = &self.queue else {
            return true;
        };
        let deadline = Instant::now() + timeout;
        if let Some(raw) = self.raw_stderr.get()
            && !raw.drain(queue, Some(deadline))
        {
            return false;
        }
        let (done, flushed) = sync_channel(1);
        if queue.push_control(Message::Flush(done)).is_err() {
            return false;
        }
        flushed
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
    }

    /// Whether raw writes to the process stderr reach the session file.
    pub fn captures_raw_stderr(&self) -> bool {
        self.raw_stderr.get().is_some()
    }

    /// Opens the session files, then captures raw stderr while queued stderr output keeps
    /// reaching `destination`, the launcher's original stderr.
    fn attach(
        &self,
        files: &SessionFiles<'_>,
        destination: Option<std::fs::File>,
    ) -> io::Result<()> {
        {
            let mut sinks = lock(&self.files);
            if sinks.log.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "session files already attached",
                ));
            }
            #[cfg_attr(not(unix), expect(unused_mut, reason = "only Unix follows on signal"))]
            let mut log = SessionLog::open(
                Family::new(files.dir, files.stem, "log"),
                files.retention,
                format!("{}\n", files.header).into_bytes(),
            )?;
            #[cfg(unix)]
            log.follow_on_signal()?;
            sinks.attach_log(log);
            // The JSON record is optional tooling output; failing to open it keeps the plain
            // session log and says why in it.
            if files.json {
                let header = serde_json::json!({ "session_start": files.header });
                match SessionLog::open(
                    Family::new(files.dir, files.stem, "jsonl"),
                    files.retention,
                    format!("{header}\n").into_bytes(),
                ) {
                    Ok(json) => sinks.json = Some(json),
                    Err(error) => sinks.write_plain(
                        format!("console: JSON log unavailable: {error}\n").as_bytes(),
                    ),
                }
            }
        }
        let Some(queue) = &self.queue else {
            return Ok(());
        };
        #[cfg(unix)]
        signal_drain::publish_terminal(destination.as_ref().map(std::os::fd::AsRawFd::as_raw_fd));
        let destination: Box<dyn Write + Send> = match destination {
            Some(file) => terminal::writer(file),
            None => Box::new(io::sink()),
        };
        if queue.push_control(Message::Redirect(destination)).is_err() {
            return Ok(());
        }
        // Writes the writer handles before the switch still go to fd 2; capturing fd 2 before
        // then would copy them into the file twice.
        if !self.flush_within(ATTACH_TIMEOUT) {
            return Err(io::Error::other(
                "console writer stalled; raw stderr is not captured",
            ));
        }
        let raw = raw_stderr::RawStderr::start(Arc::clone(queue), Arc::clone(&self.dropped))?;
        let _ = self.raw_stderr.set(raw);
        Ok(())
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        if let Some(queue) = &self.queue {
            queue.close();
        }
    }
}

/// The writer thread's state: it alone touches the terminal streams.
struct Writer {
    queue: Arc<Queue>,
    files: Arc<Mutex<Files>>,
    dropped: Arc<AtomicU64>,
    dropped_reports: Arc<AtomicU64>,
    stdout: Box<dyn Write + Send>,
    stderr: Box<dyn Write + Send>,
}

impl Writer {
    fn run(mut self) {
        while let Some(message) = self.queue.pop() {
            let count = self.dropped.swap(0, Ordering::Relaxed);
            if count > 0 {
                let reports = self.dropped_reports.swap(0, Ordering::Relaxed);
                let notice = if reports == 0 {
                    format!("console queue full: dropped {count} writes\n")
                } else {
                    format!("console queue full: dropped {count} writes ({reports} reports)\n")
                };
                lock(&self.files).record(Stream::FileOnly, notice.as_bytes());
                self.terminal(Stream::Stderr, notice.as_bytes());
            }
            match message {
                Message::Write { stream, bytes, .. } => {
                    lock(&self.files).record(stream, &bytes);
                    self.terminal(stream, &bytes);
                }
                Message::Report(stream, report) => {
                    let bytes = format!("{}\n", report.value).into_bytes();
                    drop(report);
                    lock(&self.files).record(stream, &bytes);
                    self.terminal(stream, &bytes);
                }
                Message::WriteConsole(stream, bytes) => self.terminal(stream, &bytes),
                Message::Redirect(destination) => self.stderr = destination,
                Message::Flush(done) => {
                    let _ = done.send(());
                }
            }
        }
    }

    fn terminal(&mut self, stream: Stream, bytes: &[u8]) {
        let sink = match stream {
            Stream::Stdout => &mut self.stdout,
            Stream::Stderr => &mut self.stderr,
            Stream::FileOnly | Stream::Json => return,
        };
        let _ = sink.write_all(bytes);
        let _ = sink.flush();
    }
}

/// The fallback when no writer thread runs.
fn write_inline(stream: Stream, bytes: &[u8]) {
    let _ = match stream {
        Stream::Stdout => {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(bytes).and_then(|()| stdout.flush())
        }
        Stream::Stderr => std::io::stderr().lock().write_all(bytes),
        Stream::FileOnly | Stream::Json => Ok(()),
    };
}

/// Drops terminal escape sequences so the session file stays plain text; state carries across
/// writes because raw stderr arrives in arbitrary chunks.
#[derive(Default)]
struct AnsiFilter {
    state: AnsiState,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum AnsiState {
    #[default]
    Text,
    Escape,
    Sequence,
}

impl AnsiFilter {
    fn strip(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            self.state = match (self.state, byte) {
                (AnsiState::Text, 0x1b) => AnsiState::Escape,
                (AnsiState::Text, _) => {
                    out.push(byte);
                    AnsiState::Text
                }
                (AnsiState::Escape, b'[') => AnsiState::Sequence,
                // Two-byte escapes end at their second byte.
                (AnsiState::Escape, _) => AnsiState::Text,
                (AnsiState::Sequence, 0x40..=0x7e) => AnsiState::Text,
                (AnsiState::Sequence, _) => AnsiState::Sequence,
            };
        }
        out
    }
}

/// The process console, started on first use.
pub fn console() -> &'static Console {
    &CONSOLE
}

/// Waits for queued console output, as before the process exits.
pub fn flush() {
    console().flush();
}

/// Waits a bounded time for queued console output before a panic report or forced exit, so
/// earlier lines precede it without a stalled console holding the exit.
pub fn flush_before_exit() -> bool {
    console().flush_within(EXIT_FLUSH_TIMEOUT)
}

/// For fatal-signal handlers, which must not lock or allocate: copies raw stderr still in the
/// capture pipe, such as an abort message written just before the signal, to the session log.
/// It forwards to the launcher's stderr only when that destination already uses
/// nonblocking I/O. Async-signal-safe; acts once, and only on Unix
/// once raw stderr is captured. Queued output is not flushed.
pub fn drain_raw_stderr_on_signal() {
    #[cfg(unix)]
    signal_drain::drain();
}

/// A writer that hands what it collected to the console when flushed or dropped, so one
/// marker or log event reaches its stream as one queued write, in order with the others.
pub struct Queued {
    stream: Stream,
    priority: Priority,
    pending: Vec<u8>,
}

impl Queued {
    /// A writer for `stream` whose output the console keeps at `priority`.
    #[must_use]
    pub fn new(stream: Stream, priority: Priority) -> Self {
        Self {
            stream,
            priority,
            pending: Vec::new(),
        }
    }
}

impl Write for Queued {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.pending.is_empty() {
            console().write_at(
                self.stream,
                self.priority,
                std::mem::take(&mut self.pending),
            );
        }
        Ok(())
    }
}

impl Drop for Queued {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// Stdout through the console queue; use in place of `std::io::stdout().lock()` on the frame.
#[must_use]
pub fn stdout() -> Queued {
    Queued::new(Stream::Stdout, Priority::Normal)
}

/// Stderr through the console queue at [`Priority::Normal`].
#[must_use]
pub fn stderr() -> Queued {
    Queued::new(Stream::Stderr, Priority::Normal)
}

/// Formats one diagnostic report on the console thread, with bounded queue loss accounting.
pub fn defer_stderr(report: Box<dyn std::fmt::Display + Send>) {
    console().report(Stream::Stderr, report);
}

/// The session files the console mirrors its output into: `dir/<stem>.log`, and
/// `dir/<stem>.jsonl` when `json` is set, each archived per session under `retention`.
pub struct SessionFiles<'a> {
    pub dir: &'a Path,
    pub stem: &'a str,
    /// The first line of each file, without its newline; repeated when an oversized session
    /// continues in a fresh file. The JSON file stores it as `{"session_start": header}`.
    pub header: &'a str,
    pub json: bool,
    pub retention: Retention,
}

/// Attaches the session files to the process console, then captures raw stderr into them
/// while terminal output keeps reaching the launcher's stderr. Fails when files are already
/// attached; the file mirror stays attached even if stderr capture then fails.
pub fn initialize_log(files: &SessionFiles<'_>) -> io::Result<()> {
    let destination = raw_stderr::original().ok().flatten();
    console().attach(files, destination)
}

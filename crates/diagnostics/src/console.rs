//! Console output written in order by one background thread, so a slow or paused console (a
//! terminal selection, a full pipe) never stalls the frame thread that logs or emits a marker.

mod raw_stderr;
mod synchronized;

use std::{
    io::{IsTerminal, Write},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    time::{Duration, Instant},
};

/// Writes queued before a writer waits; bounds memory while the console is stalled.
const QUEUED_WRITES: usize = 4096;
/// Large owned snapshots have a separate bound inside the console queue.
const QUEUED_REPORTS: u64 = 8;
static CONSOLE: OnceLock<Console> = OnceLock::new();
/// Pause between attempts to queue a bounded flush behind a full queue.
const FULL_QUEUE_RETRY: Duration = Duration::from_millis(1);
/// Longest a panic or forced exit waits for queued output.
const EXIT_FLUSH_TIMEOUT: Duration = Duration::from_millis(500);

/// Where a queued write goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
    /// Records a message in the file while another hook handles its terminal output.
    FileOnly,
}

enum Message {
    Write(Stream, Vec<u8>),
    /// A bounded report formatted only when the writer reaches it.
    Report(Stream, DeferredReport),
    /// Terminal output whose file copy has already been synchronized.
    WriteConsole(Stream, Vec<u8>),
    /// Acknowledged once every earlier write has reached its stream.
    Flush(SyncSender<()>),
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
    queue: Option<SyncSender<Message>>,
    log: Option<Arc<Mutex<crate::rotating_log::RotatingLog>>>,
    dropped: Arc<AtomicU64>,
    reports: Arc<AtomicU64>,
    dropped_reports: Arc<AtomicU64>,
    raw_stderr: Option<Arc<raw_stderr::RawStderr>>,
}

impl Console {
    /// Starts the writer thread over `stdout` and `stderr`; without a thread, writes go inline.
    pub fn spawn(stdout: impl Write + Send + 'static, stderr: impl Write + Send + 'static) -> Self {
        Self::spawn_with_log(stdout, stderr, None)
    }

    /// Mirrors queued writes to disk on the writer thread before sending them to their output streams.
    fn spawn_with_log(
        mut stdout: impl Write + Send + 'static,
        mut stderr: impl Write + Send + 'static,
        log: Option<crate::rotating_log::RotatingLog>,
    ) -> Self {
        let (queue, pending): (SyncSender<Message>, Receiver<Message>) =
            sync_channel(QUEUED_WRITES);
        let dropped = Arc::new(AtomicU64::new(0));
        let lost = Arc::clone(&dropped);
        let dropped_reports = Arc::new(AtomicU64::new(0));
        let lost_reports = Arc::clone(&dropped_reports);
        let mut plain = AnsiFilter::default();
        let log = log.map(|log| Arc::new(Mutex::new(log)));
        let writer_log = log.clone();
        let spawned = std::thread::Builder::new()
            .name("console-writer".to_owned())
            .spawn(move || {
                for message in pending {
                    let count = lost.swap(0, Ordering::Relaxed);
                    if count > 0 {
                        let reports = lost_reports.swap(0, Ordering::Relaxed);
                        let notice = if reports == 0 {
                            format!("console queue full: dropped {count} writes\n")
                        } else {
                            format!(
                                "console queue full: dropped {count} writes ({reports} reports)\n"
                            )
                        };
                        if let Some(log) = &writer_log {
                            let mut log = log.lock().unwrap_or_else(|error| error.into_inner());
                            let _ = log.write_all(notice.as_bytes()).and_then(|()| log.flush());
                        }
                        let _ = stderr.write_all(notice.as_bytes());
                        let _ = stderr.flush();
                    }
                    let message = match message {
                        Message::Report(stream, report) => {
                            Message::Write(stream, format!("{}\n", report.value).into_bytes())
                        }
                        message => message,
                    };
                    let mirror = matches!(&message, Message::Write(..));
                    match message {
                        Message::Write(stream, bytes) | Message::WriteConsole(stream, bytes) => {
                            if mirror && let Some(log) = &writer_log {
                                let mut log = log.lock().unwrap_or_else(|error| error.into_inner());
                                let text = plain.strip(&bytes);
                                let _ = log.write_all(&text).and_then(|()| log.flush());
                            }
                            let mut discard = std::io::sink();
                            let sink: &mut dyn Write = match stream {
                                Stream::Stdout => &mut stdout,
                                Stream::Stderr => &mut stderr,
                                Stream::FileOnly => &mut discard,
                            };
                            let _ = sink.write_all(&bytes);
                            let _ = sink.flush();
                        }
                        Message::Flush(done) => {
                            let _ = done.send(());
                        }
                        Message::Report(..) => unreachable!(),
                    }
                }
            });
        Self {
            queue: spawned.is_ok().then_some(queue),
            log,
            dropped,
            reports: Arc::new(AtomicU64::new(0)),
            dropped_reports,
            raw_stderr: None,
        }
    }

    /// Queues bytes without waiting; a full queue drops this write and reports the loss later.
    pub fn write(&self, stream: Stream, bytes: Vec<u8>) {
        let Some(queue) = &self.queue else {
            write_inline(stream, &bytes);
            return;
        };
        match queue.try_send(Message::Write(stream, bytes)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(Message::Write(stream, bytes))) => {
                write_inline(stream, &bytes);
            }
            Err(TrySendError::Disconnected(_)) => unreachable!(),
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
            .is_none_or(|queue| queue.try_send(Message::Report(stream, report)).is_err())
        {
            self.dropped_reports.fetch_add(1, Ordering::Relaxed);
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Waits until every write queued before this call has reached its stream.
    pub fn flush(&self) {
        let Some(queue) = &self.queue else {
            return;
        };
        if let Some(raw) = &self.raw_stderr {
            raw.drain(queue, None);
        }
        let (done, flushed) = sync_channel(1);
        if queue.send(Message::Flush(done)).is_ok() {
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
        if let Some(raw) = &self.raw_stderr
            && !raw.drain(queue, Some(deadline))
        {
            return false;
        }
        let (done, flushed) = sync_channel(1);
        let mut message = Message::Flush(done);
        loop {
            match queue.try_send(message) {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => return true,
                Err(TrySendError::Full(returned)) => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    message = returned;
                    std::thread::sleep(FULL_QUEUE_RETRY);
                }
            }
        }
        flushed
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
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
        Stream::FileOnly => Ok(()),
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
    CONSOLE.get_or_init(|| Console::spawn(std::io::stdout(), std::io::stderr()))
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

/// A writer that hands what it collected to the console when flushed or dropped, so one
/// marker or log event reaches its stream as one queued write, in order with the others.
pub struct Queued {
    stream: Stream,
    pending: Vec<u8>,
}

impl Write for Queued {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.pending.is_empty() {
            console().write(self.stream, std::mem::take(&mut self.pending));
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
    Queued {
        stream: Stream::Stdout,
        pending: Vec::new(),
    }
}

/// Stderr through the console queue, as a `tracing_subscriber` writer factory for log events.
#[must_use]
pub fn stderr() -> Queued {
    Queued {
        stream: Stream::Stderr,
        pending: Vec::new(),
    }
}

/// Formats one diagnostic report on the console thread, with bounded queue loss accounting.
pub fn defer_stderr(report: Box<dyn std::fmt::Display + Send>) {
    console().report(Stream::Stderr, report);
}

/// Initializes the rotating sink, capturing raw stderr for nonterminal launches before any writes.
pub fn initialize_log(path: &std::path::Path, limit: u64) -> std::io::Result<()> {
    let terminal = std::io::stderr().is_terminal();
    let destination = if terminal {
        None
    } else {
        raw_stderr::original()?
    };
    initialize_log_with_destination(path, limit, terminal, destination)
}

/// Initializes the file mirror with the launcher's optional nonterminal stderr destination.
fn initialize_log_with_destination(
    path: &std::path::Path,
    limit: u64,
    terminal: bool,
    destination: Option<std::fs::File>,
) -> std::io::Result<()> {
    if CONSOLE.get().is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "console already initialized",
        ));
    }
    let log = crate::rotating_log::RotatingLog::open(path, limit)?;
    let stderr: Box<dyn Write + Send> = if terminal {
        Box::new(std::io::stderr())
    } else {
        // Keep the launch destination before fd 2 becomes the capture pipe.
        match destination {
            Some(destination) => Box::new(destination),
            None => Box::new(std::io::sink()),
        }
    };
    let mut console = Console::spawn_with_log(std::io::stdout(), stderr, Some(log));
    if !terminal {
        let queue = console
            .queue
            .as_ref()
            .ok_or_else(|| std::io::Error::other("console writer unavailable"))?;
        console.raw_stderr = Some(raw_stderr::RawStderr::start(
            queue.clone(),
            Arc::clone(&console.dropped),
        )?);
    }
    CONSOLE.set(console).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "console already initialized",
        )
    })
}

/// Writes one diagnostic line through the console queue and its optional session file.
#[macro_export]
macro_rules! log_stderr {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!($crate::console::stderr(), $($arg)*);
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    /// Checks the formatting thread and counts only reports the writer actually consumes.
    struct ReportProbe {
        producer: std::thread::ThreadId,
        formatted: Arc<AtomicU64>,
    }

    impl std::fmt::Display for ReportProbe {
        /// Formatting must happen only after the console's stalled destination resumes.
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            assert_ne!(std::thread::current().id(), self.producer);
            self.formatted.fetch_add(1, Ordering::Relaxed);
            f.write_str("owned snapshot")
        }
    }

    /// Stalled output neither formats on the producer nor retains more than the report budget.
    #[test]
    fn deferred_reports_stay_bounded_and_format_on_the_writer() {
        let (release, stalled) = mpsc::channel();
        let (entered, started) = mpsc::channel();
        let console = Console::spawn(
            Stalled {
                release: Some(stalled),
                entered: Some(entered),
                written: Arc::new(Mutex::new(Vec::new())),
            },
            std::io::sink(),
        );
        console.write(Stream::Stdout, b"hold writer\n".to_vec());
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        let formatted = Arc::new(AtomicU64::new(0));
        for _ in 0..QUEUED_REPORTS + 2 {
            console.report(
                Stream::Stderr,
                Box::new(ReportProbe {
                    producer: std::thread::current().id(),
                    formatted: Arc::clone(&formatted),
                }),
            );
        }
        assert_eq!(formatted.load(Ordering::Relaxed), 0);
        assert_eq!(console.reports.load(Ordering::Relaxed), QUEUED_REPORTS);
        assert_eq!(console.dropped.load(Ordering::Relaxed), 2);
        assert_eq!(console.dropped_reports.load(Ordering::Relaxed), 2);
        release.send(()).unwrap();
        console.flush();
        assert_eq!(formatted.load(Ordering::Relaxed), QUEUED_REPORTS);
        assert_eq!(console.reports.load(Ordering::Relaxed), 0);
    }

    /// Recreates a launcher with no stderr destination in a separate process.
    #[test]
    fn missing_stderr_child() {
        let Some(path) = std::env::var_os("CINNABAR_MISSING_STDERR_FIXTURE") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, SetStdHandle};
            // SAFETY: Only this fixture process loses its launcher-owned stderr handle.
            assert_ne!(
                unsafe { SetStdHandle(STD_ERROR_HANDLE, std::ptr::null_mut()) },
                0
            );
            initialize_log(&path, 4096).unwrap();
        }
        #[cfg(not(windows))]
        initialize_log_with_destination(&path, 4096, false, None).unwrap();
        crate::log_stderr!("queued session marker");
        eprintln!("raw session marker");
        assert!(flush_before_exit());
    }

    /// Losing the launcher's console must not leave a newly created session file empty.
    #[test]
    fn missing_stderr_keeps_queued_and_raw_diagnostics() {
        let path =
            std::env::temp_dir().join(format!("cinnabar-missing-stderr-{}", std::process::id()));
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "console::tests::missing_stderr_child",
                "--nocapture",
            ])
            .env("CINNABAR_MISSING_STDERR_FIXTURE", &path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "fixture failed: {output:?}");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        for marker in ["queued session marker", "raw session marker"] {
            assert_eq!(
                text.matches(marker).count(),
                1,
                "lost or repeated diagnostic: {text}"
            );
        }
    }

    #[test]
    fn session_file_drops_terminal_colours_split_across_writes() {
        let mut filter = AnsiFilter::default();
        let mut text = filter.strip(b"\x1b[2m2026\x1b[0m \x1b[3");
        text.extend(filter.strip(b"2m INFO\x1b[0m done\n"));
        assert_eq!(text, b"2026  INFO done\n");
    }

    #[test]
    fn console_mirrors_both_streams_into_the_bounded_session_file() {
        let path =
            std::env::temp_dir().join(format!("cinnabar-console-file-{}", std::process::id()));
        let log = crate::rotating_log::RotatingLog::open(&path, 128).unwrap();
        let console = Console::spawn_with_log(std::io::sink(), std::io::sink(), Some(log));
        console.write(Stream::Stdout, b"stdout marker\n".to_vec());
        console.write(Stream::Stderr, b"stderr event\n".to_vec());
        console.flush();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "stdout marker\nstderr event\n"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn file_only_messages_do_not_duplicate_the_terminal_hook() {
        let path =
            std::env::temp_dir().join(format!("cinnabar-console-panic-{}", std::process::id()));
        let log = crate::rotating_log::RotatingLog::open(&path, 128).unwrap();
        let written = Arc::new(Mutex::new(Vec::new()));
        let console = Console::spawn_with_log(
            std::io::sink(),
            Stalled {
                release: None,
                entered: None,
                written: Arc::clone(&written),
            },
            Some(log),
        );
        console.write(Stream::FileOnly, b"panic details\n".to_vec());
        console.flush();
        assert!(written.lock().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "panic details\n");
        std::fs::remove_file(path).unwrap();
    }

    /// A stream that waits for a release before its first write, then records everything.
    struct Stalled {
        release: Option<mpsc::Receiver<()>>,
        entered: Option<mpsc::Sender<()>>,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for Stalled {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Some(release) = self.release.take() {
                if let Some(entered) = self.entered.take() {
                    let _ = entered.send(());
                }
                let _ = release.recv_timeout(Duration::from_secs(30));
            }
            self.written.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Markers and log lines queue in order while the console is stalled; the caller returns at
    /// once instead of waiting for the console.
    #[test]
    fn a_stalled_console_never_blocks_the_writer() {
        let (release, stalled) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let stdout = Stalled {
            release: Some(stalled),
            entered: None,
            written: Arc::clone(&written),
        };
        let console = Arc::new(Console::spawn(stdout, std::io::sink()));
        let (done, finished) = mpsc::channel();
        let writer = Arc::clone(&console);
        std::thread::spawn(move || {
            for index in 0..100 {
                writer.write(Stream::Stdout, format!("marker {index}\n").into_bytes());
            }
            done.send(()).unwrap();
        });
        finished
            .recv_timeout(Duration::from_secs(10))
            .expect("queued writes return while the console is stalled");
        release.send(()).unwrap();
        console.flush();
        let expected: String = (0..100).map(|index| format!("marker {index}\n")).collect();
        assert_eq!(
            String::from_utf8(written.lock().unwrap().clone()).unwrap(),
            expected
        );
    }

    /// A panic or forced exit gives up on a stalled console instead of hanging, and still sees
    /// the output through once the console resumes.
    #[test]
    fn a_bounded_flush_gives_up_on_a_stalled_console() {
        let (release, stalled) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let stdout = Stalled {
            release: Some(stalled),
            entered: None,
            written: Arc::clone(&written),
        };
        let console = Console::spawn(stdout, std::io::sink());
        console.write(Stream::Stdout, b"last line\n".to_vec());
        assert!(!console.flush_within(Duration::from_millis(20)));
        release.send(()).unwrap();
        assert!(console.flush_within(Duration::from_secs(10)));
        assert_eq!(written.lock().unwrap().as_slice(), b"last line\n");
    }

    /// Overflow remains visible in the session file when stderr has no terminal sink.
    #[test]
    fn dropped_writes_are_reported_in_the_log_without_a_terminal() {
        let path =
            std::env::temp_dir().join(format!("cinnabar-console-dropped-{}", std::process::id()));
        let log = crate::rotating_log::RotatingLog::open(
            &path,
            (QUEUED_WRITES * b"line\n".len() + 512) as u64,
        )
        .unwrap();
        let (release, stalled) = mpsc::channel();
        let (entered, started) = mpsc::channel();
        let console = Console::spawn_with_log(
            Stalled {
                release: Some(stalled),
                entered: Some(entered),
                written: Arc::new(Mutex::new(Vec::new())),
            },
            std::io::sink(),
            Some(log),
        );
        console.write(Stream::Stdout, b"first\n".to_vec());
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        for _ in 0..QUEUED_WRITES + 2 {
            console.write(Stream::Stdout, b"line\n".to_vec());
        }
        let dropped = console.dropped.load(Ordering::Relaxed);
        release.send(()).unwrap();
        console.flush();
        assert!(dropped > 0);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(text.contains(&format!("console queue full: dropped {dropped} writes\n")));
        assert_eq!(text.matches("console queue full:").count(), 1);
    }

    /// A full stalled queue drops excess writes instead of blocking the frame.
    #[test]
    fn a_full_queue_never_blocks_the_writer() {
        let (release, stalled) = mpsc::channel();
        let (entered, started) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let console = Arc::new(Console::spawn(
            Stalled {
                release: Some(stalled),
                entered: Some(entered),
                written,
            },
            std::io::sink(),
        ));
        console.write(Stream::Stdout, b"first\n".to_vec());
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        let (done, finished) = mpsc::channel();
        let writer = Arc::clone(&console);
        let worker = std::thread::spawn(move || {
            for _ in 0..QUEUED_WRITES + 2 {
                writer.write(Stream::Stdout, b"line\n".to_vec());
            }
            done.send(()).unwrap();
        });
        let result = finished.recv_timeout(Duration::from_secs(10));
        let dropped = console.dropped.load(Ordering::Relaxed);
        release.send(()).unwrap();
        worker.join().unwrap();
        result.expect("a full console queue returns before the sink resumes");
        assert!(dropped > 0);
        console.flush();
    }
}

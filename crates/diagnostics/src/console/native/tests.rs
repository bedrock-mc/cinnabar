use super::*;
use queue::{RESERVED_CAPACITY, SHARED_CAPACITY};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

/// Mirrors `console` into a fresh session file below `dir` with no header.
pub(super) fn with_log(console: Console, dir: &Path, limit: u64) -> (Console, PathBuf) {
    let family = Family::new(dir, "console", "log");
    let path = family.current();
    let log = SessionLog::open(
        family,
        Retention {
            session_bytes: limit,
            ..Retention::DEFAULT
        },
        Vec::new(),
    )
    .unwrap();
    lock(&console.files).log = Some(log);
    (console, path)
}

/// Recreates a launcher with no stderr destination in a separate process.
#[test]
fn missing_stderr_child() {
    let Some(dir) = std::env::var_os("CINNABAR_MISSING_STDERR_FIXTURE") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let files = SessionFiles {
        dir: &dir,
        stem: "client",
        header: "fixture header",
        json: false,
        retention: Retention::DEFAULT,
    };
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, SetStdHandle};
        // SAFETY: Only this fixture process loses its launcher-owned stderr handle.
        assert_ne!(
            unsafe { SetStdHandle(STD_ERROR_HANDLE, std::ptr::null_mut()) },
            0
        );
        initialize_log(&files).unwrap();
    }
    #[cfg(not(windows))]
    console().attach(&files, None).unwrap();
    crate::log_stderr!("queued session marker");
    eprintln!("raw session marker");
    assert!(flush_before_exit());
}

/// Losing the launcher's console must not leave a newly created session file empty.
#[test]
fn missing_stderr_keeps_queued_and_raw_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "console::native::tests::missing_stderr_child",
            "--nocapture",
        ])
        .env("CINNABAR_MISSING_STDERR_FIXTURE", dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "fixture failed: {output:?}");
    let text = std::fs::read_to_string(dir.path().join("client.log")).unwrap();
    assert!(text.starts_with("fixture header\n"), "{text}");
    for marker in ["queued session marker", "raw session marker"] {
        assert_eq!(
            text.matches(marker).count(),
            1,
            "lost or repeated diagnostic: {text}"
        );
    }
}

/// Queued and raw stderr reach the session file and the launcher's stderr once each; the
/// launcher's destination is kept whether or not it is a terminal.
#[test]
fn raw_stderr_child() {
    let Some(dir) = std::env::var_os("CINNABAR_RAW_STDERR_FIXTURE") else {
        return;
    };
    let dir = PathBuf::from(dir);
    initialize_log(&SessionFiles {
        dir: &dir,
        stem: "client",
        header: "fixture header",
        json: false,
        retention: Retention::DEFAULT,
    })
    .unwrap();
    assert!(console().captures_raw_stderr());
    crate::log_stderr!("queued once");
    eprintln!("raw once");
    assert!(flush_before_exit());
}

#[test]
fn raw_stderr_reaches_the_file_and_the_launcher_once_each() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "console::native::tests::raw_stderr_child",
            "--nocapture",
        ])
        .env("CINNABAR_RAW_STDERR_FIXTURE", dir.path())
        .stdout(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "fixture failed: {output:?}");
    let text = std::fs::read_to_string(dir.path().join("client.log")).unwrap();
    let launcher = String::from_utf8_lossy(&output.stderr);
    for marker in ["queued once", "raw once"] {
        assert_eq!(text.matches(marker).count(), 1, "file: {text}");
        assert_eq!(launcher.matches(marker).count(), 1, "stderr: {launcher}");
    }
}

/// Writes raw stderr the drain thread cannot reach, then dies the way a fatal-signal handler
/// ends, with only the signal-time drain to save it.
#[cfg(unix)]
#[test]
fn signal_drain_child() {
    let Some(dir) = std::env::var_os("CINNABAR_SIGNAL_DRAIN_FIXTURE") else {
        return;
    };
    let dir = PathBuf::from(dir);
    initialize_log(&SessionFiles {
        dir: &dir,
        stem: "client",
        header: "fixture header",
        json: false,
        retention: Retention::DEFAULT,
    })
    .unwrap();
    assert!(flush_before_exit());
    if std::env::var_os("CINNABAR_SIGNAL_DRAIN_REPLACE").is_some() {
        let _other = SessionLog::open(
            Family::new(&dir, "client", "log"),
            Retention::DEFAULT,
            b"another client\n".to_vec(),
        )
        .unwrap();
    }
    // Holding the pipe reader keeps the drain thread from queueing what follows.
    let raw = console().raw_stderr.get().unwrap();
    let _held = raw.reader.lock().unwrap();
    std::io::stderr()
        .write_all(b"written just before the fault\n")
        .unwrap();
    drain_raw_stderr_on_signal();
    std::process::abort();
}

/// Fatal output is preserved in the session log without attempting a blocking launcher write.
#[cfg(unix)]
#[test]
fn raw_stderr_in_the_pipe_survives_a_fatal_signal() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "console::native::tests::signal_drain_child",
            "--nocapture",
        ])
        .env("CINNABAR_SIGNAL_DRAIN_FIXTURE", dir.path())
        .stdout(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "fixture did not abort: {output:?}"
    );
    let marker = "written just before the fault";
    let text = std::fs::read_to_string(dir.path().join("client.log")).unwrap();
    let launcher = String::from_utf8_lossy(&output.stderr);
    assert_eq!(text.matches(marker).count(), 1, "file: {text}");
    assert_eq!(launcher.matches(marker).count(), 0, "stderr: {launcher}");
}

#[cfg(unix)]
#[test]
fn signal_drain_keeps_the_reporting_clients_archived_log() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "console::native::tests::signal_drain_child",
            "--nocapture",
        ])
        .env("CINNABAR_SIGNAL_DRAIN_FIXTURE", dir.path())
        .env("CINNABAR_SIGNAL_DRAIN_REPLACE", "1")
        .stdout(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let family = Family::new(dir.path(), "client", "log");
    assert_eq!(
        std::fs::read(family.current()).unwrap(),
        b"another client\n"
    );
    let archived = family.archives().unwrap();
    assert_eq!(archived.len(), 1);
    assert!(
        std::fs::read_to_string(&archived[0])
            .unwrap()
            .contains("written just before the fault")
    );
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
    let dir = tempfile::tempdir().unwrap();
    let (console, path) = with_log(
        Console::spawn(std::io::sink(), std::io::sink()),
        dir.path(),
        128,
    );
    console.write(Stream::Stdout, b"stdout marker\n".to_vec());
    console.write(Stream::Stderr, b"stderr event\n".to_vec());
    console.flush();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "stdout marker\nstderr event\n"
    );
}

/// Output written before the session log attaches follows its header, colours removed.
#[test]
fn output_before_the_log_attaches_follows_the_session_header() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "client", "log");
    let mut files = Files::default();
    files.record(Stream::Stderr, b"\x1b[33mearly\x1b[0m warning\n");
    files.record(Stream::Json, b"{}\n");
    let log = SessionLog::open(family.clone(), Retention::DEFAULT, b"HEADER\n".to_vec()).unwrap();
    files.attach_log(log);
    files.record(Stream::Stdout, b"after\n");
    assert_eq!(
        std::fs::read_to_string(family.current()).unwrap(),
        "HEADER\nearly warning\nafter\n"
    );
}

/// Early output past its bound is counted in the session file instead of growing memory.
#[test]
fn early_output_past_its_bound_is_reported_as_lost() {
    let dir = tempfile::tempdir().unwrap();
    let family = Family::new(dir.path(), "client", "log");
    let mut files = Files::default();
    files.record(Stream::Stderr, &vec![b'x'; EARLY_BYTES + 10]);
    let log = SessionLog::open(family.clone(), Retention::DEFAULT, Vec::new()).unwrap();
    files.attach_log(log);
    let text = std::fs::read_to_string(family.current()).unwrap();
    assert_eq!(text.matches('x').count(), EARLY_BYTES);
    assert!(text.ends_with("console: 10 bytes written before the log opened were not kept\n"));
}

#[test]
fn json_records_reach_only_the_json_file() {
    let dir = tempfile::tempdir().unwrap();
    let written = Arc::new(Mutex::new(Vec::new()));
    let console = Console::spawn(
        std::io::sink(),
        Stalled {
            release: None,
            entered: None,
            written: Arc::clone(&written),
        },
    );
    let (console, path) = with_log(console, dir.path(), 1024);
    let json = Family::new(dir.path(), "console", "jsonl");
    lock(&console.files).json =
        Some(SessionLog::open(json.clone(), Retention::DEFAULT, Vec::new()).unwrap());
    console.write(Stream::Json, b"{\"message\":\"structured\"}\n".to_vec());
    console.write(Stream::Stderr, b"plain\n".to_vec());
    console.flush();
    assert_eq!(
        std::fs::read_to_string(json.current()).unwrap(),
        "{\"message\":\"structured\"}\n"
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "plain\n");
    assert_eq!(written.lock().unwrap().as_slice(), b"plain\n");
}

#[test]
fn file_only_messages_do_not_duplicate_the_terminal_hook() {
    let dir = tempfile::tempdir().unwrap();
    let written = Arc::new(Mutex::new(Vec::new()));
    let (console, path) = with_log(
        Console::spawn(
            std::io::sink(),
            Stalled {
                release: None,
                entered: None,
                written: Arc::clone(&written),
            },
        ),
        dir.path(),
        128,
    );
    console.write(Stream::FileOnly, b"panic details\n".to_vec());
    console.flush();
    assert!(written.lock().unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "panic details\n");
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

/// A console whose stdout stalls on its first write until the returned sender fires; returns
/// once the writer thread is stalled inside that write.
fn stalled_console(written: &Arc<Mutex<Vec<u8>>>) -> (Console, mpsc::Sender<()>) {
    let (release, stalled) = mpsc::channel();
    let (entered, started) = mpsc::channel();
    let console = Console::spawn(
        Stalled {
            release: Some(stalled),
            entered: Some(entered),
            written: Arc::clone(written),
        },
        std::io::sink(),
    );
    console.write(Stream::Stdout, b"first\n".to_vec());
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    (console, release)
}

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
    let (console, release) = stalled_console(&Arc::new(Mutex::new(Vec::new())));
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
    let dir = tempfile::tempdir().unwrap();
    let written = Arc::new(Mutex::new(Vec::new()));
    let (console, release) = stalled_console(&written);
    let (console, path) = with_log(
        console,
        dir.path(),
        (SHARED_CAPACITY * b"line\n".len() + 512) as u64,
    );
    for _ in 0..SHARED_CAPACITY + 2 {
        console.write(Stream::Stdout, b"line\n".to_vec());
    }
    let dropped = console.dropped.load(Ordering::Relaxed);
    release.send(()).unwrap();
    console.flush();
    assert!(dropped > 0);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(&format!("console queue full: dropped {dropped} writes\n")));
    assert_eq!(text.matches("console queue full:").count(), 1);
}

/// A full stalled queue drops excess writes instead of blocking the frame.
#[test]
fn a_full_queue_never_blocks_the_writer() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let (console, release) = stalled_console(&written);
    let console = Arc::new(console);
    let (done, finished) = mpsc::channel();
    let writer = Arc::clone(&console);
    let worker = std::thread::spawn(move || {
        for _ in 0..SHARED_CAPACITY + 2 {
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

/// A flood of normal writes against a stalled writer cannot evict a warning or an error.
#[test]
fn an_info_flood_loses_no_warning() {
    let dir = tempfile::tempdir().unwrap();
    let written = Arc::new(Mutex::new(Vec::new()));
    let (console, release) = stalled_console(&written);
    let (console, path) = with_log(console, dir.path(), 64 << 20);
    let warnings = RESERVED_CAPACITY / 2;
    for index in 0..SHARED_CAPACITY * 4 {
        console.write_at(Stream::Stderr, Priority::Normal, b"info\n".to_vec());
        if index % 8 == 0 && index / 8 < warnings {
            let priority = if index % 16 == 0 {
                Priority::Warn
            } else {
                Priority::Error
            };
            console.write_at(
                Stream::Stderr,
                priority,
                format!("severe {}\n", index / 8).into_bytes(),
            );
        }
    }
    let dropped = console.dropped.load(Ordering::Relaxed);
    release.send(()).unwrap();
    console.flush();
    assert!(dropped > 0, "the flood overflowed the shared capacity");
    let text = std::fs::read_to_string(&path).unwrap();
    for index in 0..warnings {
        assert_eq!(
            text.matches(&format!("severe {index}\n")).count(),
            1,
            "lost severe line {index}"
        );
    }
    assert!(text.contains("console queue full: dropped"));
}

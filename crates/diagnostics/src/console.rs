//! Console output written in order by one background thread, so a slow or paused console (a
//! terminal selection, a full pipe) never stalls the frame thread that logs or emits a marker.

use std::{
    io::Write,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    time::{Duration, Instant},
};

/// Writes queued before a writer waits; bounds memory while the console is stalled.
const QUEUED_WRITES: usize = 4096;
/// Pause between attempts to queue a bounded flush behind a full queue.
const FULL_QUEUE_RETRY: Duration = Duration::from_millis(1);
/// Longest a panic or forced exit waits for queued output.
const EXIT_FLUSH_TIMEOUT: Duration = Duration::from_millis(500);

/// Where a queued write goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

enum Message {
    Write(Stream, Vec<u8>),
    /// Acknowledged once every earlier write has reached its stream.
    Flush(SyncSender<()>),
}

/// An ordered queue in front of two output streams, drained by its own thread.
pub struct Console {
    queue: Option<SyncSender<Message>>,
    dropped: Arc<AtomicU64>,
}

impl Console {
    /// Starts the writer thread over `stdout` and `stderr`; without a thread, writes go inline.
    pub fn spawn(
        mut stdout: impl Write + Send + 'static,
        mut stderr: impl Write + Send + 'static,
    ) -> Self {
        let (queue, pending): (SyncSender<Message>, Receiver<Message>) =
            sync_channel(QUEUED_WRITES);
        let dropped = Arc::new(AtomicU64::new(0));
        let lost = Arc::clone(&dropped);
        let spawned = std::thread::Builder::new()
            .name("console-writer".to_owned())
            .spawn(move || {
                for message in pending {
                    let count = lost.swap(0, Ordering::Relaxed);
                    if count > 0 {
                        let _ = writeln!(stderr, "console queue full: dropped {count} writes");
                        let _ = stderr.flush();
                    }
                    match message {
                        Message::Write(stream, bytes) => {
                            let sink: &mut dyn Write = match stream {
                                Stream::Stdout => &mut stdout,
                                Stream::Stderr => &mut stderr,
                            };
                            let _ = sink.write_all(&bytes);
                            let _ = sink.flush();
                        }
                        Message::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            });
        Self {
            queue: spawned.is_ok().then_some(queue),
            dropped,
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
            Err(TrySendError::Disconnected(Message::Flush(_))) => unreachable!(),
        }
    }

    /// Waits until every write queued before this call has reached its stream.
    pub fn flush(&self) {
        let Some(queue) = &self.queue else {
            return;
        };
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
    };
}

/// The process console, started on first use.
pub fn console() -> &'static Console {
    static CONSOLE: OnceLock<Console> = OnceLock::new();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

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

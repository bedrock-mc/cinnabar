//! Console output written in order by one background thread, so a slow or paused console (a
//! terminal selection, a full pipe) never stalls the frame thread that logs or emits a marker.

use std::{
    io::Write,
    sync::{
        OnceLock,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
};

/// Writes queued before a writer waits; bounds memory while the console is stalled.
const QUEUED_WRITES: usize = 4096;

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
}

impl Console {
    /// Starts the writer thread over `stdout` and `stderr`; without a thread, writes go inline.
    pub fn spawn(
        mut stdout: impl Write + Send + 'static,
        mut stderr: impl Write + Send + 'static,
    ) -> Self {
        let (queue, pending): (SyncSender<Message>, Receiver<Message>) =
            sync_channel(QUEUED_WRITES);
        let spawned = std::thread::Builder::new()
            .name("console-writer".to_owned())
            .spawn(move || {
                for message in pending {
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
        }
    }

    /// Queues `bytes` behind every earlier write; waits only while the queue is full.
    pub fn write(&self, stream: Stream, bytes: Vec<u8>) {
        let Some(queue) = &self.queue else {
            write_inline(stream, &bytes);
            return;
        };
        if let Err(error) = queue.send(Message::Write(stream, bytes)) {
            let Message::Write(stream, bytes) = error.0 else {
                return;
            };
            write_inline(stream, &bytes);
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
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for Stalled {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Some(release) = self.release.take() {
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
}

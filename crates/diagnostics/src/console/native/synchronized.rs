//! File synchronization that cannot wait on the console writer or its external streams.

use super::*;

impl Console {
    /// Writes and flushes the file directly, then queues terminal output without waiting.
    /// A missing writer or full queue may lose terminal output but retains the file copy.
    pub fn write_synced(&self, stream: Stream, bytes: Vec<u8>) {
        lock(&self.files).write_plain(&AnsiFilter::default().strip(&bytes));
        if let Some(queue) = &self.queue
            && let Err(Rejected::Full) = queue.push(Message::WriteConsole(stream, bytes))
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::native::{queue::SHARED_CAPACITY, tests::with_log};
    use std::sync::mpsc;

    /// Holds terminal output until the test releases it, independently of file writes.
    struct Paused {
        entered: mpsc::Sender<()>,
        release: Option<mpsc::Receiver<()>>,
    }

    impl Write for Paused {
        /// Signals that the writer is inside terminal I/O, then waits for an explicit release.
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Some(release) = self.release.take() {
                self.entered.send(()).unwrap();
                release.recv().unwrap();
            }
            Ok(bytes.len())
        }

        /// This fixture stalls writes, so flushing requires no additional coordination.
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn file_sync_completes_while_the_console_and_its_queue_are_stalled() {
        let dir = tempfile::tempdir().unwrap();
        let (entered, started) = mpsc::channel();
        let (release, paused) = mpsc::channel();
        let (console, path) = with_log(
            Console::spawn(
                std::io::sink(),
                Paused {
                    entered,
                    release: Some(paused),
                },
            ),
            dir.path(),
            1024 * 1024,
        );
        let console = Arc::new(console);
        console.write(Stream::Stderr, b"before pause\n".to_vec());
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        for _ in 0..SHARED_CAPACITY {
            console.write(Stream::Stdout, b"queued\n".to_vec());
        }
        let (done, finished) = mpsc::channel();
        let writer = console.clone();
        let worker = std::thread::spawn(move || {
            writer.write_synced(Stream::Stdout, b"renderer boundary\n".to_vec());
            done.send(()).unwrap();
        });
        let completed = finished.recv_timeout(Duration::from_secs(10));
        let before_release = std::fs::read_to_string(&path).unwrap();
        release.send(()).unwrap();
        worker.join().unwrap();
        completed.expect("file synchronization does not wait for the terminal");
        assert!(before_release.contains("renderer boundary\n"));
        assert!(console.flush_within(Duration::from_secs(10)));
        console.write_synced(Stream::Stdout, b"after resume\n".to_vec());
        assert!(console.flush_within(Duration::from_secs(10)));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(saved.matches("renderer boundary\n").count(), 1);
        assert_eq!(saved.matches("after resume\n").count(), 1);
    }
}

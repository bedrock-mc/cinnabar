//! Bounded result decoding that never makes a polling UI wait for an unfinished reader.

use super::{OUTPUT_BYTES, RESULT_NESTING, Response, read_limited};
use std::{
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

/// One decoder, cancelled cooperatively and joined only after completion during normal polling.
pub(super) struct Decoder {
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<Response, String>>>,
}

impl Decoder {
    /// Starts all payload reads and decoding on a stack sized for accepted expression trees.
    pub(super) fn start(reader: impl Read + Send + 'static) -> Result<Self, String> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = std::thread::Builder::new()
            .name("shader compiler result".into())
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let bytes = read_limited(
                    CancelledReader {
                        reader,
                        cancelled: &worker_cancelled,
                    },
                    OUTPUT_BYTES,
                )
                .map_err(|error| error.to_string())?;
                let result: Result<Response, String> =
                    ciborium::de::from_reader_with_recursion_limit(
                        CancelledReader {
                            reader: bytes.as_slice(),
                            cancelled: &worker_cancelled,
                        },
                        RESULT_NESTING,
                    )
                    .map_err(|error| format!("invalid shader compiler result: {error}"))?;
                if worker_cancelled.load(Ordering::Acquire) {
                    Err("shader result decoding cancelled".into())
                } else {
                    result
                }
            })
            .map_err(|error| format!("cannot start shader result decoder: {error}"))?;
        Ok(Self {
            cancelled,
            worker: Some(worker),
        })
    }

    /// Stops future payload reads, including while CBOR is being decoded.
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Takes a completed result; an unfinished reader always returns immediately.
    pub(super) fn poll(&mut self) -> Option<Result<Response, String>> {
        if !self.worker.as_ref()?.is_finished() {
            return None;
        }
        Some(
            self.worker
                .take()
                .unwrap()
                .join()
                .unwrap_or_else(|_| Err("shader result decoder stopped".into())),
        )
    }
}

impl Drop for Decoder {
    /// Cancels and joins owned work at final teardown; UI hosts retire readers before dropping them.
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Checks cancellation at every read instead of waiting for a full payload to finish.
struct CancelledReader<'a, R> {
    reader: R,
    cancelled: &'a AtomicBool,
}

impl<R: Read> Read for CancelledReader<'_, R> {
    /// Refuses further work once its owner has cancelled this reader.
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(io::Error::other("shader result decoding cancelled"));
        }
        self.reader.read(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// Holds a read until its test releases it, independently of scheduler timing.
    struct HeldReader {
        entered: Arc<AtomicBool>,
        release: mpsc::Receiver<()>,
    }

    impl Read for HeldReader {
        /// Announces a pending read, then waits for the test's explicit release.
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            self.entered.store(true, Ordering::Release);
            let _ = self.release.recv();
            Ok(0)
        }
    }

    /// Releases a held read even when a test assertion unwinds.
    struct ReleaseOnDrop(mpsc::Sender<()>);

    impl Drop for ReleaseOnDrop {
        /// Unblocks the reader before the decoder is joined.
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    #[test]
    fn polling_does_not_wait_for_a_pending_payload_read() {
        let entered = Arc::new(AtomicBool::new(false));
        let (send, receive) = mpsc::channel();
        let mut decoder = Decoder::start(HeldReader {
            entered: Arc::clone(&entered),
            release: receive,
        })
        .unwrap();
        let release = ReleaseOnDrop(send);
        test_time::eventually("payload reader entered", || entered.load(Ordering::Acquire));
        assert!(decoder.poll().is_none());
        decoder.cancel();
        drop(release);
        let mut result = None;
        test_time::eventually("cancelled payload reader retired", || {
            result = decoder.poll();
            result.is_some()
        });
        assert!(result.unwrap().is_err());
    }
}

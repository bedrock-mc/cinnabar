//! One disk owner saves option snapshots without blocking the frame thread.

use shaderpack::{OptionValues, PackOption};
use std::{
    io,
    path::PathBuf,
    sync::{Mutex, mpsc},
    thread::{self, JoinHandle},
};

/// An immutable option-file replacement.
pub(super) struct Request {
    pub path: PathBuf,
    pub options: Vec<PackOption>,
    pub values: OptionValues,
}

/// A bounded save mailbox and its completion channel.
pub(super) struct Writer {
    requests: Option<mpsc::SyncSender<Request>>,
    results: Mutex<mpsc::Receiver<Result<(), String>>>,
    worker: Option<JoinHandle<()>>,
}

impl Writer {
    /// Starts the sole writer; the host keeps at most one unacknowledged save.
    pub fn new() -> io::Result<Self> {
        let (send, receive) = mpsc::sync_channel::<Request>(1);
        let (complete, results) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("shader-options-writer".into())
            .spawn(move || {
                while let Ok(request) = receive.recv() {
                    let result = shaderpack::write_option_file(
                        &request.path,
                        &request.options,
                        &request.values,
                    )
                    .map_err(|error| format!("{}: {error}", request.path.display()));
                    if complete.send(result).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests: Some(send),
            results: Mutex::new(results),
            worker: Some(worker),
        })
    }

    /// Queues a snapshot without waiting for filesystem work.
    pub fn submit(&self, request: Request) -> Result<(), String> {
        self.requests
            .as_ref()
            .ok_or("the options writer stopped")?
            .try_send(request)
            .map_err(|error| error.to_string())
    }

    /// Takes a completed save, reporting a stopped worker as a retryable failure.
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        match self
            .results
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .try_recv()
        {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("the options writer stopped".into())),
        }
    }
}

impl Drop for Writer {
    /// Flushes accepted work when the host shuts down.
    fn drop(&mut self) {
        drop(self.requests.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

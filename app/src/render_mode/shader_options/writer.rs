//! Owns one accepted option save until its disk operation has completed.

use shaderpack::{OptionValues, PackOption};
use std::{
    io,
    path::PathBuf,
    thread::{self, JoinHandle},
};

/// An immutable option-file replacement.
pub(super) struct Request {
    pub path: PathBuf,
    pub options: Vec<PackOption>,
    pub values: OptionValues,
}

/// One save worker, with no idle thread after its result has been acknowledged.
pub(super) struct Writer {
    worker: Option<JoinHandle<Result<(), String>>>,
}

impl Writer {
    /// Starts one replacement; the worker exits as soon as this disk operation finishes.
    pub fn start(request: Request) -> io::Result<Self> {
        Self::spawn(move || {
            shaderpack::write_option_file(&request.path, &request.options, &request.values)
                .map_err(|error| format!("{}: {error}", request.path.display()))
        })
    }

    /// Owns one background operation whose completion can be polled without waiting.
    pub(super) fn spawn(
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> io::Result<Self> {
        let worker = thread::Builder::new()
            .name("shader-options-writer".into())
            .spawn(work)?;
        Ok(Self {
            worker: Some(worker),
        })
    }

    /// Takes a finished save; an unfinished operation always returns immediately.
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        if !self.worker.as_ref()?.is_finished() {
            return None;
        }
        Some(
            self.worker
                .take()
                .unwrap()
                .join()
                .unwrap_or_else(|_| Err("the options writer stopped".into())),
        )
    }
}

impl Drop for Writer {
    /// Flushes accepted work at host teardown; normal polling retires it after completion.
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

//! One cancellable account catalog read on the application's background runtime.

use super::{CatalogListing, NativeAccount};
use crossbeam_channel::{Receiver, bounded};
use std::{io, sync::Arc};
use tokio::sync::oneshot;

/// Owns one request's cancellation. Dropping it never waits on HTTP or credential persistence.
pub struct CatalogWorker {
    result: Receiver<io::Result<CatalogListing>>,
    cancel: Option<oneshot::Sender<()>>,
    worker: std::thread::JoinHandle<()>,
}

impl std::fmt::Debug for CatalogWorker {
    /// Omits account identities, paths and SDK state.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CatalogWorker")
    }
}

impl CatalogWorker {
    /// Starts one request; the shared account serializes cleanup against a subsequent connection prepare.
    pub fn start(
        account: Arc<NativeAccount>,
        runtime: tokio::runtime::Handle,
        tasks: tokio_util::task::TaskTracker,
    ) -> io::Result<Self> {
        Self::start_read(move || account.list_catalog(), runtime, tasks)
    }

    /// Runs a controlled read through the actual thread, cancellation and completion owner.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn start_test_read(
        read: impl std::future::Future<Output = io::Result<CatalogListing>> + Send + 'static,
        runtime: tokio::runtime::Handle,
        tasks: tokio_util::task::TaskTracker,
    ) -> io::Result<Self> {
        Self::start_read(move || read, runtime, tasks)
    }

    /// Creates the read on its original OS worker; the read itself need not be Send.
    fn start_read<F, R>(
        read: R,
        runtime: tokio::runtime::Handle,
        tasks: tokio_util::task::TaskTracker,
    ) -> io::Result<Self>
    where
        F: std::future::Future<Output = io::Result<CatalogListing>>,
        R: FnOnce() -> F + Send + 'static,
    {
        let (sender, result) = bounded(1);
        let (cancel, mut cancelled) = oneshot::channel();
        let worker = std::thread::Builder::new()
            .name("native-catalog".into())
            .spawn(crate::worker::tracked(
                move || {
                    let result = runtime.block_on(async {
                        tokio::select! { biased;
                            _ = &mut cancelled => None,
                            result = read() => Some(result),
                        }
                    });
                    if let Some(result) = result {
                        let _ = sender.try_send(result);
                    }
                },
                &tasks,
            ))?;
        Ok(Self {
            result,
            cancel: Some(cancel),
            worker,
        })
    }

    /// Signals only this actual request; its Operation still owns protected persistence.
    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }

    /// Observes actual thread completion without joining or waiting on the frame.
    pub fn is_finished(&self) -> bool {
        self.worker.is_finished()
    }

    /// Reads a completed list without waiting from the frame.
    pub fn poll(&mut self) -> Option<io::Result<CatalogListing>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
            Err(crossbeam_channel::TryRecvError::Disconnected) => Some(Err(io::Error::other(
                "native account catalog worker stopped",
            ))),
        }
    }
}

impl Drop for CatalogWorker {
    /// Cancellation drops only the HTTP future; Operation retains acquired refresh until protected persistence.
    fn drop(&mut self) {
        self.cancel();
    }
}

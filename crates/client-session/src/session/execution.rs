//! Host execution choices for the shared session task.

use bridge::tasks::Timers;
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

/// A session reader movable between native runtime workers.
#[cfg(not(target_arch = "wasm32"))]
pub trait SessionRead: bridge::FrameRead + Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<R: bridge::FrameRead + Send> SessionRead for R {}
/// A browser session reader confined to its owning event loop.
#[cfg(target_arch = "wasm32")]
pub trait SessionRead: bridge::FrameRead {}
#[cfg(target_arch = "wasm32")]
impl<R: bridge::FrameRead> SessionRead for R {}

#[cfg(not(target_arch = "wasm32"))]
pub(super) trait SessionOwner: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> SessionOwner for T {}
#[cfg(target_arch = "wasm32")]
pub(super) trait SessionOwner {}
#[cfg(target_arch = "wasm32")]
impl<T> SessionOwner for T {}

#[cfg(not(target_arch = "wasm32"))]
pub(super) trait SessionFuture: Future + Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Future + Send> SessionFuture for T {}
#[cfg(target_arch = "wasm32")]
pub(super) trait SessionFuture: Future {}
#[cfg(target_arch = "wasm32")]
impl<T: Future> SessionFuture for T {}

/// Selects outbound execution and deadlines once per session at composition.
pub trait NetworkExecutor: Clone {
    /// Cancels its outbound task when dropped.
    type OutboundTask;

    /// Spawns the finite outbound future; its guard must cancel it on every terminal path.
    fn spawn_outbound(&self, work: impl Future<Output = ()> + Send + 'static)
    -> Self::OutboundTask;

    /// Registers deadlines with the host's timer driver.
    fn timers(&self) -> Timers;

    /// Observes applied pack lifetime when this host offers pack reporting.
    fn report_packs(&self, _applied: bool) -> impl Future<Output = ()> {
        std::future::ready(())
    }
}

/// Native execution retains Tokio tasks, timers and the core's pack-application observer.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Default)]
pub(super) struct NativeNetworkExecutor {
    pub(super) socket_dir: PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl NetworkExecutor for NativeNetworkExecutor {
    type OutboundTask = NativeOutboundTask;

    fn spawn_outbound(
        &self,
        work: impl Future<Output = ()> + Send + 'static,
    ) -> Self::OutboundTask {
        NativeOutboundTask(tokio::spawn(work))
    }

    fn timers(&self) -> Timers {
        Timers::tokio()
    }

    async fn report_packs(&self, applied: bool) {
        protocol::report_pack_application(&self.socket_dir, applied).await;
    }
}

/// Aborts the native outbound task when its pump stops.
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct NativeOutboundTask(tokio::task::JoinHandle<()>);
#[cfg(not(target_arch = "wasm32"))]
impl Drop for NativeOutboundTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

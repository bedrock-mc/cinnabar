//! Host execution choices for the shared session task.

use bridge::tasks::Timers;
use std::future::Future;

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

/// Native execution retains the original Tokio tasks and timer driver.
#[cfg(all(not(target_arch = "wasm32"), any(feature = "native-networking", test)))]
#[derive(Clone, Default)]
pub(super) struct NativeNetworkExecutor {
    pub(super) tasks: tokio_util::task::TaskTracker,
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "native-networking", test)))]
impl NetworkExecutor for NativeNetworkExecutor {
    type OutboundTask = NativeOutboundTask;

    fn spawn_outbound(
        &self,
        work: impl Future<Output = ()> + Send + 'static,
    ) -> Self::OutboundTask {
        tracing::info!(shutdown_phase = "session_outbound_spawn_begin");
        NativeOutboundTask(tokio::spawn(self.tasks.track_future(work)))
    }

    fn timers(&self) -> Timers {
        Timers::tokio()
    }
}

/// Aborts the native outbound task when its pump stops.
#[cfg(all(not(target_arch = "wasm32"), any(feature = "native-networking", test)))]
pub(super) struct NativeOutboundTask(tokio::task::JoinHandle<()>);
#[cfg(all(not(target_arch = "wasm32"), any(feature = "native-networking", test)))]
impl Drop for NativeOutboundTask {
    fn drop(&mut self) {
        tracing::info!(
            shutdown_phase = "session_outbound_abort_requested",
            task_finished = self.0.is_finished()
        );
        self.0.abort();
    }
}

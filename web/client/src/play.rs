//! A play stream leased from the page's retained relay session.

use futures::{FutureExt, channel::oneshot};
use std::{future::Future, pin::Pin};
use web_transport::CloseReason;

/// A request to open one play stream on the already authenticated control session.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) struct Request<S> {
    pub reply: oneshot::Sender<Result<Lease<S>, CloseReason>>,
}

/// Keeps the control generation alive until the game's play task releases it.
pub struct Lease<S> {
    pub(crate) io: Io<S>,
    pub(crate) ended: oneshot::Sender<()>,
    pub(crate) cancelled: oneshot::Receiver<()>,
}

/// Prepared bridge I/O; the host owns writer execution and cancellation.
pub struct Io<S> {
    pub join: u64,
    pub reader: bridge::FramedReader<tokio::io::ReadHalf<S>>,
    pub frames: bridge::FrameQueue,
    pub writer: Pin<Box<dyn Future<Output = ()>>>,
    pub packs: Pin<Box<dyn Future<Output = Result<Option<S>, CloseReason>>>>,
}

impl<S> Lease<S> {
    /// Separates ordered I/O from the lifetime guard retained by the game's session task.
    pub fn into_parts(self) -> (Io<S>, Guard) {
        (
            self.io,
            Guard {
                _ended: self.ended,
                cancelled: self.cancelled,
            },
        )
    }
}

/// Dropping this guard retires the play generation before another join can open.
pub struct Guard {
    _ended: oneshot::Sender<()>,
    cancelled: oneshot::Receiver<()>,
}

impl Guard {
    /// Completes when navigation, sign-out or transport loss invalidates this play stream.
    pub async fn cancelled(&mut self) {
        let _ = (&mut self.cancelled).await;
    }

    /// Drives I/O until the session finishes (true) or its generation is cancelled (false).
    /// A stopped writer leaves terminal recovery to the session and its existing deadlines.
    pub async fn run(
        &mut self,
        session: impl Future<Output = ()>,
        writer: impl Future<Output = ()>,
    ) -> bool {
        let session = session.fuse();
        let writer = writer.fuse();
        let cancelled = self.cancelled().fuse();
        futures::pin_mut!(session, writer, cancelled);
        loop {
            futures::select_biased! {
                _ = cancelled => return false,
                _ = session => return true,
                _ = writer => {},
            }
        }
    }
}

/// Control-side ownership; its drop wakes the game's cancellation future.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) struct Active {
    pub finished: oneshot::Receiver<()>,
    pub cancel: Option<oneshot::Sender<()>>,
}

#[cfg(test)]
mod tests;

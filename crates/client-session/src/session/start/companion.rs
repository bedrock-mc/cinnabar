//! Application-owned services polled beside the existing login/bootstrap/play future.

use crate::session::TerminalOwner;
use std::{
    future::{Future, poll_fn},
    io,
    task::Poll,
};
use tokio::sync::watch;

/// One application service lifetime on the network worker's existing runtime.
/// Keep cancellation-sensitive owners in self; close must be bounded and retain unresolved cleanup.
pub trait SessionCompanion: Send + 'static {
    /// Prepares optional native credentials while retaining its service owner across cancellation.
    #[cfg(feature = "native-networking")]
    fn prepare(
        &mut self,
    ) -> impl Future<
        Output = io::Result<
            Option<(
                protocol::NativeConnectionTarget,
                protocol::NativeAuthentication,
            )>,
        >,
    > + Send {
        std::future::ready(Ok(None))
    }

    /// Advances one finite service event; cancellation must preserve progress in self.
    fn drive(&mut self) -> impl Future<Output = io::Result<()>> + Send {
        std::future::pending()
    }

    /// Explicitly ends owned membership after every network terminal or canceled preparation.
    /// Preserve unresolved membership in a bounded application owner before returning.
    fn close(&mut self) -> impl Future<Output = ()> + Send {
        std::future::ready(())
    }
}

impl SessionCompanion for () {}

#[cfg(test)]
mod terminal_tests;

/// Keeps services alive through queue backpressure; a retained network terminal owns ordered publication.
/// An alive stalled consumer may delay publication and cleanup until capacity or external cancellation.
pub(super) async fn run_with_companion(
    work: impl Future<Output = ()>,
    companion: &mut impl SessionCompanion,
    shutdown: &watch::Sender<bool>,
    terminal: &TerminalOwner,
) -> io::Result<()> {
    tokio::pin!(work);
    loop {
        let event = {
            // Preserve the actual HTTP/credential future across unrelated network wakes.
            let drive = companion.drive();
            tokio::pin!(drive);
            poll_fn(|cx| {
                if work.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(None);
                }
                drive.as_mut().poll(cx).map(Some)
            })
            .await
        };
        match event {
            None => return Ok(()),
            Some(Ok(())) => {}
            Some(Err(error)) => {
                // An already retained network boundary owns publication even while its FIFO is full.
                // The atomic claim also arbitrates errors observed by the separate outbound task.
                if *shutdown.borrow() || !terminal.companion() {
                    work.await;
                    return Ok(());
                }
                shutdown.send_replace(true);
                work.await;
                return Err(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::mpsc;

    struct Events {
        events: mpsc::Receiver<io::Result<()>>,
        started: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    }
    struct PendingRequest(Arc<AtomicUsize>);
    impl Drop for PendingRequest {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl SessionCompanion for Events {
        async fn drive(&mut self) -> io::Result<()> {
            self.started.fetch_add(1, Ordering::SeqCst);
            let _request = PendingRequest(Arc::clone(&self.dropped));
            self.events
                .recv()
                .await
                .unwrap_or_else(|| Err(io::Error::other("fixture closed")))
        }
    }

    /// Creates only bounded application signals, without sockets, clocks or background workers.
    fn events() -> (Events, mpsc::Sender<io::Result<()>>) {
        let (sender, receiver) = mpsc::channel(2);
        (
            Events {
                events: receiver,
                started: Arc::default(),
                dropped: Arc::default(),
            },
            sender,
        )
    }

    #[tokio::test]
    async fn full_bootstrap_queue_keeps_one_inflight_service_request_across_network_wakes() {
        let (mut companion, events) = events();
        let started = Arc::clone(&companion.started);
        let dropped = Arc::clone(&companion.dropped);
        let (output, mut queue) = mpsc::channel(1);
        let work = async {
            output.send(1).await.unwrap();
            output.send(2).await.unwrap();
        };
        let (shutdown, _) = watch::channel(false);
        let terminal = TerminalOwner::default();
        let mut running = Box::pin(run_with_companion(
            work,
            &mut companion,
            &shutdown,
            &terminal,
        ));
        for _ in 0..3 {
            assert!(
                poll_fn(|cx| Poll::Ready(running.as_mut().poll(cx)))
                    .await
                    .is_pending()
            );
        }
        assert_eq!(started.load(Ordering::SeqCst), 1);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        events.send(Ok(())).await.unwrap();
        assert!(
            poll_fn(|cx| Poll::Ready(running.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(queue.recv().await, Some(1));
        running.await.unwrap();
        assert_eq!(queue.recv().await, Some(2));
    }

    #[tokio::test]
    async fn accepted_network_terminal_wins_over_a_simultaneously_ready_service_failure() {
        let (mut companion, events) = events();
        let started = Arc::clone(&companion.started);
        events
            .send(Err(io::Error::other("service ended")))
            .await
            .unwrap();
        let (shutdown, _) = watch::channel(false);
        run_with_companion(
            std::future::ready(()),
            &mut companion,
            &shutdown,
            &TerminalOwner::default(),
        )
        .await
        .unwrap();
        assert_eq!(started.load(Ordering::SeqCst), 0);
        assert!(!*shutdown.borrow());
    }

    #[tokio::test]
    async fn service_failure_waits_for_the_networks_ordered_shutdown_before_returning() {
        let (mut companion, events) = events();
        events
            .send(Err(io::Error::other("service ended")))
            .await
            .unwrap();
        let (shutdown, mut cancelled) = watch::channel(false);
        let (terminal, mut output) = mpsc::channel(1);
        let work = async {
            cancelled.changed().await.unwrap();
            terminal.send("network stopped").await.unwrap();
        };
        assert!(
            run_with_companion(work, &mut companion, &shutdown, &TerminalOwner::default())
                .await
                .is_err()
        );
        assert_eq!(output.recv().await, Some("network stopped"));
    }
}

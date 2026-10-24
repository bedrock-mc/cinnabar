//! A bounded set of original peer continuations, polled only by the host's existing caller.

use super::super::{
    ClosingHostedPeer, NativeHostedPeer, NativeHostedWorld,
    directory::HostDirectory,
    serve::play::{Step, cleanup_deadline},
};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use std::{
    io,
    sync::{Arc, Weak},
    time::Instant,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

struct Ready {
    peer: NativeHostedPeer,
    step: io::Result<Option<Step>>,
    deadline: Option<Instant>,
}

/// Each admitted peer exists in exactly one pending, ready or closing owner until retirement.
#[derive(Default)]
pub(in crate::native_account::hosted_world) struct Peers {
    pending: FuturesUnordered<BoxFuture<'static, Ready>>,
    ready: Vec<Ready>,
    closing: Vec<ClosingHostedPeer>,
    occupied: usize,
    limit: usize,
    wake: Arc<Notify>,
    stop: CancellationToken,
    deadline: Option<Instant>,
    turn_controls: Vec<Weak<nethernet_tokio::SessionSender>>,
}

impl Peers {
    /// Uses only the capacity admitted by the actual opening and retained accepting transport.
    pub(in crate::native_account::hosted_world) fn new(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    /// Counts initialized peers through their actual closing owner, including a checked-out close.
    pub(in crate::native_account::hosted_world) fn occupied(&self) -> usize {
        self.occupied
    }

    /// Captures a coalesced state wake; the notification carries neither authority nor packet data.
    pub(in crate::native_account::hosted_world) fn wake(&self) -> Arc<Notify> {
        Arc::clone(&self.wake)
    }

    /// Wakes the caller after the original service owner applies and acknowledges a changed snapshot.
    pub(in crate::native_account::hosted_world) fn changed(&self) {
        self.wake.notify_one();
    }

    /// Retains a peer only after actual typed Initialized and its original backend handoff.
    pub(in crate::native_account::hosted_world) fn admit(
        &mut self,
        peer: NativeHostedPeer,
    ) -> io::Result<()> {
        if self.deadline.is_some() || self.occupied >= self.limit {
            return Err(unavailable());
        }
        self.turn_controls
            .retain(|control| control.strong_count() != 0);
        self.turn_controls.push(Arc::downgrade(peer.turn_control()));
        self.occupied += 1;
        self.schedule(peer);
        Ok(())
    }

    /// Snapshots only actual initialized peers; entering close drops their original strong owner.
    /// Weak metadata is bounded by admitted capacity and never keeps a retired association alive.
    pub(super) fn turn_controls(&mut self) -> Vec<Arc<nethernet_tokio::SessionSender>> {
        self.turn_controls
            .retain(|control| control.strong_count() != 0);
        self.turn_controls
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }

    /// Keeps the actual IO future, partial frames, granted permit and first timer across other wakes.
    fn schedule(&mut self, mut peer: NativeHostedPeer) {
        let stop = self.stop.clone();
        let original = peer.deadline();
        self.pending.push(
            async move {
                let step = tokio::select! { biased;
                    result = async {
                        match original {
                            Some(deadline) => super::bounded(deadline, peer.next_step()).await?,
                            None => peer.next_step().await,
                        }
                    } => result.map(Some),
                    _ = stop.cancelled() => Ok(None),
                };
                let deadline = match (original, peer.deadline()) {
                    (Some(first), Some(next)) => Some(first.min(next)),
                    (first, next) => first.or(next),
                };
                Ready {
                    peer,
                    step,
                    deadline,
                }
            }
            .boxed(),
        );
    }

    /// Moves one completed IO continuation without consuming another peer's retained result.
    pub(in crate::native_account::hosted_world) async fn next(&mut self) {
        if self.pending.is_empty() {
            std::future::pending::<()>().await;
        }
        if let Some(ready) = self.pending.next().await {
            self.ready.push(ready);
        }
    }

    /// Keeps the original completed operation cutoff alive while its fresh authority check waits.
    pub(in crate::native_account::hosted_world) fn ready_deadline(&self) -> Option<Instant> {
        self.ready.iter().filter_map(|ready| ready.deadline).min()
    }

    /// Ready terminal owners can retire during account contention; ordinary admission keeps waiting.
    pub(in crate::native_account::hosted_world) fn can_advance(&self, busy: bool) -> bool {
        self.ready.iter().any(|ready| {
            !busy
                || matches!(ready.step, Ok(Some(Step::Ended)) | Ok(None) | Err(_))
                || ready
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
        })
    }

    /// Applies ready effects only at the sole current directory's synchronous admission boundary.
    /// Busy account/document state retains the exact continuation and original effect deadline.
    pub(in crate::native_account::hosted_world) fn advance(&mut self, directory: &HostDirectory) {
        let mut index = 0;
        while index < self.ready.len() {
            let ready = &self.ready[index];
            let allowed = match &ready.step {
                Ok(Some(Step::Ended)) | Ok(None) | Err(_) => Ok(()),
                _ if ready
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline) =>
                {
                    Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "native hosted peer operation expired",
                    ))
                }
                _ => directory.recheck_approval(ready.peer.approval()),
            };
            if allowed
                .as_ref()
                .is_err_and(|error| error.kind() == io::ErrorKind::WouldBlock)
            {
                index += 1;
                continue;
            }
            let Ready { mut peer, step, .. } = self.ready.swap_remove(index);
            let result = allowed.and_then(|()| match step {
                Ok(Some(step)) => peer.apply(step),
                Ok(None) => Ok(true),
                Err(error) => Err(error),
            });
            match result {
                Ok(false) => self.schedule(peer),
                ended => {
                    let (closing, outcome) = peer.into_closing(
                        ended.map(|_| ()),
                        self.deadline.unwrap_or_else(cleanup_deadline),
                    );
                    if let Err(error) = outcome {
                        tracing::debug!(kind = ?error.kind(), "native hosted peer ended");
                    }
                    self.closing.push(closing);
                    self.wake.notify_one();
                }
            }
        }
    }

    /// Checks out one original closing owner without releasing its occupied capacity.
    pub(in crate::native_account::hosted_world) fn take_closing(
        &mut self,
    ) -> Option<ClosingHostedPeer> {
        self.closing.pop()
    }

    /// Returns a suspended/failed close to the same bounded set for explicit retry or host retirement.
    pub(super) fn retain_closing(&mut self, closing: ClosingHostedPeer) {
        self.closing.push(closing);
    }

    /// Releases only a peer whose actual consuming close has completed.
    pub(in crate::native_account::hosted_world) fn retired(&mut self) {
        self.occupied = self
            .occupied
            .checked_sub(1)
            .expect("original occupied peer");
    }

    /// Seals new admission and wakes the same retained IO futures under one caller cutoff.
    pub(super) fn begin_close(&mut self, deadline: Instant) {
        self.deadline.get_or_insert(deadline);
        self.stop.cancel();
    }

    /// Returns every original peer from its retained future, then attempts their same bounded drain.
    pub(super) async fn finish_close(&mut self, world: &mut NativeHostedWorld) -> io::Result<()> {
        let deadline = self.deadline.ok_or_else(unavailable)?;
        while !self.pending.is_empty() {
            self.next().await;
        }
        for Ready { peer, step, .. } in self.ready.drain(..) {
            let result = step.map(|_| ());
            self.closing.push(peer.into_closing(result, deadline).0);
        }
        let mut result = Ok(());
        let mut index = 0;
        while index < self.closing.len() {
            let closed = self.closing[index].finish_close(world).await;
            if self.closing[index].is_finished() {
                drop(self.closing.swap_remove(index));
                self.retired();
            } else {
                index += 1;
            }
            if result.is_ok() {
                result = closed;
            }
        }
        result
    }
}

/// Keeps peer/account/world details out of capacity and lifecycle diagnostics.
fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "native hosted peer set is unavailable",
    )
}

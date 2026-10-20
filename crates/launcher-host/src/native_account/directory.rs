//! Shared account scope and caller-driven directory control progress.

use super::{Cached, NativeAccount, Operation};
use bedrock_services::xbox::{mpsd, rta};
use rta::websocket::{RtaSocket, SocketError};
use sha2::{Digest, Sha256};
use std::{
    future::{Future, poll_fn},
    io,
    task::Poll,
    time::Instant,
};

#[derive(Clone)]
pub(super) struct Scope {
    pub(super) generation: String,
    pub(super) xuid: String,
}

impl Scope {
    /// Captures an already verified selection without frame-thread files, acquisition or credentials.
    pub(super) fn selected(account: &NativeAccount) -> io::Result<Self> {
        let state = account
            .state
            .try_lock()
            .map_err(|_| invalid("native account operation is active"))?;
        if state.busy {
            return Err(invalid("native account operation is active"));
        }
        Self::from_cached(
            state
                .cached
                .as_ref()
                .ok_or_else(|| invalid("native account is not verified"))?,
        )
    }

    /// Captures only the current verified application identity, without credential duplication.
    pub(super) fn from_operation(operation: &Operation) -> io::Result<Self> {
        let cached = operation
            .cached
            .as_ref()
            .ok_or_else(|| invalid("friend account is unavailable"))?;
        Self::from_cached(cached)
    }

    /// Reads only a current cached account generation and its verified XUID.
    fn from_cached(cached: &Cached) -> io::Result<Self> {
        let identity = cached
            .identity
            .as_ref()
            .filter(|identity| identity.current_credential)
            .ok_or_else(|| invalid("friend account is not verified"))?;
        Ok(Self {
            generation: cached.generation.clone(),
            xuid: identity.xuid.clone(),
        })
    }

    /// Rejects account replacement even when the user signs into the same Xbox identity again.
    pub(super) fn check(&self, operation: &Operation) -> io::Result<()> {
        let current = Self::from_operation(operation)?;
        if current.generation != self.generation || current.xuid != self.xuid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "friend-world account generation changed",
            ));
        }
        Ok(())
    }

    /// Supplies RTA's bounded generation identity; exact original generation equality is checked separately.
    pub(super) fn rta_generation(&self) -> io::Result<rta::Generation> {
        let digest = Sha256::digest(self.generation.as_bytes());
        let value = u64::from_be_bytes(digest[..8].try_into().expect("SHA-256 prefix"));
        rta::Generation::new(value.max(1)).map_err(|_| invalid("invalid friend account generation"))
    }
}

/// Polls RTA controls concurrently with one borrowed service operation, preserving its owner on cancellation.
pub(super) async fn service<T>(
    socket: &mut RtaSocket,
    subscription: rta::SubscriptionId,
    reference: &mpsd::Reference,
    changed: &mut bool,
    deadline: Instant,
    future: impl Future<Output = T>,
) -> io::Result<T> {
    check_deadline(deadline)?;
    tokio::pin!(future);
    let timeout = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
    tokio::pin!(timeout);
    poll_fn(|cx| {
        if Instant::now() >= deadline || timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(timed_out()));
        }
        let event = {
            let drive = socket.drive();
            tokio::pin!(drive);
            drive.as_mut().poll(cx)
        };
        let progressed = match event {
            Poll::Ready(Ok(event)) => {
                if let Err(error) = event_changed(event, subscription, reference, changed) {
                    return Poll::Ready(Err(error));
                }
                true
            }
            Poll::Ready(Err(error)) => return Poll::Ready(Err(socket_error(error))),
            Poll::Pending => false,
        };
        if let Poll::Ready(value) = future.as_mut().poll(cx) {
            return Poll::Ready(Ok(value));
        }
        if progressed {
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    })
    .await
}

/// Handles only this directory subscription's revision hints; payload snapshots remain HTTP-owned.
pub(super) fn event_changed(
    event: rta::Event,
    subscription: rta::SubscriptionId,
    reference: &mpsd::Reference,
    changed: &mut bool,
) -> io::Result<()> {
    match event {
        rta::Event::SessionsChanged {
            subscription: received,
            changes,
        } if received == subscription => {
            *changed |= changes.iter().any(|change| change.is_for(reference))
        }
        rta::Event::Resync { subscriptions } => *changed |= subscriptions.contains(&subscription),
        rta::Event::Failed { .. } | rta::Event::Disconnected | rta::Event::Unsubscribed { .. } => {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "friend-world subscription ended",
            ));
        }
        _ => {}
    }
    Ok(())
}

/// Gives the original absolute deadline priority before polling a possibly ready I/O future.
pub(super) async fn bounded<T>(
    deadline: Instant,
    future: impl Future<Output = T>,
) -> io::Result<T> {
    check_deadline(deadline)?;
    tokio::select! { biased;
        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => Err(timed_out()),
        value = future => Ok(value),
    }
}

/// Rejects expired phases before starting account or network I/O.
pub(super) fn check_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(timed_out())
    } else {
        Ok(())
    }
}

/// Keeps service bodies, handles and title metadata out of diagnostics.
pub(super) fn directory_error(_: mpsd::Error) -> io::Error {
    io::Error::other("friend-world directory request failed")
}
/// Keeps protected upgrade details out of diagnostics.
pub(super) fn rta_error(_: rta::Error) -> io::Error {
    io::Error::other("friend-world subscription failed")
}
/// Keeps WebSocket responses and close reasons out of diagnostics.
pub(super) fn socket_error(_: SocketError) -> io::Error {
    io::Error::other("friend-world realtime connection failed")
}
/// Reports only application-owned static validation reasons.
pub(super) fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}
/// Reports the retained deadline rather than exposing transport diagnostics.
pub(super) fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "friend-world operation timed out")
}

/// Retains ambiguous membership instead of claiming that a canceled write could not have applied.
pub(super) fn cleanup_pending() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "friend-world membership remains uncertain; retry cleanup",
    )
}

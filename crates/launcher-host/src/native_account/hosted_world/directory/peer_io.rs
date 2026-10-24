//! Keeps original peer IO pinned across directory refresh and physical realtime recovery.

use super::super::active::peers::Peers;
use super::*;
use crate::native_account::{ACQUISITION_TIMEOUT, LEASE_RETRY, directory::timed_out};

impl HostDirectory {
    /// Keeps one actual peer future pinned while the sole RTA owner refreshes complete snapshots.
    /// A refresh is completed under its first deadline before another application result is exposed.
    pub(in crate::native_account::hosted_world) async fn peer_io<T>(
        &mut self,
        deadline: Option<Instant>,
        future: impl std::future::Future<Output = io::Result<T>>,
        mut peers: Option<&mut Peers>,
    ) -> io::Result<T> {
        tokio::pin!(future);
        let mut completed = None;
        loop {
            self.active()?;
            if let Some(deadline) = deadline {
                check_deadline(deadline)?;
            }
            // A terminal already observed by the socket can precede its retained notification.
            // Consume that original FIFO before the live-binding admission check below.
            while self.recovery.is_none()
                && self
                    .socket
                    .as_ref()
                    .is_some_and(|socket| !socket.connection().is_connected())
            {
                let cutoff = self.control_deadline(deadline);
                let event =
                    bounded(cutoff, self.socket.as_mut().expect("host socket").drive()).await?;
                let result = event.map_err(socket_error).and_then(|event| {
                    event_changed(
                        event,
                        self.subscription
                            .ok_or_else(|| invalid("host subscription is unavailable"))?,
                        &self.reference,
                        &mut self.changed,
                    )
                });
                if let Err(error) = result {
                    self.retain_interruption(error)?;
                }
            }
            self.changed |= self.session.as_ref().is_some_and(mpsd::Session::needs_sync);
            let mut serviced = false;
            if self.changed || self.recovery.is_some() {
                let refresh_deadline = self.control_deadline(deadline);
                self.refresh_control(refresh_deadline).await?;
                serviced = true;
            }
            if peers.is_some()
                && self.advertisement.is_some()
                && self
                    .approvals
                    .as_ref()
                    .is_some_and(|approvals| !approvals.acknowledged())
            {
                let first = *self
                    .refresh_deadline
                    .get_or_insert_with(|| Instant::now() + ACQUISITION_TIMEOUT);
                let cutoff = deadline.map_or(first, |deadline| first.min(deadline));
                self.publish_approvals(cutoff).await?;
                serviced = true;
            }
            if serviced {
                self.refresh_deadline = None;
                if let Some(peers) = &peers {
                    peers.changed();
                }
            }
            self.validate_session()?;
            let busy = matches!(self.account_authority()?, Authority::Busy);
            let peer_deadline = peers.as_ref().and_then(|peers| peers.ready_deadline());
            let advance = peers.as_ref().is_some_and(|peers| peers.can_advance(busy));
            let socket = self
                .socket
                .as_mut()
                .ok_or_else(|| invalid("host RTA is unavailable"))?;
            tokio::select! { biased;
                _ = async { match deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                    None => std::future::pending::<()>().await,
                }} => return Err(timed_out()),
                event = socket.drive() => {
                    let result = event.map_err(socket_error).and_then(|event| event_changed(
                        event, self.subscription.ok_or_else(|| invalid("host subscription is unavailable"))?,
                        &self.reference, &mut self.changed));
                    if let Err(error) = result { self.retain_interruption(error)?; }
                }
                _ = async { match peer_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
                    None => std::future::pending::<()>().await,
                }}, if !advance => {}
                _ = std::future::ready(()), if advance => {
                    peers.as_mut().expect("ready original peer set").advance(self);
                }
                _ = tokio::time::sleep(LEASE_RETRY), if busy || completed.is_some() => {}
                _ = std::future::ready(()), if !busy && completed.is_some() => {
                    match self.current() {
                        Ok(()) => {
                            if let Some(deadline) = deadline { check_deadline(deadline)?; }
                            return completed.take().expect("one actual completed IO result");
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                        Err(error) => return Err(error),
                    }
                }
                result = async {
                    tokio::select! {
                        result = &mut future, if completed.is_none() => Some(result),
                        _ = async { match &mut peers {
                            Some(peers) => peers.next().await,
                            None => std::future::pending::<()>().await,
                        }} => None,
                    }
                } => { if let Some(result) = result { completed = Some(result); } }
            }
        }
    }
}

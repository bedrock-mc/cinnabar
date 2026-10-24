//! Real original-member cancellation and cutoff ownership, without game or account substitutes.

use super::*;
use crate::native_account::test_support::friend::Fixture;
use bedrock_services::test_support::{HostedRebindHold, HostedRtaReconnect};
use std::{
    future::Future,
    task::{Context, Poll, Waker},
    time::Duration,
};

struct Original {
    fixture: Fixture,
    join: FriendJoin,
    recovery: HostedRtaReconnect,
    hold: HostedRebindHold,
    _root: tempfile::TempDir,
}

impl Original {
    /// Uses the actual selected account, complete catalogue and opaque row before real admission.
    async fn admitted(legacy: bool, applied: bool, deadline: Instant) -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("account.json");
        let device = root.path().join("device.json");
        let mut fixture = if legacy {
            Fixture::legacy(path, device, deadline).await
        } else {
            Fixture::messaging(path, device, deadline).await
        };
        let recovery = fixture.hold_rta_reconnect();
        let hold = if applied {
            fixture.hold_rebind_response()
        } else {
            fixture.hold_rebind_application()
        };
        let (account, _, _) = fixture.presentation().await.unwrap().into_parts();
        let listing = Arc::clone(&account).list_catalog().await.unwrap();
        let cards = match listing.into_current_cards(&account) {
            Ok(Some(cards)) => cards,
            _ => panic!("original acquired catalogue is not current"),
        };
        let selected = cards
            .friends
            .unwrap()
            .into_iter()
            .find_map(|world| world.selection())
            .expect("actual selectable service row");
        let mut join = selected.begin(deadline).unwrap();
        let ready = {
            let advancing = join.advance();
            tokio::pin!(advancing);
            tokio::select! {
                result = fixture.joined() => result.unwrap(),
                _ = &mut advancing => panic!("admission completed before actual nonce release"),
            }
            fixture.admit();
            matches!(advancing.await, Ok(JoinProgress::Ready(_)))
        };
        assert!(ready && join.current_admission().is_ok());
        Self {
            fixture,
            join,
            recovery,
            hold,
            _root: root,
        }
    }

    /// Cancels only the original drive borrow after the protected PUT reached its real hold.
    async fn cancel_rebind(&mut self, deadline: Instant) -> bool {
        self.recovery.disconnect();
        let driving = self.join.drive(deadline);
        tokio::pin!(driving);
        tokio::select! {
            _ = self.hold.requested() => true,
            _ = &mut driving => false,
        }
    }

    /// Captures only existing owner state; no binding or admission can be constructed here.
    fn pending(&self) -> bool {
        !self.join.subscribed
            && self.join.current_admission().is_err()
            && self
                .join
                .recovery
                .as_ref()
                .is_some_and(|recovery| recovery.candidate.is_some())
            && !self.join.closed
    }

    /// Releases the exact canceled stream and joins every original service owner before any oracle.
    async fn finish(mut self, deadline: Instant) -> (bool, bool) {
        self.hold.release();
        let canceled_socket = self.hold.retired().await.is_ok();
        let closed = self.join.close(deadline).await.is_ok();
        let local_closed = self.join.closed && self.join.session.is_none();
        self.join.stop_transport();
        self.recovery.stop();
        let observed = if closed {
            Some(self.fixture.finish().await)
        } else {
            self.fixture.abort().await;
            None
        };
        let facts = self.recovery.observation();
        let owners = canceled_socket
            && local_closed
            && observed.is_some_and(|observed| {
                observed.joins == 1
                    && observed.leaves == 1
                    && observed.nonce_published
                    && !observed.minted
                    && !observed.signaling_started
                    && observed.rta_retired
            })
            && facts.upgrades == 2
            && facts.acknowledgments == 2
            && facts.rebinds == 1
            && facts.first_retired
            && facts.synchronized;
        (closed, owners)
    }
}

/// Both canceled states retain the same member/candidate until actual synchronization and one leave.
#[tokio::test]
async fn friend_recovery_canceled_bind_retains_original_cleanup_until_actual_application() {
    let outcomes = tokio::time::timeout(Duration::from_secs(30), async {
        let mut outcomes = Vec::new();
        for legacy in [false, true] {
            for applied in [true, false] {
                let deadline = Instant::now() + Duration::from_secs(20);
                let mut original = Original::admitted(legacy, applied, deadline).await;
                let reached = original.cancel_rebind(deadline).await;
                let pending = original.pending();
                let old_read_retained = if !applied && reached {
                    let result = original.join.close(deadline).await;
                    matches!(result, Err(error) if error.kind() == io::ErrorKind::WouldBlock)
                        && original.pending()
                        && !original.join.closed
                } else {
                    applied
                };
                let (closed, owners) = original.finish(deadline).await;
                outcomes.push((
                    legacy,
                    applied,
                    reached,
                    pending,
                    old_read_retained,
                    closed,
                    owners,
                ));
            }
        }
        outcomes
    })
    .await
    .expect("finite original canceled member cleanup");
    assert!(
        outcomes
            .iter()
            .all(|(_, _, reached, pending, retained, closed, owners)| {
                *reached && *pending && *retained && *closed && *owners
            }),
        "canceled original binding escaped cleanup ownership: {outcomes:?}"
    );
}

/// A later borrow cannot renew the first real recovery cutoff while the original bind is uncertain.
#[tokio::test]
async fn friend_recovery_canceled_borrow_preserves_first_absolute_cutoff() {
    let outcomes = tokio::time::timeout(Duration::from_secs(60), async {
        let mut outcomes = Vec::new();
        for legacy in [false, true] {
            let cleanup_deadline = tokio::time::Instant::now().into_std() + Duration::from_secs(30);
            let mut original = Original::admitted(legacy, true, cleanup_deadline).await;
            let first = tokio::time::Instant::now().into_std() + Duration::from_secs(10);
            let reached = original.cancel_rebind(first).await;
            let retained =
                original.pending() && original.join.active_refresh_deadline == Some(first);
            // Retain the real checkout so the resumed borrow waits under its original timer,
            // rather than starting an unrelated TLS request that clock advancement would cancel.
            let checkout = original.join.account.begin().unwrap();
            let later = first + Duration::from_secs(10);
            // All real TLS/member work has reached its explicit physical hold before clock control.
            tokio::time::pause();
            let (pending, timed_out) = {
                let mut next = Box::pin(original.join.drive(later));
                match next.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
                    Poll::Ready(result) => (
                        false,
                        matches!(result, Err(error) if error.kind() == io::ErrorKind::TimedOut),
                    ),
                    Poll::Pending => {
                        let remaining = tokio::time::Instant::from_std(first)
                            .saturating_duration_since(tokio::time::Instant::now());
                        tokio::time::advance(remaining).await;
                        let result = next.await;
                        (
                            true,
                            matches!(result, Err(error) if error.kind() == io::ErrorKind::TimedOut)
                                && tokio::time::Instant::now().into_std() < later,
                        )
                    }
                }
            };
            tokio::time::resume();
            drop(checkout);
            let unchanged =
                original.join.active_refresh_deadline == Some(first) && original.pending();
            let (closed, owners) = original.finish(cleanup_deadline).await;
            outcomes.push((
                legacy, reached, retained, pending, timed_out, unchanged, closed, owners,
            ));
        }
        outcomes
    })
    .await
    .expect("finite original cutoff cleanup");
    assert!(
        outcomes.iter().all(
            |(_, reached, retained, pending, timed_out, unchanged, closed, owners)| {
                *reached && *retained && *pending && *timed_out && *unchanged && *closed && *owners
            }
        ),
        "a canceled recovery renewed its original cutoff: {outcomes:?}"
    );
}

//! One bounded application owner for active or unresolved friend membership.

use client_session::SessionCompanion;
use launcher_host::native_account::{FriendJoin, JoinProgress, NativeFriendSelection};
use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(15);

/// One slot is sufficient for the application's one active world; unresolved cleanup backpressures joins.
#[derive(Clone, Default)]
pub(crate) struct FriendCleanupOwner(Arc<Mutex<CleanupState>>);

#[derive(Default)]
struct CleanupState {
    reserved: bool,
    pending: Option<FriendJoin>,
}

struct CleanupLease {
    owner: FriendCleanupOwner,
    friend: Option<FriendJoin>,
}

impl FriendCleanupOwner {
    /// Observes the original reservation and uncertain-member slot for developer acceptance.
    #[cfg(feature = "developer-control")]
    pub(crate) fn observation(&self) -> (bool, bool) {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (state.reserved, state.pending.is_some())
    }

    /// Reserves the sole cleanup slot before any new remote membership is created.
    fn reserve(&self) -> io::Result<CleanupLease> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.reserved {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "previous friend-world cleanup is still active",
            ));
        }
        state.reserved = true;
        Ok(CleanupLease {
            owner: self.clone(),
            friend: state.pending.take(),
        })
    }

    /// Captures the original opaque selection and a reserved lifetime without authentication or frame I/O.
    pub(crate) fn request(
        &self,
        selection: NativeFriendSelection,
    ) -> io::Result<FriendConnectionRequest> {
        Ok(FriendConnectionRequest {
            selection,
            lease: self.reserve()?,
        })
    }
}

impl Drop for CleanupLease {
    /// Retains uncertain membership even when preparation, teardown or the owning worker is canceled.
    fn drop(&mut self) {
        if let Some(friend) = &mut self.friend {
            friend.stop_transport();
        }
        let mut state = self
            .owner
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending = self.friend.take();
        state.reserved = false;
    }
}

/// One original catalog selection plus its bounded cleanup reservation.
pub(crate) struct FriendConnectionRequest {
    selection: NativeFriendSelection,
    lease: CleanupLease,
}

impl FriendConnectionRequest {
    /// Moves the reserved owner into the existing network worker, without a task or runtime of its own.
    pub(super) fn into_companion(
        self,
        claims: serde_json::Value,
        random_id: protocol::ClientRandomId,
    ) -> FriendCompanion {
        FriendCompanion {
            selection: Some(self.selection),
            lease: self.lease,
            claims: Some(claims),
            random_id,
            admitted: false,
        }
    }
}

pub(super) struct FriendCompanion {
    selection: Option<NativeFriendSelection>,
    lease: CleanupLease,
    claims: Option<serde_json::Value>,
    random_id: protocol::ClientRandomId,
    admitted: bool,
}

impl FriendCompanion {
    /// Reconciles the retained original member before freeing the slot for another world.
    async fn cleanup(&mut self) -> io::Result<()> {
        if let Some(friend) = &mut self.lease.friend {
            friend.close(Instant::now() + CLEANUP_TIMEOUT).await?;
            self.lease.friend = None;
        }
        self.admitted = false;
        Ok(())
    }
}

impl SessionCompanion for FriendCompanion {
    /// Joins and prepares on the existing runtime; cancellation leaves ownership in self.
    async fn prepare(
        &mut self,
    ) -> io::Result<
        Option<(
            protocol::NativeConnectionTarget,
            protocol::NativeAuthentication,
        )>,
    > {
        // A previous uncertain member consumes the only slot until explicit cleanup succeeds.
        self.cleanup().await?;
        let selection = self
            .selection
            .take()
            .ok_or_else(|| io::Error::other("friend-world selection was already used"))?;
        self.lease.friend = Some(selection.begin(Instant::now() + protocol::NATIVE_JOIN_TIMEOUT)?);
        let friend = self.lease.friend.as_mut().expect("selected friend owner");
        loop {
            match friend.advance().await? {
                JoinProgress::Ready(_) => break,
                JoinProgress::RetryAt(when) => friend.wait_retry(when).await?,
            }
        }
        self.admitted = true;
        let claims = self
            .claims
            .take()
            .ok_or_else(|| io::Error::other("friend-world client data was already used"))?;
        friend
            .prepare_friend(claims, self.random_id)
            .await
            .map(Some)
    }

    /// Continues RTA even while transport connection, bootstrap publication or world queues are blocked.
    async fn drive(&mut self) -> io::Result<()> {
        if !self.admitted {
            return std::future::pending().await;
        }
        self.lease
            .friend
            .as_mut()
            .ok_or_else(|| io::Error::other("friend-world owner is unavailable"))?
            .drive_active()
            .await
    }

    /// Makes one finite cleanup attempt; Drop returns any unresolved exact membership to the reserved slot.
    async fn close(&mut self) {
        if self.cleanup().await.is_err() {
            if let Some(friend) = &mut self.lease.friend {
                friend.stop_transport();
            }
            bevy::log::warn!("friend-world membership cleanup remains pending");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "developer-control")]
    fn cleanup_observation_follows_the_original_reserved_lifetime() {
        let owner = FriendCleanupOwner::default();
        assert_eq!(owner.observation(), (false, false));
        let lease = owner.reserve().unwrap();
        assert_eq!(owner.observation(), (true, false));
        drop(lease);
        assert_eq!(owner.observation(), (false, false));
    }

    #[test]
    fn cleanup_capacity_backpressures_another_join_until_the_reserved_owner_returns() {
        let owner = FriendCleanupOwner::default();
        let first = owner.reserve().unwrap();
        assert_eq!(
            owner.reserve().err().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(first);
        assert!(owner.reserve().is_ok());
    }
}

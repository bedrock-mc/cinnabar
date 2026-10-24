//! Original native host membership, conditional nonce authority and explicit advertised lifetimes.

mod active;
pub(crate) use active::HostedInvitation;
pub use active::{ActiveHostedWorld, ClosingHostedWorld};
mod approval;
mod directory;
mod login;
pub use login::{HostedLoginApproval, NativeHostedLoginPolicy};
mod serve;
pub use serve::{ClosingHostedPeer, NativeHostedPeer, NativeHostedProcess};
mod slot;
pub(crate) use slot::HostSlot;

use super::{NativeAccount, directory::Scope};
use crate::local_worlds::HostOpening;
use bedrock_services::xbox::mpsd;
use directory::HostDirectory;
use std::{io, sync::Arc, time::Instant};

/// Title approval for a Login XUID already authenticated by a future transport's identity owner.
/// This proves only the acknowledged nonce policy; it does not authenticate SDP or Login itself.
/// No Debug or serialization exposes its original world/account/session identity.
pub struct HostedApproval {
    opening: HostOpening,
    scope: Scope,
    reference: mpsd::Reference,
    xuid: String,
    member_epoch: u64,
    nonce: [u8; 32],
}

/// One caller-driven host directory owner, with a manager-owned bounded unresolved-cleanup slot.
/// Construction creates no activity, route, invitation, listener, worker or runtime.
/// `activate` explicitly consumes an unpublished owner into its actual advertised lifetime.
#[must_use = "drive the host directory and explicitly close its original remote membership"]
pub struct NativeHostedWorld {
    slot: Option<HostSlot>,
    directory: Option<HostDirectory>,
    peers: active::peers::Peers,
}

impl NativeHostedWorld {
    /// Establishes acknowledged RTA and publishes one stable unadvertised host membership.
    /// Cancellation retains the original attempt before its first directory write await.
    pub async fn advance(&mut self) -> io::Result<()> {
        self.directory_mut()?.advance().await
    }

    /// Drives one finite RTA event and any resulting complete-snapshot refresh, without idle timeout.
    /// Keep this actual future pinned beside application I/O so active service work is not replayed.
    pub async fn drive(&mut self) -> io::Result<()> {
        self.directory_mut()?.drive().await
    }

    /// Explicitly publishes the current bounded nonce intent using the original binding and exact ETag.
    /// A412 requires synchronization and another caller decision; no write is retried internally.
    pub async fn publish_approvals(&mut self, deadline: Instant) -> io::Result<()> {
        self.directory_mut()?.publish_approvals(deadline).await
    }

    /// Checks acknowledged title nonce policy for an already authenticated Login identity.
    /// The caller must cryptographically authenticate authenticated_xuid before invoking this method.
    /// WouldBlock means another operation temporarily owns the account; retain the original Login.
    pub fn check_approval(
        &self,
        authenticated_xuid: &str,
        nonce: &str,
    ) -> io::Result<HostedApproval> {
        self.directory_ref()?
            .check_approval(authenticated_xuid, nonce)
    }

    /// Rechecks the same opening/account/reference/member/nonce revision before final transport admission.
    /// WouldBlock denies new admission while retaining the original approval for an explicit retry.
    pub fn recheck_approval(&self, approval: &HostedApproval) -> io::Result<()> {
        self.directory_ref()?.recheck_approval(approval)
    }

    /// Explicitly reconciles and leaves the original membership, including after its world/socket ends.
    /// Failure/cancellation keeps this owner reachable; Drop returns it to the manager's sole slot.
    pub async fn close(&mut self, deadline: Instant) -> io::Result<()> {
        if self.directory.is_none() {
            return Ok(());
        }
        self.directory_mut()?.close(deadline).await?;
        self.directory = None;
        self.return_slot();
        Ok(())
    }

    /// Borrows the live owner without fabricating a replacement after closure.
    fn directory_mut(&mut self) -> io::Result<&mut HostDirectory> {
        self.directory.as_mut().ok_or_else(closed)
    }

    /// Borrows the same owned membership for immediate policy checks.
    fn directory_ref(&self) -> io::Result<&HostDirectory> {
        self.directory.as_ref().ok_or_else(closed)
    }

    /// Releases one unique reservation, retaining any uncertain remote obligation without an Arc cycle.
    fn return_slot(&mut self) {
        if let Some(slot) = self.slot.take() {
            slot.release(self.directory.take());
        }
    }
}

impl Drop for NativeHostedWorld {
    fn drop(&mut self) {
        self.return_slot();
    }
}

/// Reports only a local lifecycle reason, without world handles or credentials.
fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotConnected,
        "native host directory is closed",
    )
}

#[cfg(test)]
mod tests;

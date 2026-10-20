//! Fresh concrete application authority at the original retained transport commit boundary.

use super::{HostedApproval, HostedLoginApproval, NativeHostedLoginPolicy, NativeHostedWorld};
use bedrock_net::login::AuthenticatedLogin;
use protocol::{NativeAcceptingIo, NativeAcceptingSend};
use std::{io, time::Instant};

/// Borrows the exact existing predicate owners for this accepting continuation.
pub(super) enum SendAuthority<'a> {
    Policy(&'a NativeHostedLoginPolicy),
    KeyExchange(&'a NativeHostedLoginPolicy, &'a HostedLoginApproval),
    Login(
        &'a AuthenticatedLogin,
        &'a NativeHostedLoginPolicy,
        &'a HostedLoginApproval,
    ),
    Member(&'a HostedApproval),
}

impl SendAuthority<'_> {
    /// Reuses each original predicate without retaining a new approval or copying policy.
    fn check(&self, host: &NativeHostedWorld) -> io::Result<()> {
        match self {
            Self::Policy(policy) => host.directory_ref()?.check_login_policy(policy),
            Self::KeyExchange(policy, approval) => host.recheck_login_grant(policy, approval),
            Self::Login(login, policy, approval) => host.recheck_login(login, policy, approval),
            Self::Member(approval) => host.recheck_approval(approval),
        }
    }

    /// Retains this original concrete predicate while the actual account owner is checked out.
    async fn wait(&self, host: &mut NativeHostedWorld, deadline: Instant) -> io::Result<()> {
        loop {
            host.directory_mut()?
                .service_barrier(Some(deadline))
                .await?;
            match self.check(host) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
        }
    }
}

impl NativeHostedWorld {
    /// Drives original capacity/receipts; every new synchronous commit follows current service authority.
    pub(super) async fn advance_send(
        &mut self,
        send: &mut NativeAcceptingSend<'_>,
        authority: SendAuthority<'_>,
    ) -> io::Result<()> {
        let deadline = send.deadline();
        loop {
            let capacity = self.peer_io(Some(deadline), send.next()).await?;
            let Some(capacity) = capacity else {
                return authority.wait(self, deadline).await;
            };
            authority.wait(self, deadline).await?;
            capacity.commit()?;
        }
    }

    /// Completes setup output before its original typed flushed or codec-install continuation.
    pub(super) async fn send_setup(
        &mut self,
        mut send: NativeAcceptingSend<'_>,
        authority: SendAuthority<'_>,
    ) -> io::Result<()> {
        self.advance_send(&mut send, authority).await?;
        send.finish()
    }

    /// Settles only already committed output before consuming the original global close capability.
    pub(super) async fn retire_wire(wire: &mut NativeAcceptingIo, deadline: Instant) {
        let _ = wire.begin_send_retirement(deadline).finish().await;
    }

    /// Exercises the same production admission driver without pretending to own a backend receipt.
    #[cfg(test)]
    pub(in crate::native_account::hosted_world) async fn send_member_raw(
        &mut self,
        approval: &HostedApproval,
        wire: &mut NativeAcceptingIo,
        packet: bytes::Bytes,
        deadline: Instant,
    ) -> io::Result<()> {
        let send = wire.prepare_raw(packet, deadline)?;
        self.send_setup(send, SendAuthority::Member(approval)).await
    }
}

//! One backend delivery obligation; packet bytes and wire progress stay with their original owners.

use super::{HostedApproval, NativeAcceptingIo, NativeHostedWorld, SendAuthority};
use bytes::Bytes;
use std::{io, time::Instant};

#[derive(Default)]
pub(in crate::native_account::hosted_world) enum Output {
    #[default]
    Idle,
    Forwarding(Instant),
    Acknowledging(Instant),
}

impl Output {
    /// Records backend delivery before any service postcheck can discard its selected result.
    pub(in crate::native_account::hosted_world) fn stage(
        &mut self,
        wire: &mut NativeAcceptingIo,
        packet: Bytes,
        deadline: Instant,
    ) -> io::Result<()> {
        if !matches!(self, Self::Idle) {
            return Err(invalid());
        }
        *self = Self::Forwarding(deadline);
        let _retained = wire.prepare_raw(packet, deadline)?;
        Ok(())
    }

    /// Borrows the first delivered packet's cutoff, including throughout its backend receipt.
    pub(super) fn deadline(&self) -> Option<Instant> {
        match self {
            Self::Idle => None,
            Self::Forwarding(deadline) | Self::Acknowledging(deadline) => Some(*deadline),
        }
    }

    /// Resumes the same prepared effect; successful completion remains retained until received succeeds.
    pub(in crate::native_account::hosted_world) async fn forward(
        &mut self,
        host: &mut NativeHostedWorld,
        approval: &HostedApproval,
        wire: &mut NativeAcceptingIo,
    ) -> io::Result<()> {
        if let Self::Forwarding(deadline) = *self {
            host.advance_send(&mut wire.resume_send()?, SendAuthority::Member(approval))
                .await?;
            *self = Self::Acknowledging(deadline);
        }
        Ok(())
    }

    /// Marks only an externally proven whole effect eligible for its original backend receipt.
    pub(super) fn externally_complete(&mut self) {
        if let Self::Forwarding(deadline) = *self {
            *self = Self::Acknowledging(deadline);
        }
    }

    /// Records actual received completion before its service wrapper can fail a later postcheck.
    pub(in crate::native_account::hosted_world) fn acknowledged(
        &mut self,
        wire: &mut NativeAcceptingIo,
    ) -> io::Result<()> {
        if !matches!(self, Self::Acknowledging(_)) {
            return Err(invalid());
        }
        wire.resume_send()?.finish()?;
        *self = Self::Idle;
        Ok(())
    }
}

/// Refuses a second delivered packet without changing the sole original obligation.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "native world output is still retained",
    )
}

//! Exact running-world opening retained beside its actual private process connection.

use super::{NativeHostedWorld, rejected};
use crate::local_worlds::HostOpening;
use bridge::host_world::{
    HostProcess, PendingHostProcess,
    contract::{CAPABILITY_BYTES, OPENING_BYTES},
};
use std::{io, time::Instant};
use zeroize::Zeroizing;

/// One original running-world process, never reconstructed from a local address or display name.
#[must_use]
pub struct NativeHostedProcess {
    pub(super) process: HostProcess,
    opening: HostOpening,
}

impl NativeHostedWorld {
    /// Establishes only a private IPC connection to this original live Dragonfly child.
    /// Call from the existing worlds/account worker, never a Bevy frame. No route is advertised.
    pub async fn prepare_dragonfly_bridge(
        &mut self,
        deadline: Instant,
    ) -> io::Result<NativeHostedProcess> {
        let opening = loop {
            self.directory_mut()?
                .service_barrier(Some(deadline))
                .await?;
            match self.directory_ref()?.opening() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => break result?,
            }
        };
        let mut capability = Zeroizing::new([0; CAPABILITY_BYTES as usize]);
        let mut identifier = Zeroizing::new([0; OPENING_BYTES as usize]);
        getrandom::fill(&mut *capability).map_err(|_| rejected())?;
        getrandom::fill(&mut *identifier).map_err(|_| rejected())?;
        let pending = self
            .peer_io(
                Some(deadline),
                PendingHostProcess::bind(&capability, &identifier),
            )
            .await?;
        self.wait_process(&opening, deadline).await?;
        opening.request_bridge(pending.bootstrap())?;
        let process = self
            .peer_io(Some(deadline), pending.establish(deadline))
            .await?;
        self.wait_process(&opening, deadline).await?;
        Ok(NativeHostedProcess { process, opening })
    }

    /// Checks the original opaque opening, including its actual account/RTA proof and process owner.
    pub(super) fn check_process(&self, opening: &HostOpening) -> io::Result<()> {
        if !self.directory_ref()?.opening()?.same(opening) {
            return Err(rejected());
        }
        opening.validate()
    }

    /// Rechecks the concrete prepared bridge before any peer is routed into its selected world.
    pub(in crate::native_account::hosted_world) async fn check_bridge(
        &mut self,
        process: &NativeHostedProcess,
        deadline: Instant,
    ) -> io::Result<()> {
        self.wait_process(&process.opening, deadline).await
    }

    /// Retains the original process/opening while unrelated account work temporarily owns its proof.
    pub(super) async fn wait_process(
        &mut self,
        opening: &HostOpening,
        deadline: Instant,
    ) -> io::Result<()> {
        loop {
            self.peer_io(Some(deadline), std::future::ready(Ok(())))
                .await?;
            match self.check_process(opening) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => return result,
            }
        }
    }
}

impl NativeHostedProcess {
    /// Explicitly closes this original IPC bridge even after its account/world authority retires.
    /// Failure/cancellation retains the real owner and first finite cleanup deadline for retry.
    pub async fn close(&mut self, deadline: Instant) -> io::Result<()> {
        self.process.close(deadline).await
    }
}

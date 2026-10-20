//! One concrete paired play lifetime, owned and polled by the existing application worker.

pub(in crate::native_account::hosted_world) mod output;
use output::Output;
mod backend;
mod step;
use backend::{BackendProgress, ClosingPeerBackend, PeerBackend};
use step::Phase;
pub(in crate::native_account::hosted_world) use step::Step;

use super::{HostedApproval, HostedLoginApproval, NativeHostedWorld, SendAuthority};
use bedrock_client::server::session::AcceptedSession;
use bridge::host_world::{ActiveBackendPeer, ClosingBackendPeer, contract::CLOSE_MILLIS};
use protocol::{NativeAcceptingIo, NativeAcceptingRetired, NativeAcceptingSendOutcome};
use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};

/// Actual initialized Login, original hosted approval, external codec and Go world connection.
/// No worker is spawned. The caller retains this owner and polls it on its existing runtime.
#[must_use]
pub struct NativeHostedPeer {
    wire: NativeAcceptingIo,
    backend: PeerBackend,
    accepted: AcceptedSession<HostedLoginApproval>,
    output: Output,
    phase: Phase,
    backend_deadline: Option<Instant>,
    turn_control: Arc<nethernet_tokio::SessionSender>,
}

/// New play admission ended; original backend output can finish before actual transport retirement.
#[must_use]
pub struct ClosingHostedPeer {
    wire: Option<NativeAcceptingIo>,
    retired: Option<NativeAcceptingRetired>,
    backend: Option<ClosingPeerBackend>,
    accepted: AcceptedSession<HostedLoginApproval>,
    output: Output,
    deadline: Instant,
    failure: Option<Arc<io::Error>>,
    settled: bool,
    finished: bool,
}

impl NativeHostedPeer {
    /// Borrows only this actual typed Initialized session's original title approval.
    pub(in crate::native_account::hosted_world) fn approval(&self) -> &HostedApproval {
        &self.accepted.grant().approval
    }
    /// Borrows the actual initialized peer's same actor without exposing raw sends publicly.
    pub(in crate::native_account::hosted_world) fn turn_control(
        &self,
    ) -> &Arc<nethernet_tokio::SessionSender> {
        &self.turn_control
    }
    /// Takes only the actual typed initialization and both corresponding live IO owners.
    pub(super) fn new(
        wire: NativeAcceptingIo,
        backend: ActiveBackendPeer,
        accepted: AcceptedSession<HostedLoginApproval>,
        turn_control: Arc<nethernet_tokio::SessionSender>,
    ) -> Self {
        Self {
            wire,
            backend: PeerBackend::Dragonfly(Box::new(backend)),
            accepted,
            output: Output::Idle,
            phase: Phase::Receive,
            backend_deadline: None,
            turn_control,
        }
    }

    /// Takes the actual BDS initialized handoff without creating another transport or coordinator.
    pub(super) fn new_bds(
        wire: NativeAcceptingIo,
        backend: protocol::NativeHostedUpstream,
        accepted: AcceptedSession<HostedLoginApproval>,
        turn_control: Arc<nethernet_tokio::SessionSender>,
    ) -> Self {
        Self {
            wire,
            backend: PeerBackend::Bds {
                upstream: Box::new(backend),
                ended: false,
            },
            accepted,
            output: Output::Idle,
            phase: Phase::Receive,
            backend_deadline: None,
            turn_control,
        }
    }

    /// Drives the original host/RTA and both play directions without an idle timeout.
    /// Keep this actual future pinned. Cancellation physically retires the two owned sockets;
    /// the separate host directory remains with its sole application cleanup owner.
    /// A terminal result returns the same peer as a bounded explicit closing owner.
    pub async fn run(
        mut self,
        host: &mut NativeHostedWorld,
    ) -> (ClosingHostedPeer, io::Result<()>) {
        let result = self.play(host).await;
        self.into_closing(result, cleanup_deadline())
    }

    /// Moves the same peer into explicit retirement, retaining its first play failure and caller cutoff.
    pub(in crate::native_account::hosted_world) fn into_closing(
        self,
        result: io::Result<()>,
        deadline: Instant,
    ) -> (ClosingHostedPeer, io::Result<()>) {
        let mut closing = self.begin_close(deadline);
        let result = match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let error = Arc::new(error);
                closing.failure = Some(Arc::clone(&error));
                Err(repeat(&error))
            }
        };
        (closing, result)
    }

    /// Uses the same owned IO steps as the multi-peer coordinator, with original authority at commit.
    async fn play(&mut self, host: &mut NativeHostedWorld) -> io::Result<()> {
        loop {
            host.wait_approval(&self.accepted.grant().approval, self.deadline())
                .await?;
            let event = host.peer_io(self.deadline(), self.next_step()).await?;
            host.wait_approval(&self.accepted.grant().approval, self.deadline())
                .await?;
            if self.apply(event)? {
                return Ok(());
            }
        }
    }

    /// Captures one finite closing budget and prevents all new application sends.
    pub fn begin_close(self, deadline: Instant) -> ClosingHostedPeer {
        let deadline = self
            .output
            .deadline()
            .map_or(deadline, |first| first.min(deadline));
        ClosingHostedPeer {
            wire: Some(self.wire),
            retired: None,
            backend: Some(self.backend.begin_close(deadline)),
            accepted: self.accepted,
            output: self.output,
            deadline,
            failure: None,
            settled: false,
            finished: false,
        }
    }
}

impl ClosingHostedPeer {
    /// Reports actual transport retirement independently of the retained original play failure.
    pub(in crate::native_account::hosted_world) fn is_finished(&self) -> bool {
        self.finished
    }

    /// Drains accepted Go output with fresh original authority before each new external commit.
    /// Cancellation retains partial framing, the delivered obligation and its first deadline.
    /// Revocation permits only an already committed whole effect's receipt; partial output aborts
    /// the backend without reporting Flushed. No peer-Minecraft processing is promised.
    pub async fn finish_close(&mut self, host: &mut NativeHostedWorld) -> io::Result<()> {
        if self.finished {
            return self.result();
        }
        if self.failure.is_none()
            && let Err(error) = self.drain_backend(host).await
        {
            self.failure = Some(Arc::new(error));
        }
        if self.failure.is_some() && !self.settled {
            self.settle_output().await;
            if let Some(backend) = &mut self.backend {
                let _ = backend.settle_failure().await;
            }
            self.backend = None;
            self.settled = true;
        }
        if self.retired.is_none() {
            self.retired = Some(self.wire.take().expect("one consuming close").begin_close());
        }
        let result = self
            .retired
            .as_mut()
            .expect("retired transport")
            .finish_close(self.deadline)
            .await;
        if result.is_ok() {
            self.finished = true;
        }
        self.result().and(result)
    }

    /// Resumes an existing delivery before reading another packet from the original backend slot.
    async fn drain_backend(&mut self, host: &mut NativeHostedWorld) -> io::Result<()> {
        loop {
            if let Some(deadline) = self.output.deadline() {
                let wire = self.wire.as_mut().expect("established until backend drain");
                self.output
                    .forward(host, &self.accepted.grant().approval, wire)
                    .await?;
                let backend = self.backend.as_mut().expect("live backend drain");
                let output = &mut self.output;
                host.peer_io(Some(deadline), async {
                    backend.received().await?;
                    output.acknowledged(wire)
                })
                .await?;
            }
            let Some(backend) = &mut self.backend else {
                return Ok(());
            };
            match host
                .peer_io(Some(self.deadline), backend.finish_close())
                .await
            {
                Ok(()) => {
                    self.backend = None;
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    let wire = self.wire.as_mut().expect("established until backend drain");
                    let output = &mut self.output;
                    let deadline = self.deadline;
                    host.peer_io(Some(deadline), async {
                        if let Some(packet) = backend.recv().await? {
                            output.stage(wire, packet, deadline)?;
                        }
                        Ok(())
                    })
                    .await?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Seals new admission and settles only the exact committed receipt without requiring new authority.
    async fn settle_output(&mut self) {
        let wire = self.wire.as_mut().expect("original wire until retirement");
        let result = wire.begin_send_retirement(self.deadline).finish().await;
        if matches!(result, Ok(NativeAcceptingSendOutcome::Complete))
            && self.output.deadline().is_some()
        {
            self.output.externally_complete();
            if let Some(backend) = &mut self.backend
                && backend.received().await.is_ok()
            {
                let _ = self.output.acknowledged(wire);
            }
        }
    }

    /// Keeps the first failure primary through later transport retirement and repeated completion.
    fn result(&self) -> io::Result<()> {
        self.failure
            .as_ref()
            .map_or(Ok(()), |error| Err(repeat(error)))
    }
}

/// One finite edge operation budget; idle play never uses this deadline.
pub(in crate::native_account::hosted_world) fn cleanup_deadline() -> Instant {
    Instant::now() + Duration::from_millis(u64::from(CLOSE_MILLIS))
}

/// Ends a delivered-output path without exposing world identifiers.
fn ended() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "native world ended during output",
    )
}

/// Preserves the first failure's class and cause across explicit closing calls.
fn repeat(error: &Arc<io::Error>) -> io::Error {
    io::Error::new(error.kind(), Arc::clone(error))
}

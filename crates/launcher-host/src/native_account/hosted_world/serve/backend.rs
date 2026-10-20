//! Concrete original world capabilities; only Dragonfly owns a private process bridge.
use super::*;
use crate::local_worlds::HostOpening;
use protocol::NativeConnectionTarget;

pub(in crate::native_account::hosted_world) enum HostedBackend {
    Dragonfly(Box<NativeHostedProcess>),
    Bds {
        opening: Box<HostOpening>,
        target: NativeConnectionTarget,
    },
}

impl NativeHostedWorld {
    /// Captures the original backend without opening a spare player connection or starting a process.
    pub(in crate::native_account::hosted_world) async fn prepare_backend(
        &mut self,
        deadline: Instant,
    ) -> io::Result<HostedBackend> {
        let opening = loop {
            self.directory_mut()?
                .service_barrier(Some(deadline))
                .await?;
            match self.directory_ref()?.opening() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => break result?,
            }
        };
        match opening.world().backend {
            bridge::Backend::Dragonfly => self
                .prepare_dragonfly_bridge(deadline)
                .await
                .map(Box::new)
                .map(HostedBackend::Dragonfly),
            bridge::Backend::Bds => {
                let target = opening.native_connection_target()?;
                self.wait_process(&opening, deadline).await?;
                Ok(HostedBackend::Bds {
                    opening: Box::new(opening),
                    target,
                })
            }
        }
    }

    /// Checks the same selected process generation, independently of the backend transport dialect.
    pub(in crate::native_account::hosted_world) async fn check_backend(
        &mut self,
        backend: &HostedBackend,
        deadline: Instant,
    ) -> io::Result<()> {
        match backend {
            HostedBackend::Dragonfly(process) => self.check_bridge(process, deadline).await,
            HostedBackend::Bds { opening, .. } => self.wait_process(opening, deadline).await,
        }
    }

    /// Dispatches one original accepting association through the selected concrete backend.
    pub(in crate::native_account::hosted_world) async fn accept_backend_peer(
        &mut self,
        backend: &mut HostedBackend,
        accepted: nethernet_tokio::AcceptedSession,
        key: Arc<SigningKey>,
        options: ServerHandshakeOptions,
        deadline: Instant,
    ) -> io::Result<NativeHostedPeer> {
        match backend {
            HostedBackend::Dragonfly(process) => {
                self.accept_dragonfly_peer(process, accepted, key, options, deadline)
                    .await
            }
            HostedBackend::Bds { opening, target } => {
                self.accept_bds_peer(opening, target, accepted, key, options, deadline)
                    .await
            }
        }
    }
}

impl HostedBackend {
    /// Releases only host-serving capability; the original manager retains BDS stop/save rights.
    pub(in crate::native_account::hosted_world) async fn close(
        &mut self,
        deadline: Instant,
    ) -> io::Result<()> {
        match self {
            Self::Dragonfly(process) => process.close(deadline).await,
            Self::Bds { .. } => Ok(()),
        }
    }
}

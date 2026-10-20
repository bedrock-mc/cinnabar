//! Concrete original peer legs; local BDS cursor release is distinct from Dragonfly IPC receipts.
use super::*;
use bytes::Bytes;
use protocol::{NativeHostedUpstream, NativeHostedUpstreamClosing};

pub(super) enum PeerBackend {
    Dragonfly(Box<ActiveBackendPeer>),
    Bds {
        upstream: Box<NativeHostedUpstream>,
        ended: bool,
    },
}

pub(super) enum BackendProgress {
    Complete,
    Read,
    Capacity,
}

pub(super) enum ClosingPeerBackend {
    Dragonfly(Box<ClosingBackendPeer>),
    Bds {
        upstream: Box<NativeHostedUpstreamClosing>,
        closed: bool,
        ended: bool,
    },
}

impl PeerBackend {
    /// Borrows only the existing leg; no background reader or receive queue is introduced.
    pub async fn recv(&mut self) -> io::Result<Option<Bytes>> {
        match self {
            Self::Dragonfly(backend) => backend.recv().await,
            Self::Bds { upstream, ended } => {
                if *ended {
                    Ok(None)
                } else {
                    upstream.receive().await.map(Some)
                }
            }
        }
    }

    /// Prepares the original effect without committing the BDS cipher or its capacity permit.
    pub fn queue_send(&mut self, packet: &Bytes, deadline: Instant) -> io::Result<()> {
        match self {
            Self::Dragonfly(backend) => backend.queue_send(packet, deadline),
            Self::Bds {
                upstream,
                ended: false,
            } => upstream.prepare_raw(packet.clone(), deadline).map(|_| ()),
            Self::Bds { ended: true, .. } => Err(ended()),
        }
    }

    /// Returns the same BDS permit to the coordinator's fresh synchronous authority boundary.
    pub async fn flush_send(&mut self) -> io::Result<BackendProgress> {
        match self {
            Self::Dragonfly(backend) => match backend.flush_send().await {
                Ok(()) => Ok(BackendProgress::Complete),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    Ok(BackendProgress::Read)
                }
                Err(error) => Err(error),
            },
            Self::Bds { upstream, .. } => {
                let mut send = upstream.resume_send()?;
                if send.next().await?.is_some() {
                    Ok(BackendProgress::Capacity)
                } else {
                    send.finish()?;
                    Ok(BackendProgress::Complete)
                }
            }
        }
    }

    /// Commits only the original permit that this backend returned before the authority check.
    pub fn commit_capacity(&mut self) -> io::Result<()> {
        match self {
            Self::Bds { upstream, .. } => upstream.resume_send()?.reserved_capacity()?.commit(),
            Self::Dragonfly(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "backend has no retained transport permit",
            )),
        }
    }

    /// Completes the real IPC receipt or releases only the original BDS source cursor.
    pub async fn received(&mut self, deadline: Instant) -> io::Result<()> {
        match self {
            Self::Dragonfly(backend) => backend.received(deadline).await,
            Self::Bds { upstream, ended } => {
                *ended |= upstream.release_received()?;
                Ok(())
            }
        }
    }

    /// Retains the original close owner and seals all new local upstream send admission.
    pub fn begin_close(self, deadline: Instant) -> ClosingPeerBackend {
        match self {
            Self::Dragonfly(backend) => {
                ClosingPeerBackend::Dragonfly(Box::new((*backend).begin_close(deadline)))
            }
            Self::Bds { upstream, ended } => ClosingPeerBackend::Bds {
                upstream: Box::new((*upstream).begin_close(deadline)),
                closed: false,
                ended,
            },
        }
    }
}

impl ClosingPeerBackend {
    /// On revocation, settles only an original BDS receipt and physical transport, without forwarding.
    pub async fn settle_failure(&mut self) -> io::Result<()> {
        match self {
            Self::Dragonfly(_) => Ok(()),
            Self::Bds { upstream, .. } => upstream.finish_close().await,
        }
    }

    /// Completes original physical retirement before draining BDS's already accepted raw prefix.
    pub async fn finish_close(&mut self) -> io::Result<()> {
        match self {
            Self::Dragonfly(backend) => backend.finish_close().await,
            Self::Bds {
                upstream,
                closed,
                ended,
            } => {
                upstream.finish_close().await?;
                *closed = true;
                if *ended {
                    Ok(())
                } else {
                    Err(io::ErrorKind::WouldBlock.into())
                }
            }
        }
    }

    /// A closed BDS EOF completes its actual accepted prefix; Dragonfly still requires its IPC close.
    pub async fn recv(&mut self) -> io::Result<Option<Bytes>> {
        match self {
            Self::Dragonfly(backend) => backend.recv().await?.map(Some).ok_or_else(ended),
            Self::Bds {
                upstream,
                closed,
                ended,
            } => {
                if *ended {
                    return Ok(None);
                }
                match upstream.receive().await {
                    Ok(packet) => Ok(Some(packet)),
                    Err(error) if *closed && error.kind() == io::ErrorKind::UnexpectedEof => {
                        *ended = true;
                        Ok(None)
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }

    /// Never claims BDS peer processing or fabricates a Dragonfly Received acknowledgment.
    pub async fn received(&mut self) -> io::Result<()> {
        match self {
            Self::Dragonfly(backend) => backend.received().await,
            Self::Bds {
                upstream, ended, ..
            } => {
                *ended |= upstream.release_received()?;
                Ok(())
            }
        }
    }
}

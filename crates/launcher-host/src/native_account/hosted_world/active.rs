//! One advertised host incarnation, driven by the application's existing reactor.

mod acquire;
pub(super) mod peers;
mod renewal;
pub(super) mod setup;

use super::{ClosingHostedPeer, NativeHostedWorld, serve::HostedBackend};
use bedrock_net::messaging::server::{ClosingMessagingServer, MessagingServer};
use std::{future::Future, io, sync::Arc, time::Instant};
use tokio::sync::{mpsc, oneshot};

/// The generated world bridge cap reserves one original local player.
pub(super) const VISITOR_LIMIT: usize = bridge::contract::HOSTED_WORLD_MAX_PLAYERS as usize - 1;

/// An invitation command cannot be retargeted to a replacement host incarnation.
#[derive(Clone)]
pub(crate) struct HostedInvitation(mpsc::Sender<Invite>);

struct Invite {
    xuid: String,
    deadline: Instant,
    result: oneshot::Sender<io::Result<()>>,
}

/// Actual acknowledged route, activity, transport and process belonging to one original host.
/// No runtime is created; keep `run` pinned on the existing application reactor.
#[must_use = "drive the active host and explicitly close its original membership"]
pub struct ActiveHostedWorld {
    world: NativeHostedWorld,
    server: MessagingServer,
    process: HostedBackend,
    identity: protocol::NativeHostingIdentity,
    expires: Instant,
    phase_started: bool,
    renewal_failure: Option<Arc<io::Error>>,
    retiring: Option<ClosingHostedPeer>,
    visitor_limit: usize,
    invitations: mpsc::Receiver<Invite>,
    invitation: HostedInvitation,
    #[cfg(test)]
    renewal_committed: Option<oneshot::Sender<()>>,
}

/// Accepting and invitation capability ended; the same local and remote owners remain reachable.
#[must_use = "finish retiring the original listener, process and membership"]
pub struct ClosingHostedWorld {
    world: NativeHostedWorld,
    server: ClosingMessagingServer,
    process: HostedBackend,
    deadline: Instant,
    peers: peers::Peers,
}

impl NativeHostedWorld {
    /// Configures the actual accepting route before this world's first original publication.
    /// Activity is acknowledged only after the same membership and process bridge are checked.
    /// Cancellation drops local IO and returns any dispatched remote obligation to its manager slot.
    pub async fn activate(mut self, deadline: Instant) -> io::Result<ActiveHostedWorld> {
        let deadline = self.directory_mut()?.activation_deadline(deadline)?;
        let visitor_limit = self.directory_ref()?.visitor_limit()?.min(VISITOR_LIMIT);
        let acquire::Acquired {
            mut server,
            identity,
            messaging_id,
            expires,
        } = acquire::acceptor(&mut self, deadline, visitor_limit).await?;
        self.directory_mut()?
            .configure_acceptor(&server, messaging_id, visitor_limit)?;
        driving(&mut server, self.advance()).await?;
        driving(&mut server, self.publish_approvals(deadline)).await?;
        let process = driving(&mut server, self.prepare_backend(deadline)).await?;
        driving(
            &mut server,
            self.directory_mut()?.publish_activity(deadline),
        )
        .await?;
        if Instant::now() >= expires {
            return Err(expired());
        }
        let (send, invitations) = mpsc::channel(1);
        self.peers = peers::Peers::new(visitor_limit);
        Ok(ActiveHostedWorld {
            world: self,
            server,
            process,
            identity,
            expires,
            phase_started: false,
            renewal_failure: None,
            retiring: None,
            visitor_limit,
            invitations,
            invitation: HostedInvitation(send),
            #[cfg(test)]
            renewal_committed: None,
        })
    }
}

impl NativeHostedWorld {
    /// Coalesces peer retirement and acknowledged service changes while the same directory drives IO.
    async fn drive_active(&mut self) -> io::Result<()> {
        let wake = self.peers.wake();
        self.peer_io(None, async {
            wake.notified().await;
            Ok(())
        })
        .await
    }
}

impl ActiveHostedWorld {
    /// Creates a command capability only from this actually acknowledged active owner.
    pub(crate) fn invitation(&self) -> HostedInvitation {
        self.invitation.clone()
    }

    /// Services original RTA and independent live peers beside one finite accepting setup.
    /// Token/TURN expiry gates new admission while existing Ready peers keep their original lifetime.
    /// Cancellation during setup or renewal ends further admission; consume with `begin_close`.
    /// Dropping the whole owner returns unresolved membership through the sole manager slot.
    pub async fn run(&mut self) -> io::Result<()> {
        if self.phase_started || self.retiring.is_some() {
            return Err(unavailable());
        }
        loop {
            let current = Instant::now() < self.expires && self.renewal_failure.is_none();
            if !current && self.renewal_failure.is_none() {
                self.renew_admission().await;
                continue;
            }
            if !current && self.world.peers.occupied() == 0 {
                return Err(self.admission_error());
            }
            let accepting = current && self.world.peers.occupied() < self.visitor_limit;
            enum Event {
                Lease,
                Service,
                Peer(Box<nethernet_tokio::AcceptedSession>),
                Invite(Option<Invite>),
            }
            let event = tokio::select! { biased;
                _ = tokio::time::sleep_until(self.expires.into()), if current => Event::Lease,
                result = self.world.drive_active() => { result?; Event::Service },
                invite = self.invitations.recv() => Event::Invite(invite),
                accepted = async {
                    if accepting {
                        self.server.accept().await.map(Some)
                    } else {
                        self.server.maintain().await.map(|()| None)
                    }
                } => match accepted.map_err(|_| unavailable())? {
                    Some(accepted) => Event::Peer(Box::new(accepted)),
                    None => return Err(unavailable()),
                },
            };
            match event {
                Event::Lease => continue,
                Event::Service => {
                    self.retire_ready().await?;
                    let deadline = operation_deadline();
                    let occupied = self.world.peers.occupied();
                    driving(
                        &mut self.server,
                        self.world.directory_mut()?.visitor(occupied, deadline),
                    )
                    .await?;
                    driving(&mut self.server, self.world.publish_approvals(deadline)).await?;
                }
                Event::Invite(Some(invite)) => {
                    if invite.result.is_closed() {
                        continue;
                    }
                    let result = if Instant::now() >= self.expires || self.renewal_failure.is_some()
                    {
                        Err(self.admission_error())
                    } else {
                        driving(
                            &mut self.server,
                            self.world
                                .directory_mut()?
                                .invite(&invite.xuid, invite.deadline),
                        )
                        .await
                    };
                    let _ = invite.result.send(result);
                }
                Event::Invite(None) => return Ok(()),
                Event::Peer(accepted) => {
                    if Instant::now() >= self.expires {
                        drop(accepted);
                        continue;
                    }
                    // A canceled peer phase requires closing this host, not admitting another peer.
                    self.phase_started = true;
                    {
                        let peer = serve(
                            &mut self.world,
                            &mut self.server,
                            &mut self.process,
                            &self.identity,
                            *accepted,
                        );
                        tokio::pin!(peer);
                        loop {
                            tokio::select! { biased;
                                invite = self.invitations.recv() => {
                                    if let Some(invite) = invite { let _ = invite.result.send(Err(busy())); }
                                }
                                result = &mut peer => { result?; break; }
                            }
                        }
                    }
                    self.phase_started = false;
                }
            }
        }
    }

    /// Finishes each checked-out original close while the other owned peers continue through peer_io.
    async fn retire_ready(&mut self) -> io::Result<()> {
        loop {
            if self.retiring.is_none() {
                self.retiring = self.world.peers.take_closing();
            }
            let Some(closing) = &mut self.retiring else {
                return Ok(());
            };
            let closed = driving(&mut self.server, async {
                Ok(closing.finish_close(&mut self.world).await)
            })
            .await?;
            if !closing.is_finished() {
                return closed.and(Err(unavailable()));
            }
            if let Err(error) = closed {
                tracing::debug!(kind = ?error.kind(), "native hosted peer retired");
            }
            self.retiring = None;
            self.world.peers.retired();
        }
    }

    /// Withdraws invite/accept capability synchronously and captures one original finite cleanup cutoff.
    pub fn begin_close(mut self, deadline: Instant) -> ClosingHostedWorld {
        self.invitations.close();
        while let Ok(invite) = self.invitations.try_recv() {
            let _ = invite.result.send(Err(unavailable()));
        }
        if let Some(retiring) = self.retiring.take() {
            self.world.peers.retain_closing(retiring);
        }
        let mut peers = std::mem::take(&mut self.world.peers);
        peers.begin_close(deadline);
        ClosingHostedWorld {
            world: self.world,
            server: self.server.begin_close(),
            process: self.process,
            deadline,
            peers,
        }
    }
}

/// Completes one finite typed setup while the original directory services existing peer continuations.
async fn serve(
    world: &mut NativeHostedWorld,
    server: &mut MessagingServer,
    process: &mut HostedBackend,
    identity: &protocol::NativeHostingIdentity,
    accepted: nethernet_tokio::AcceptedSession,
) -> io::Result<()> {
    let deadline = operation_deadline();
    driving(server, world.publish_approvals(deadline)).await?;
    let peer = driving(server, async {
        Ok(world
            .accept_backend_peer(
                process,
                accepted,
                identity.signing_key(),
                Default::default(),
                deadline,
            )
            .await)
    })
    .await?;
    let peer = match peer {
        Ok(peer) => peer,
        Err(error) => return setup::recover(world, process, error).await,
    };
    world.peers.admit(peer)?;
    let occupied = world.peers.occupied();
    driving(server, world.directory_mut()?.visitor(occupied, deadline)).await?;
    driving(server, world.publish_approvals(deadline)).await
}

impl ClosingHostedWorld {
    /// Attempts every original owner, retaining failed/ambiguous membership for explicit retry or Drop.
    /// Repeated calls use the same first cutoff; no success is inferred from physical socket destruction.
    pub async fn finish_close(&mut self) -> io::Result<()> {
        let peers = self.peers.finish_close(&mut self.world).await;
        let listener = bounded(self.deadline, self.server.finish_close())
            .await
            .and_then(|r| r.map_err(|_| unavailable()));
        let process = self.process.close(self.deadline).await;
        let directory = self.world.close(self.deadline).await;
        peers.and(listener).and(process).and(directory)
    }
}

impl HostedInvitation {
    /// Sends once to the exact active owner; cancellation never retries an ambiguous HTTP dispatch.
    pub(crate) async fn send(&self, xuid: &str, deadline: Instant) -> io::Result<()> {
        let (result, receive) = oneshot::channel();
        bounded(
            deadline,
            self.0.send(Invite {
                xuid: xuid.to_owned(),
                deadline,
                result,
            }),
        )
        .await?
        .map_err(|_| unavailable())?;
        bounded(deadline, receive)
            .await?
            .map_err(|_| unavailable())?
    }
}

/// Keeps the actual application future pinned through signaling progress; no operation is replayed.
async fn driving<T>(
    server: &mut MessagingServer,
    future: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    tokio::pin!(future);
    tokio::select! { biased;
        result = server.maintain() => { result.map_err(|_| unavailable())?; Err(unavailable()) },
        result = &mut future => result,
    }
}

/// Bounds one edge operation without imposing an idle deadline on play.
fn operation_deadline() -> Instant {
    Instant::now() + super::super::ACQUISITION_TIMEOUT
}

/// Applies an original caller cutoff to acquisition/retirement without renewing it.
async fn bounded<T>(deadline: Instant, future: impl Future<Output = T>) -> io::Result<T> {
    if Instant::now() >= deadline {
        return Err(expired());
    }
    let result = tokio::time::timeout_at(deadline.into(), future)
        .await
        .map_err(|_| expired())?;
    if Instant::now() >= deadline {
        return Err(expired());
    }
    Ok(result)
}

/// Redacts account, service, world and transport identifiers.
fn unavailable() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "native hosting is unavailable")
}
/// Stops new admission when the original proof or relay lease expires.
fn expired() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "native hosting admission expired")
}
/// Refuses additional invitation dispatch while the sole visitor owns capacity.
fn busy() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "native hosted visitor capacity is occupied",
    )
}

#[cfg(test)]
mod tests;

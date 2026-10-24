//! Replacement of one accepting socket, retaining the original world, membership and live peers.

use super::*;
use std::time::Duration;

const FIRST_RETRY: Duration = Duration::from_secs(5);
const MAX_RETRY: Duration = Duration::from_secs(120);

impl ActiveHostedWorld {
    /// Keeps retries finite individually while the original lifetime retains all initialized peers.
    /// Cancellation seals admission and leaves the same original owners available to begin_close.
    pub(super) async fn reconnect(&mut self) -> io::Result<()> {
        self.phase_started = true;
        #[cfg(test)]
        if let Some(started) = self.reconnect_started.take() {
            let _ = started.send(());
        }
        let mut delay = FIRST_RETRY;
        loop {
            let deadline = operation_deadline();
            let network_id = self.server.local_network_id();
            #[cfg(test)]
            let replacement = self.replacement.take();
            let candidate = async {
                #[cfg(test)]
                if let Some(replacement) = replacement {
                    let server = self
                        .world
                        .peer_io(Some(deadline), async {
                            replacement.await.map_err(|_| unavailable())
                        })
                        .await?;
                    return Ok(acquire::Replacement {
                        server,
                        identity: None,
                        messaging_id: self.messaging_id,
                        expires: self.expires,
                        ice: nethernet_tokio::ServerIceConfig {
                            turn_streams: Vec::new(),
                            turn_tls: None,
                            ice: Default::default(),
                            bind_addr: std::net::SocketAddr::from((
                                std::net::Ipv4Addr::UNSPECIFIED,
                                0,
                            )),
                        },
                    });
                }
                acquire::replacement(
                    &mut self.world,
                    &self.identity,
                    network_id,
                    deadline,
                    self.visitor_limit,
                )
                .await
            };
            let candidate = reconnecting(&mut self.invitations, candidate).await;
            match candidate {
                Ok(mut candidate) => {
                    // Departures staged while acquiring must finish before occupancy is published.
                    self.retire_ready().await?;
                    let occupied = self.world.peers.occupied();
                    let committed = async {
                        driving(
                            &mut candidate.server,
                            self.world.directory_mut()?.visitor(occupied, deadline),
                        )
                        .await?;
                        self.world
                            .directory_mut()?
                            .replace_acceptor(&candidate.server, candidate.messaging_id)?;
                        driving(
                            &mut candidate.server,
                            self.world.publish_approvals(deadline),
                        )
                        .await?;
                        acquire::renewal::renew_streams(
                            &mut self.world,
                            &mut candidate.server,
                            &candidate.ice,
                            deadline,
                        )
                        .await?;
                        driving(
                            &mut candidate.server,
                            self.world
                                .peer_io(Some(deadline), std::future::ready(Ok(()))),
                        )
                        .await?;
                        if Instant::now() >= candidate.expires {
                            return Err(expired());
                        }
                        Ok(())
                    };
                    if let Err(error) = reconnecting(&mut self.invitations, committed).await {
                        tracing::debug!(kind = ?error.kind(), "native host replacement was not committed");
                        retire_listener(
                            &mut self.world,
                            &mut self.invitations,
                            candidate.server,
                            deadline,
                        )
                        .await;
                    } else {
                        let original = std::mem::replace(&mut self.server, candidate.server);
                        if let Some(identity) = candidate.identity {
                            self.identity = identity;
                        }
                        self.messaging_id = candidate.messaging_id;
                        self.expires = candidate.expires;
                        self.renewal_failure = None;
                        playing(&mut self.server, async {
                            retire_listener(
                                &mut self.world,
                                &mut self.invitations,
                                original,
                                deadline,
                            )
                            .await;
                            Ok(())
                        })
                        .await?;
                        self.phase_started = false;
                        return Ok(());
                    }
                }
                Err(error) => {
                    tracing::debug!(kind = ?error.kind(), "native host signaling replacement failed")
                }
            }
            self.retire_ready().await?;
            reconnecting(
                &mut self.invitations,
                self.world.peer_io(None, async {
                    tokio::time::sleep(delay).await;
                    Ok(())
                }),
            )
            .await?;
            delay = delay.saturating_mul(2).min(MAX_RETRY);
        }
    }
}

/// A reconnecting route grants no invitation dispatch while existing world IO stays caller-driven.
async fn reconnecting<T>(
    invitations: &mut mpsc::Receiver<Invite>,
    operation: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    tokio::pin!(operation);
    loop {
        tokio::select! { biased;
            result = &mut operation => return result,
            invite = invitations.recv(), if !invitations.is_closed() => {
                if let Some(invite) = invite { let _ = invite.result.send(Err(busy())); }
            }
        }
    }
}

/// Seals the retired listener synchronously, then attempts its original finite physical joins.
async fn retire_listener(
    world: &mut NativeHostedWorld,
    invitations: &mut mpsc::Receiver<Invite>,
    listener: MessagingServer,
    deadline: Instant,
) {
    let mut closing = listener.begin_close();
    let closed = reconnecting(
        invitations,
        world.peer_io(Some(deadline), async {
            closing.finish_close().await.map_err(|_| unavailable())
        }),
    )
    .await;
    if let Err(error) = closed {
        tracing::debug!(kind = ?error.kind(), "retired native host signaling cleanup is unconfirmed");
    }
}

//! Actual local BDS Login, retained pack delivery and raw initialized relay handoff.
use super::*;
use crate::local_worlds::HostOpening;
use protocol::{
    NativeConnectionTarget, NativeHostedRelay, NativeHostedRelayProgress, NativeHostedUpstream,
};

impl NativeHostedWorld {
    /// Uses one original BDS route and verified guest; the host account never logs into the offline world.
    pub(super) async fn accept_bds_peer(
        &mut self,
        opening: &HostOpening,
        target: &NativeConnectionTarget,
        accepted: nethernet_tokio::AcceptedSession,
        key: Arc<SigningKey>,
        options: ServerHandshakeOptions,
        deadline: Instant,
    ) -> io::Result<NativeHostedPeer> {
        self.wait_process(opening, deadline).await?;
        let policy = self.login_policy(deadline).await?;
        let turn_control = Arc::new(accepted.session.sender());
        let (mut wire, handshake) = self
            .peer_io(
                Some(deadline),
                NativeAcceptingIo::from_nether(accepted, key, options, deadline),
            )
            .await?;
        let mut upstream = None;
        let preparation = async {
            let ready = self.login(&mut wire, handshake, &policy, deadline).await?;
            self.wait_login(ready.login(), &policy, ready.grant())
                .await?;
            self.wait_process(opening, deadline).await?;
            // The pinned Go host does not opt into upstream blob caching before downstream status exists.
            upstream = Some(
                self.peer_io(Some(deadline), async {
                    NativeHostedUpstream::connect(target, ready.login(), false, deadline)
                        .await
                        .map_err(io::Error::other)
                })
                .await?,
            );
            self.wait_login(ready.login(), &policy, ready.grant())
                .await?;
            self.wait_process(opening, deadline).await?;
            let upstream = upstream.as_mut().expect("original upstream retained");
            let snapshot = upstream.take_packs()?;
            self.deliver_packs(
                &mut wire,
                ready,
                snapshot,
                packs::PackSource::Bds(upstream),
                &policy,
                deadline,
            )
            .await
        }
        .await;
        let ready = match preparation {
            Ok(ready) => ready,
            Err(error) => {
                if let Some(upstream) = upstream {
                    let _ = upstream.begin_close(deadline).finish_close().await;
                }
                Self::retire_wire(&mut wire, deadline).await;
                let _ = wire.begin_close().finish_close(deadline).await;
                return Err(error);
            }
        };
        let mut relay = upstream.expect("prepared original upstream").relay_world(
            wire,
            ready,
            &policy,
            Instant::now(),
            policy.utc().map_err(|_| rejected())?,
        )?;
        let result = self.relay_bds_startup(&mut relay, &policy, opening).await;
        if let Err(error) = result {
            let _ = relay.begin_close(deadline).finish_close().await;
            return Err(error);
        }
        let (wire, upstream, accepted) = relay.finish(
            &policy,
            Instant::now(),
            policy.utc().map_err(|_| rejected())?,
        )?;
        Ok(NativeHostedPeer::new_bds(
            wire,
            upstream,
            accepted,
            turn_control,
        ))
    }

    /// Advances only actual raw effects; every permit commit uses the same original Login authority.
    async fn relay_bds_startup(
        &mut self,
        relay: &mut NativeHostedRelay<HostedLoginApproval>,
        policy: &NativeHostedLoginPolicy,
        opening: &HostOpening,
    ) -> io::Result<()> {
        let deadline = relay.deadline();
        loop {
            self.wait_process(opening, deadline).await?;
            self.wait_login(relay.login(), policy, relay.grant())
                .await?;
            self.peer_io(Some(deadline), relay.receive()).await?;
            self.wait_login(relay.login(), policy, relay.grant())
                .await?;
            let (login, grant, mut send) = relay.prepare_forward()?;
            self.advance_send(&mut send, SendAuthority::Login(login, policy, grant))
                .await?;
            match relay.complete_forward()? {
                NativeHostedRelayProgress::Forwarded => {}
                NativeHostedRelayProgress::Initialized => {
                    self.wait_process(opening, deadline).await?;
                    self.wait_login(relay.login(), policy, relay.grant())
                        .await?;
                    return Ok(());
                }
                NativeHostedRelayProgress::Terminal => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "local hosted world ended during startup",
                    ));
                }
            }
        }
    }
}

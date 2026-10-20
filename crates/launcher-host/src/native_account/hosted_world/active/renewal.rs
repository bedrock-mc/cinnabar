//! One finite same-owner renewal while new admission is sealed and original peers keep progressing.

use super::*;
use acquire::renewal::Renewed;

impl ActiveHostedWorld {
    /// Starts once at the actual proof/credential cutoff; failed renewal never creates a retry loop.
    /// A canceled borrowed run leaves phase_started set, retaining only explicit original cleanup.
    pub(super) async fn renew_admission(&mut self) {
        self.phase_started = true;
        let deadline = operation_deadline();
        let result = {
            let renewing = candidate(&mut self.world, &mut self.server, &self.identity, deadline);
            tokio::pin!(renewing);
            loop {
                tokio::select! { biased;
                    result = &mut renewing => break result,
                    invite = self.invitations.recv(), if !self.invitations.is_closed() => {
                        if let Some(invite) = invite { let _ = invite.result.send(Err(busy())); }
                    }
                }
            }
        };
        #[cfg(test)]
        let committed = result.is_ok();
        match result {
            Ok(Renewed {
                identity,
                ice,
                expires,
            }) => {
                // The completed candidate's final barrier and these setters share this same poll.
                self.server.set_identity(identity.sdp_identity());
                self.server.set_token_trust(identity.offer_trust());
                self.server.set_server_ice(Some(ice));
                self.identity = identity;
                self.expires = expires;
            }
            Err(error) => self.renewal_failure = Some(Arc::new(error)),
        }
        self.phase_started = false;
        #[cfg(test)]
        if committed && let Some(observed) = self.renewal_committed.take() {
            let _ = observed.send(());
        }
    }

    /// Repeats the first renewal failure after the final original peer retires, without another mint.
    pub(super) fn admission_error(&self) -> io::Error {
        self.renewal_failure.as_ref().map_or_else(expired, |error| {
            io::Error::new(error.kind(), error.to_string())
        })
    }
}

/// Retains the complete candidate across temporary checkout, under its one original finite cutoff.
async fn candidate(
    world: &mut NativeHostedWorld,
    server: &mut MessagingServer,
    original: &protocol::NativeHostingIdentity,
    deadline: Instant,
) -> io::Result<Renewed> {
    let renewed = acquire::renewal::renew(world, server, original, deadline).await?;
    if !Arc::ptr_eq(&original.signing_key(), &renewed.identity.signing_key()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "native host renewal changed its original key owner",
        ));
    }
    loop {
        driving(
            server,
            world.peer_io(Some(deadline), std::future::ready(Ok(()))),
        )
        .await?;
        if Instant::now() >= deadline || Instant::now() >= renewed.expires {
            return Err(expired());
        }
        match world.directory_ref()?.opening() {
            Ok(_) => return Ok(renewed),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests;

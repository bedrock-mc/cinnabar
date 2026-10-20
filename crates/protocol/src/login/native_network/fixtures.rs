//! Nondefault paired-host fixtures over the same provider, negotiation and spawn owners.

use super::{
    Connection, NativeAuthentication, NativeProvider, negotiated, packs, spawn,
    transport::Transport,
};
use crate::{GameData, PlaySession, ProtocolError, SessionLogin};
use bedrock_packs::ResourcePackHandoff;
use nethernet_tokio::NetherClient;
use std::{io, time::Instant};

/// Seeds a finite fixture's public provider response through the normal bounded metadata verifier.
/// This does not qualify HTTPS acquisition or weaken any normal provider trust policy.
#[doc(hidden)]
pub fn hosted_provider(
    issuer: &str,
    user_agent: &str,
    jwks: &[u8],
    now: Instant,
) -> io::Result<NativeProvider> {
    NativeProvider::fixture_metadata(issuer, user_agent, jwks, now)
}

/// Runs the sole authenticated client Login/packs/spawn loop on an actual ready fixture association.
/// The fixture owns signed association policy and the original host-issued nonce. No Ready owner,
/// verified identity, archive, GameData or initialization capability is fabricated here.
#[doc(hidden)]
pub async fn hosted_client<P: Send + 'static>(
    client: NetherClient,
    nonce: String,
    address: String,
    authentication: NativeAuthentication,
    login: SessionLogin<'_>,
    prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
) -> Result<(PlaySession, P), ProtocolError> {
    let credentials = negotiated::Credentials::new(Some(authentication))?;
    let mut connection = Connection::new(Transport::fixture_nether(client, nonce));
    let (packs, _) = negotiated::negotiate_connected(
        &mut connection,
        &address,
        negotiated::LoginData::Desktop(&login),
        credentials,
        packs::Acquisition::InBand,
    )
    .await?;
    spawn::continue_join(
        negotiated::NegotiatedConnection {
            connection,
            packs: packs.handoff,
        },
        login,
        prepare,
    )
    .await
}

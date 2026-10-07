//! No challenge, packet, worker or fetch exists before an approved marker.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    crypto::{self, SignedDocument},
    manifest::Offer,
    policy::*,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub server_key: String,
    pub offer: SignedDocument,
}

#[derive(Clone, Debug)]
pub struct VerifiedOffer {
    pub offer: Offer,
    pub digest: String,
}

impl VerifiedOffer {
    /// Reads only the admitted marker bytes, with no external requests.
    pub fn read(bytes: &[u8], audience: &str, now_unix: u64) -> Result<Self> {
        ensure!(bytes.len() <= MAX_MARKER_BYTES, "marker too large");
        let marker: Marker = serde_json::from_slice(bytes)?;
        let (offer, digest): (Offer, _) = marker.offer.verify(
            &marker.server_key,
            crypto::OFFER_DOMAIN,
            MAX_MARKER_BYTES / 2,
        )?;
        ensure!(offer.server_key == marker.server_key, "key substitution");
        offer.validate(audience, now_unix)?;
        Ok(Self { offer, digest })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub version: u16,
    pub api: u16,
    pub capabilities: std::collections::BTreeSet<crate::manifest::Permission>,
    pub offer_digest: String,
    pub client_challenge: String,
    pub connection: String,
    pub subclient: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Accept {
    pub hello: Hello,
    pub server_challenge: String,
    pub session: String,
    pub audience: String,
    pub offer_digest: String,
    pub revision: u64,
    pub expires_unix: u64,
}

#[derive(Clone, Debug)]
pub struct Grant {
    pub offer: VerifiedOffer,
    pub session: String,
    pub connection: String,
    pub subclient: u8,
    pub expires_unix: u64,
}

/// A pending handshake belongs to exactly one live connection generation.
///
/// ```compile_fail
/// fn duplicate(pending: server_experience::negotiation::Pending) {
///     let _ = pending.clone();
/// }
/// ```
#[derive(Debug)]
pub struct Pending {
    offer: VerifiedOffer,
    hello: Hello,
    started_ms: u64,
}

impl Pending {
    /// Called only by the consent controller, never by packet ingress.
    pub fn approve(offer: VerifiedOffer, subclient: u8, now_ms: u64) -> Result<Self> {
        ensure!(subclient <= 3, "invalid subclient");
        let hello = Hello {
            version: WIRE_VERSION,
            api: API_VERSION,
            capabilities: if std::env::var(DEVELOPER_ENV).as_deref() == Ok("1") {
                crate::manifest::developer_permissions()
            } else {
                Default::default()
            },
            offer_digest: offer.digest.clone(),
            client_challenge: crypto::challenge()?,
            connection: crypto::challenge()?,
            subclient,
        };
        Ok(Self {
            offer,
            hello,
            started_ms: now_ms,
        })
    }

    /// Produces the only pre-grant packet, after the user has approved.
    pub fn hello(&self) -> &Hello {
        &self.hello
    }

    /// Consumes the challenge so an accept cannot be replayed within a session.
    pub fn accept(self, document: &SignedDocument, now_unix: u64, now_ms: u64) -> Result<Grant> {
        ensure!(
            now_ms.saturating_sub(self.started_ms) <= NEGOTIATION_TIMEOUT_MS,
            "handshake timed out"
        );
        let (accept, _): (Accept, _) = document.verify(
            &self.offer.offer.server_key,
            crypto::ACCEPT_DOMAIN,
            MAX_PAYLOAD_BYTES,
        )?;
        ensure!(accept.hello == self.hello, "wrong connection challenge");
        ensure!(
            accept.audience == self.offer.offer.audience,
            "wrong audience"
        );
        ensure!(
            accept.offer_digest == self.offer.digest,
            "offer changed after consent"
        );
        ensure!(
            accept.revision == self.offer.offer.revision,
            "deployment revision changed"
        );
        ensure!(
            accept.expires_unix > now_unix && accept.expires_unix <= self.offer.offer.expires_unix,
            "invalid grant expiry"
        );
        crypto::fixed_hex::<32>(&accept.server_challenge)?;
        crypto::fixed_hex::<32>(&accept.session)?;
        Ok(Grant {
            offer: self.offer,
            session: accept.session,
            connection: self.hello.connection,
            subclient: self.hello.subclient,
            expires_unix: accept.expires_unix,
        })
    }
}

/// Canonicalizes the selected address; advertised addresses never replace it.
pub fn canonical_audience(address: &str) -> Result<String> {
    let url = url::Url::parse(&format!("https://{address}"))?;
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "invalid destination"
    );
    ensure!(
        url.path() == "/" && url.query().is_none() && url.fragment().is_none(),
        "invalid destination"
    );
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("missing host"))?;
    ensure!(
        !host.ends_with('.'),
        "use a destination without a trailing dot"
    );
    // Bedrock's default comes from the existing protocol/core contract at the call site.
    let port: u16 = address
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("destination requires an explicit port"))?
        .1
        .parse()?;
    ensure!(port != 0, "invalid destination port");
    Ok(format!("{host}:{port}"))
}

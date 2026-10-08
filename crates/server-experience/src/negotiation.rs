//! No challenge, packet, worker or fetch exists before an approved marker.

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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

/// Ceilings of the selected wire version. A Hello offers the host's constants; an Accept may
/// lower each one and never raise it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Largest inline payload, and largest data of one fragment, in bytes of payload JSON.
    pub max_fragment_bytes: u32,
    /// Largest reassembled payload.
    pub max_message_bytes: u32,
    /// Carrier bytes one direction holds for undelivered messages, an open message included.
    pub max_reassembly_bytes: u32,
}

impl Limits {
    /// The ceilings this host offers.
    pub const fn host() -> Self {
        Self {
            max_fragment_bytes: MAX_PAYLOAD_BYTES as u32,
            max_message_bytes: MAX_MESSAGE_BYTES as u32,
            max_reassembly_bytes: MAX_QUEUE_BYTES as u32,
        }
    }

    /// Nonzero, ordered and nowhere above `offer`.
    fn within(&self, offer: &Self) -> bool {
        0 < self.max_fragment_bytes
            && self.max_fragment_bytes <= self.max_message_bytes
            && self.max_message_bytes <= self.max_reassembly_bytes
            && self.max_fragment_bytes <= offer.max_fragment_bytes
            && self.max_message_bytes <= offer.max_message_bytes
            && self.max_reassembly_bytes <= offer.max_reassembly_bytes
    }
}

/// What a Hello offers: every wire version the client speaks and its ceilings.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WireOffer {
    pub versions: BTreeSet<u16>,
    pub limits: Limits,
}

/// The wire version and ceilings of a session.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Wire {
    pub version: u16,
    pub limits: Limits,
}

impl Wire {
    /// The original wire: whole envelopes of at most `MAX_PAYLOAD_BYTES` and no fragments.
    pub const fn v1() -> Self {
        Self {
            version: WIRE_VERSION,
            limits: Limits {
                max_fragment_bytes: MAX_PAYLOAD_BYTES as u32,
                max_message_bytes: MAX_PAYLOAD_BYTES as u32,
                max_reassembly_bytes: MAX_QUEUE_BYTES as u32,
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub version: u16,
    pub api: u16,
    pub capabilities: BTreeSet<crate::manifest::Permission>,
    pub offer_digest: String,
    pub client_challenge: String,
    pub connection: String,
    pub subclient: u8,
    /// Absent from a v1 client's Hello, which keeps its exact bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<WireOffer>,
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
    /// The selected version; absent when a server selects v1, which keeps v1 Accept bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<Wire>,
}

#[derive(Clone, Debug)]
pub struct Grant {
    pub offer: VerifiedOffer,
    pub session: String,
    pub connection: String,
    pub subclient: u8,
    pub expires_unix: u64,
    pub wire: Wire,
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
            wire: Some(WireOffer {
                versions: (WIRE_VERSION..=MAX_WIRE_VERSION).collect(),
                limits: Limits::host(),
            }),
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
        let wire = match (&self.hello.wire, accept.wire) {
            (_, None) => Wire::v1(),
            (Some(offer), Some(wire)) => {
                ensure!(
                    wire.version != WIRE_VERSION
                        && offer.versions.contains(&wire.version)
                        && wire.limits.within(&offer.limits),
                    "wire selection outside the hello"
                );
                wire
            }
            (None, Some(_)) => bail!("wire selection without an offer"),
        };
        Ok(Grant {
            offer: self.offer,
            session: accept.session,
            connection: self.hello.connection,
            subclient: self.hello.subclient,
            expires_unix: accept.expires_unix,
            wire,
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

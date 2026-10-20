//! Qualified provider credentials and their original finite validity.

mod answer;
mod hosting;
pub(super) use answer::offline_answer_trust;
pub use hosting::NativeHostingIdentity;
mod messaging;
mod provider;
pub use provider::NativeProvider;
mod verifier;
pub(super) use messaging::identity_policy;
pub(super) use messaging::{MessagingAdmission, MessagingTarget};
pub use verifier::NativeTokenVerifier;

use crate::login::client_data::{NativeClaims, RawClientData};
use p384::ecdsa::SigningKey;
use serde_json::value::RawValue;
use std::{
    io,
    sync::Arc,
    time::{Instant, SystemTime},
};
use zeroize::Zeroizing;

/// One acquired response with its service-clock observation and original hard validity.
/// No clock is read, and no token lifetime is restarted by this owner.
pub struct NativeCredential {
    token: Zeroizing<String>,
    clock: NativeServiceClock,
    hard_expiry: SystemTime,
}

impl NativeCredential {
    /// Captures a mint response. Observe monotonic time before sampling service time.
    pub fn new(
        token: String,
        service_time: SystemTime,
        hard_expiry: SystemTime,
        observed: Instant,
    ) -> io::Result<Self> {
        let token = Zeroizing::new(token);
        if token.is_empty()
            || token.len() > verifier::MAX_TOKEN_BYTES
            || hard_expiry <= service_time
        {
            return Err(expired());
        }
        verifier::parsed(&token)?;
        Ok(Self {
            token,
            clock: NativeServiceClock {
                service_time,
                observed,
            },
            hard_expiry,
        })
    }

    /// Borrows the acquired credential only for the provider's bounded key-cache lookup.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Rejects a mint that expired while waiting for provider metadata or connection work.
    pub fn ensure_current(&self, now: Instant) -> io::Result<()> {
        if self.now(now)? < self.hard_expiry {
            Ok(())
        } else {
            Err(expired())
        }
    }

    /// Advances the original service UTC observation with caller-supplied monotonic time.
    fn now(&self, now: Instant) -> io::Result<SystemTime> {
        self.clock.now(now)
    }

    /// Qualifies the original selected key/account and derives its unchanged hard monotonic cutoff.
    fn qualify(
        &self,
        key: &p384::ecdsa::VerifyingKey,
        verifier: &NativeTokenVerifier,
        selected_xuid: &str,
        now: Instant,
    ) -> io::Result<(nethernet::identity::jwt::Claims, Instant)> {
        let identity = verifier.verify(&self.token, key, selected_xuid, self.now(now)?)?;
        let expiry = identity
            .expiry()
            .expect("qualified identity has expiry")
            .min(self.hard_expiry);
        let expires = self
            .clock
            .observed
            .checked_add(
                expiry
                    .duration_since(self.clock.service_time)
                    .map_err(|_| expired())?,
            )
            .ok_or_else(expired)?;
        ensure_current(expires, now)?;
        Ok((identity, expires))
    }
}

/// One verified provider credential and the connection proof key it was minted for.
///
/// The application owns account authentication, secure persistence and device
/// policy. This handoff moves the private key into the connection and keeps the
/// credential out of Debug output. Verification is pure; HTTP remains provider-owned.
pub struct NativeAuthentication {
    pub(super) key: SigningKey,
    pub(super) token: Zeroizing<String>,
    pub(super) claims: NativeClaims,
    pub(super) expires: Instant,
    pub(super) identity: crate::session_wire::SessionIdentity,
    clock: NativeServiceClock,
    verifier: Arc<NativeTokenVerifier>,
    messaging: Option<(bedrock_net::messaging::PreparedMessaging, MessagingTarget)>,
}

impl NativeAuthentication {
    /// Qualifies an acquired credential against trusted provider keys and the selected Xbox identity.
    ///
    /// Acquire the token for this key. Supply account/device/version claims for
    /// the same sign-in policy; the connection supplies ServerAddress and Nonce.
    /// The key is retained through client-data signing and server key exchange.
    ///
    /// # Errors
    /// Rejects untrusted, wrong-identity/key or expired credentials and malformed client JSON.
    /// The signed display name overrides stale application display-name metadata.
    pub fn new(
        key: SigningKey,
        credential: NativeCredential,
        client_data: &RawValue,
        verifier: &NativeTokenVerifier,
        selected_xuid: &str,
        now: Instant,
    ) -> io::Result<Self> {
        credential.ensure_current(now)?;
        let limit = bedrock_net::BatchLimits::default().max_decoded_bytes;
        if client_data.get().len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native account credential or client data exceeds its contract",
            ));
        }
        let claims = NativeClaims::canonical(client_data)?;
        Self::qualify(key, credential, claims, verifier, selected_xuid, now)
    }

    /// Qualifies the same original proof/credential while retaining untrusted relay value lexemes.
    /// This checks credential authority, not full title metadata; the caller must validate that data.
    pub fn new_relay(
        key: SigningKey,
        credential: NativeCredential,
        client_data: RawClientData,
        verifier: &NativeTokenVerifier,
        selected_xuid: &str,
        now: Instant,
    ) -> io::Result<Self> {
        credential.ensure_current(now)?;
        Self::qualify(
            key,
            credential,
            NativeClaims::Relay(client_data),
            verifier,
            selected_xuid,
            now,
        )
    }

    /// Verifies once and installs canonical signed identity for both concrete claim representations.
    fn qualify(
        key: SigningKey,
        credential: NativeCredential,
        mut claims: NativeClaims,
        verifier: &NativeTokenVerifier,
        selected_xuid: &str,
        now: Instant,
    ) -> io::Result<Self> {
        let (identity, expires) =
            credential.qualify(key.verifying_key(), verifier, selected_xuid, now)?;
        claims.set_display_name(
            identity
                .display_name()
                .expect("qualified identity has name"),
        )?;
        Ok(Self {
            key,
            token: credential.token,
            claims,
            expires,
            clock: credential.clock,
            identity: verifier::session_identity(&identity)?,
            verifier: Arc::new(verifier.clone()),
            messaging: None,
        })
    }

    /// Associates this selected-account mint with that account's protected messaging socket.
    /// The peer route must be the caller's explicit joined-session target.
    pub fn with_messaging(
        mut self,
        prepared: bedrock_net::messaging::PreparedMessaging,
        target: crate::NativeMessagingTarget,
    ) -> io::Result<Self> {
        if self.messaging.is_some() || !target.matches_recipient(prepared.recipient()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native messaging target mismatch",
            ));
        }
        self.messaging = Some((prepared, MessagingTarget::Joined(target)));
        Ok(self)
    }

    /// Attaches the caller-authorized, nonce-free service route to this account's protected socket.
    ///
    /// The caller must have obtained this exact recipient from a successful authenticated
    /// Realm or Gathering join on the same acquired account. This method checks transport
    /// consistency, not membership. These services do not supply a joined-friend admission nonce.
    /// Present answers retain provider verification and the originally minted key.
    /// Legacy service routes may omit their answer identity.
    pub fn with_service_messaging(
        mut self,
        prepared: bedrock_net::messaging::PreparedMessaging,
    ) -> io::Result<Self> {
        if self.messaging.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native messaging handoff was already supplied",
            ));
        }
        let target = MessagingTarget::Service(prepared.recipient().clone());
        self.messaging = Some((prepared, target));
        Ok(self)
    }

    /// Attaches this account's protected socket to an explicitly parsed raw service destination.
    /// The target grants no friend membership or Login nonce. Only this explicit raw policy permits
    /// absent answer identity for either dialect; every present assertion still uses provider trust.
    pub fn with_raw_messaging(
        mut self,
        prepared: bedrock_net::messaging::PreparedMessaging,
        target: crate::NativeRawTarget,
    ) -> io::Result<Self> {
        if self.messaging.is_some() || !target.matches_recipient(prepared.recipient()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native raw service target mismatch",
            ));
        }
        self.messaging = Some((prepared, MessagingTarget::Raw(target)));
        Ok(self)
    }

    /// Returns the original absolute Login admission deadline without extending the minted lifetime.
    pub fn expires_at(&self) -> Instant {
        self.expires
    }

    /// Retains the sole proof key, original service clock and configured peer verifier during SDP.
    pub(super) fn into_handoff(self) -> io::Result<AuthenticationHandoff> {
        let key = Arc::new(self.key);
        let messaging = self
            .messaging
            .map(|(prepared, target)| {
                MessagingAdmission::new(
                    prepared,
                    target,
                    Arc::clone(&key),
                    &self.token,
                    self.clock,
                    Arc::clone(&self.verifier),
                    self.expires,
                )
            })
            .transpose()?;
        Ok(AuthenticationHandoff {
            key,
            token: self.token,
            claims: self.claims,
            expires: self.expires,
            identity: self.identity,
            messaging,
            sdp: SdpPolicy {
                clock: self.clock,
                verifier: self.verifier,
            },
        })
    }
}

/// The one qualified key owner after connection admission begins.
pub(super) struct AuthenticationHandoff {
    pub(super) key: Arc<SigningKey>,
    pub(super) token: Zeroizing<String>,
    pub(super) claims: NativeClaims,
    pub(super) expires: Instant,
    pub(super) identity: crate::session_wire::SessionIdentity,
    pub(super) messaging: Option<MessagingAdmission>,
    pub(super) sdp: SdpPolicy,
}

/// Pure original credential policy retained only for an actual signaling consumer.
pub(super) struct SdpPolicy {
    pub(super) clock: NativeServiceClock,
    pub(super) verifier: Arc<NativeTokenVerifier>,
}

/// The mint's original calibrated service observation, shared by local SDP and peer verification.
#[derive(Clone, Copy)]
pub struct NativeServiceClock {
    service_time: SystemTime,
    observed: Instant,
}

impl NativeServiceClock {
    /// Captures the existing SDK service-time observation without creating another credential lifetime.
    pub fn new(service_time: SystemTime, observed: Instant) -> Self {
        Self {
            service_time,
            observed,
        }
    }
    /// Advances service UTC with explicit monotonic time; rollback or overflow fails closed.
    pub fn now(self, now: Instant) -> io::Result<SystemTime> {
        self.service_time
            .checked_add(
                now.checked_duration_since(self.observed)
                    .ok_or_else(expired)?,
            )
            .ok_or_else(expired)
    }
}

/// Rejects expired admission without re-anchoring a copied remaining lifetime.
pub(super) fn ensure_current(expires: Instant, now: Instant) -> io::Result<()> {
    if now < expires {
        Ok(())
    } else {
        Err(expired())
    }
}

/// Identifies only the finite credential deadline, preserving transport errors separately.
pub(super) fn is_expired(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|cause| cause.is::<ExpiredCredential>())
}

#[derive(Debug, thiserror::Error)]
#[error("native multiplayer credential expired before Login")]
struct ExpiredCredential;

/// Omits token, identity, provider URL and key diagnostics.
pub(super) fn expired() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, ExpiredCredential)
}

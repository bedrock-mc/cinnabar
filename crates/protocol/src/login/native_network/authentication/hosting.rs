//! One provider-qualified host proof shared by SDP answers and the accepting Login key exchange.

use super::{NativeCredential, NativeTokenVerifier, messaging};
use nethernet::identity::{SdpIdentity, TokenTrust};
use p384::ecdsa::SigningKey;
use std::{io, sync::Arc, time::Instant};

/// The selected account's actual key-bound multiplayer mint for one accepting host incarnation.
/// No client-data object, alternate HTTP owner, runtime or replacement key is manufactured.
/// The application must stop new admission at the original expiry and retain its remote cleanup.
pub struct NativeHostingIdentity {
    key: Arc<SigningKey>,
    identity: Arc<dyn SdpIdentity>,
    trust: TokenTrust,
    expires: Instant,
}

impl NativeHostingIdentity {
    /// Verifies the original issuer signature, selected XUID, proof key and finite service-clock validity.
    ///
    /// # Errors
    /// Rejects a different account/proof key, untrusted provider credential, or elapsed original validity.
    pub fn new(
        key: Arc<SigningKey>,
        credential: NativeCredential,
        verifier: &NativeTokenVerifier,
        selected_xuid: &str,
        now: Instant,
    ) -> io::Result<Self> {
        credential.ensure_current(now)?;
        let (_, expires) = credential.qualify(key.verifying_key(), verifier, selected_xuid, now)?;
        let (identity, trust) = messaging::identity_policy(
            Arc::clone(&key),
            &credential.token,
            credential.clock,
            Arc::new(verifier.clone()),
            expires,
        )?;
        Ok(Self {
            key,
            identity,
            trust,
            expires,
        })
    }

    /// Shares the exact original allocation with the accepting Login owner, without copying private material.
    pub fn signing_key(&self) -> Arc<SigningKey> {
        Arc::clone(&self.key)
    }

    /// Shares the actual provider-token signer and its original calibrated expiry for SDP answers.
    pub fn sdp_identity(&self) -> Arc<dyn SdpIdentity> {
        Arc::clone(&self.identity)
    }

    /// Applies the same acquired provider and calibrated clock to present peer assertions.
    /// Anonymous-offer policy and cryptographic Login remain the application's separate decisions.
    pub fn offer_trust(&self) -> TokenTrust {
        self.trust.clone()
    }

    /// Returns the original hard monotonic admission deadline without renewing the minted lifetime.
    pub fn expires_at(&self) -> Instant {
        self.expires
    }
}

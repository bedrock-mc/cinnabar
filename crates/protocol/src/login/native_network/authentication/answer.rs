//! Strict present server assertions on an explicitly selected offline route.

use nethernet::identity::{TokenTrust, TokenVerifier, error::IdentityError, jwt::Claims};
use std::{sync::Arc, time::SystemTime};

/// Admits self-signed server proofs; no offline route supplies provider-token authority.
pub(in crate::login::native_network) fn offline_answer_trust() -> TokenTrust {
    TokenTrust::ServerAnswer(Arc::new(NoAccountProvider))
}

struct NoAccountProvider;

impl TokenVerifier for NoAccountProvider {
    /// A signed-out route cannot authenticate a provider-issued server token.
    fn verify(&self, _: &str, _: SystemTime) -> nethernet::identity::error::Result<Claims> {
        Err(IdentityError::Untrusted(
            "local server provider is not configured".into(),
        ))
    }
}

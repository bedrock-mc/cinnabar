//! The original Login proof and credential policy shared by signed direct transports.

use super::*;
use crate::login::native_network::transport::identity::TransportIdentity;
use nethernet::identity::{SelfSignedIdentity, TokenTrust};

impl Credentials {
    /// Retains one proof key for both SDP and Login without acquiring another account credential.
    pub(in crate::login::native_network) fn transport_identity(
        &self,
    ) -> io::Result<TransportIdentity> {
        if self.messaging.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "direct transport cannot consume a Messaging handoff",
            ));
        }
        match (&self.authentication, &self.sdp) {
            (Some((token, _, expires)), Some(policy)) => {
                let (identity, trust) = authentication::identity_policy(
                    Arc::clone(&self.key),
                    token,
                    policy.clock,
                    Arc::clone(&policy.verifier),
                    *expires,
                )?;
                let TokenTrust::Provider(provider) = trust else {
                    unreachable!("qualified identity policy retains its configured provider");
                };
                Ok(TransportIdentity {
                    identity,
                    trust: TokenTrust::ServerAnswer(provider),
                    expires: Some(*expires),
                })
            }
            (None, None) => Ok(TransportIdentity {
                identity: Arc::new(
                    SelfSignedIdentity::from_key(
                        Arc::clone(&self.key),
                        SystemTime::now(),
                        crate::NATIVE_JOIN_TIMEOUT,
                    )
                    .map_err(io::Error::other)?,
                ),
                trust: authentication::offline_answer_trust(),
                expires: None,
            }),
            _ => Err(io::Error::other(
                "direct transport credential policy is unavailable",
            )),
        }
    }
}

#[cfg(test)]
mod tests;

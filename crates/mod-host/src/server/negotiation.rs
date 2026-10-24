//! The registry confirmation and request for one component instance.

use server_experience::{runtime::Command, wire::Scalar};

/// A terminal disagreement between the client and server's shared definitions.
#[derive(Debug)]
pub struct RegistryMismatch;

impl std::fmt::Display for RegistryMismatch {
    /// Uses the same diagnostic as the SDK's registry guard.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(experience_sdk::REGISTRY_MISMATCH)
    }
}

impl std::error::Error for RegistryMismatch {}

pub(super) struct Negotiation {
    pub confirmation: String,
    pub request: Command,
}

impl Negotiation {
    /// Checks the server reply against the fingerprint in this instance's request.
    pub fn matches(&self, record: &[u8]) -> bool {
        let Command::Send {
            record: expected, ..
        } = &self.request
        else {
            unreachable!("registry negotiation stores a send command");
        };
        serde_json::from_slice::<Vec<Scalar>>(record).is_ok_and(|record| record == *expected)
    }

    /// Records the confirmation channel and the request to renew when the world changes.
    pub fn new(confirmation: String, request: Command) -> Self {
        Self {
            confirmation,
            request,
        }
    }
}

#[cfg(test)]
mod tests;

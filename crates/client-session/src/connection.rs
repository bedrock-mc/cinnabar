//! Generation-fenced join admission independent of the menu and transport owner.

use std::path::PathBuf;

/// A prepared join retained while the connecting screen's admission fences remain closed.
#[derive(Debug)]
pub struct JoinAttempt {
    pub generation: u64,
    pub address: String,
    pub auth_cache: Option<PathBuf>,
    pub local_world: bool,
    pub endpoint: protocol::SessionEndpoint,
}

impl JoinAttempt {
    /// Releases the original endpoint only while this same generation is still connecting.
    pub fn poll(
        self,
        current_generation: u64,
        connecting: bool,
    ) -> Option<protocol::SessionEndpoint> {
        (self.generation == current_generation && connecting).then_some(self.endpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A target description carries no connected session or account authority.
    fn attempt() -> JoinAttempt {
        JoinAttempt {
            generation: 7,
            address: "fixture.invalid:19132".into(),
            auth_cache: None,
            local_world: false,
            endpoint: protocol::SessionEndpoint::Native(protocol::NativeConnectionTarget::new(
                "fixture.invalid:19132".into(),
                protocol::NativeTransport::Addressed,
            )),
        }
    }

    #[test]
    fn retired_generation_does_not_start_transport() {
        assert!(attempt().poll(8, true).is_none());
        assert!(attempt().poll(7, false).is_none());
    }

    #[test]
    fn current_connecting_generation_retains_its_prepared_endpoint() {
        let endpoint = attempt().poll(7, true).expect("current join");
        let protocol::SessionEndpoint::Native(target) = endpoint;
        assert_eq!(target.address, "fixture.invalid:19132");
        assert_eq!(target.transport, protocol::NativeTransport::Addressed);
    }
}

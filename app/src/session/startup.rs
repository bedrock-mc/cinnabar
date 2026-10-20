//! Initial endpoints join through the same admission fence as menu connections.

use super::{JoinAttempt, JoinStage, SessionController};

impl SessionController {
    /// Retains the prepared endpoint and account until the ordinary mod admission fence opens.
    pub(crate) fn with_startup_endpoint(
        mut self,
        endpoint: Option<protocol::SessionEndpoint>,
        address: Option<&str>,
        #[cfg(feature = "native-networking")] account: Option<
            std::sync::Arc<launcher_host::native_account::NativeAccount>,
        >,
    ) -> Self {
        if let Some(endpoint) = endpoint {
            self.join = Some(JoinAttempt {
                generation: self.generation,
                address: address.unwrap_or_default().to_owned(),
                auth_cache: None,
                local_world: false,
                stage: JoinStage::Prepared(endpoint),
            });
            #[cfg(feature = "native-networking")]
            {
                self.startup_account = account;
            }
            self.connecting = true;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn startup_endpoints_remain_pending_until_the_join_owner_admits_them() {
        for address in [None, Some("fixture.example:19132")] {
            let endpoint = PathBuf::from("fixture-startup-endpoint");
            let mut controller = SessionController::default().with_startup_endpoint(
                Some(protocol::SessionEndpoint::Core(endpoint.clone())),
                address,
                #[cfg(feature = "native-networking")]
                None,
            );
            assert!(controller.status().connecting);
            let attempt = controller
                .join
                .take()
                .expect("startup must enter the join fence");
            assert_eq!(attempt.address, address.unwrap_or_default());
            assert!(matches!(
                attempt.poll(controller.generation, true, || panic!("endpoint is ready")),
                client_session::connection::JoinPoll::Ready { endpoint: protocol::SessionEndpoint::Core(socket_dir), directory: None }
                    if socket_dir == endpoint
            ));
        }
        let menu = SessionController::default().with_startup_endpoint(
            None,
            None,
            #[cfg(feature = "native-networking")]
            None,
        );
        assert!(menu.join.is_none());
        assert!(!menu.status().connecting);
    }
}

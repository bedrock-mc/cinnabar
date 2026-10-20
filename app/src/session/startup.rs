//! Initial endpoints join through the same admission fence as menu connections.

use super::{JoinAttempt, JoinStage, SessionController};
use std::path::PathBuf;

impl SessionController {
    /// Queues an already prepared startup endpoint through the ordinary mod admission fence.
    pub(crate) fn with_startup_endpoint(
        mut self,
        socket_dir: Option<PathBuf>,
        address: Option<&str>,
    ) -> Self {
        if let Some(socket_dir) = socket_dir {
            let (reply, ready) = crossbeam_channel::bounded(1);
            reply
                .send(Ok(socket_dir))
                .expect("new startup endpoint channel");
            self.join = Some(JoinAttempt {
                generation: self.generation,
                address: address.unwrap_or_default().to_owned(),
                auth_cache: None,
                local_world: false,
                stage: JoinStage::Launcher(ready),
            });
            self.connecting = true;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_endpoints_remain_pending_until_the_join_owner_admits_them() {
        for address in [None, Some("fixture.example:19132")] {
            let endpoint = PathBuf::from("fixture-startup-endpoint");
            let mut controller =
                SessionController::default().with_startup_endpoint(Some(endpoint.clone()), address);
            assert!(controller.status().connecting);
            let attempt = controller
                .join
                .take()
                .expect("startup must enter the join fence");
            assert_eq!(attempt.address, address.unwrap_or_default());
            assert!(matches!(
                attempt.poll(controller.generation, true, || panic!("endpoint is ready")),
                client_session::connection::JoinPoll::Ready { socket_dir, directory: None }
                    if socket_dir == endpoint
            ));
        }
        let menu = SessionController::default().with_startup_endpoint(None, None);
        assert!(menu.join.is_none());
        assert!(!menu.status().connecting);
    }
}

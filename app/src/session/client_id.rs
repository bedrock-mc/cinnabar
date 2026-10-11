//! Frame-safe polling of launcher-owned persistent login state.

use super::SessionController;
use launcher::install_layout::InstallLayout;
use protocol::ClientRandomId;
use std::sync::mpsc::TryRecvError;

impl SessionController {
    /// Retains an ID already loaded before direct-connection startup.
    pub(crate) fn with_client_random_id(mut self, id: Option<ClientRandomId>) -> Self {
        self.client_random_id = id;
        self
    }

    /// Starts at most one storage worker; later joins reuse its successful result.
    pub(super) fn prepare_client_random_id(
        &mut self,
        layout: &InstallLayout,
    ) -> Result<(), String> {
        if self.client_random_id.is_none() && self.client_id_load.is_none() {
            self.client_id_load = Some(
                layout
                    .load_client_random_id_async()
                    .map(std::sync::Mutex::new)
                    .map_err(|error| format!("Could not load login client ID: {error}"))?,
            );
        }
        Ok(())
    }

    /// Takes only a ready storage result; pending I/O never waits on the calling frame.
    pub(super) fn poll_client_random_id(&mut self) -> Result<Option<ClientRandomId>, String> {
        if let Some(id) = self.client_random_id {
            return Ok(Some(id));
        }
        let Some(receiver) = self.client_id_load.as_mut() else {
            return Err("Login client ID loading was not started".to_owned());
        };
        let receiver = receiver
            .get_mut()
            .expect("only the session owner accesses the storage receiver");
        let result = match receiver.try_recv() {
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) => Err(std::io::Error::other("storage worker stopped")),
            Ok(result) => result,
        };
        self.client_id_load = None;
        let id = result.map_err(|error| format!("Could not load login client ID: {error}"))?;
        self.client_random_id = Some(id);
        Ok(Some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn pending_storage_does_not_wait_and_ready_id_survives_join_replacement() {
        let mut controller = SessionController::default();
        let (sender, receiver) = mpsc::channel();
        controller.client_id_load = Some(std::sync::Mutex::new(receiver));
        assert_eq!(controller.poll_client_random_id().unwrap(), None);
        let id = ClientRandomId::new(i64::MIN).unwrap();
        sender.send(Ok(id)).unwrap();
        assert_eq!(controller.poll_client_random_id().unwrap(), Some(id));
        controller.next_generation();
        assert_eq!(controller.poll_client_random_id().unwrap(), Some(id));
        assert!(controller.client_id_load.is_none());
    }

    #[test]
    fn failed_storage_does_not_publish_an_id_and_allows_another_attempt() {
        let mut controller = SessionController::default();
        let (sender, receiver) = mpsc::channel();
        controller.client_id_load = Some(std::sync::Mutex::new(receiver));
        sender
            .send(Err(std::io::Error::other("write failed")))
            .unwrap();
        assert!(
            controller
                .poll_client_random_id()
                .unwrap_err()
                .contains("write failed")
        );
        assert!(controller.client_random_id.is_none());
        assert!(controller.client_id_load.is_none());
        let (sender, receiver) = mpsc::channel();
        controller.client_id_load = Some(std::sync::Mutex::new(receiver));
        let id = ClientRandomId::new(-7).unwrap();
        sender.send(Ok(id)).unwrap();
        assert_eq!(controller.poll_client_random_id().unwrap(), Some(id));
    }
}

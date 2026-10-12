//! Joins through a web relay, which dials the server itself, as a browser has no core of its own.

use std::sync::Arc;

use super::{BlobCache, Commands, MenuRuntime, SessionController, SessionResources};

/// The relay a browser's sessions join through, and a join waiting to open on it.
#[derive(Default)]
pub(super) struct RelayLink {
    sessions: Option<Arc<dyn bridge::RelaySessions>>,
    pub(super) pending_join: Option<String>,
}

impl std::fmt::Debug for RelayLink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RelayLink")
            .field("relay", &self.sessions.is_some())
            .field("pending_join", &self.pending_join)
            .finish()
    }
}

impl SessionController {
    /// Joins every session through the relay `sessions` issue, rather than a core.
    pub(crate) fn with_relay(mut self, sessions: Option<Arc<dyn bridge::RelaySessions>>) -> Self {
        self.relay.sessions = sessions;
        self
    }
}

/// Holds a remote join for the relay, which dials the server itself, so the session opens on
/// a later poll; returns the address when the join goes through a core instead.
pub(super) fn defer_join(
    controller: &mut SessionController,
    menu: &mut MenuRuntime,
    address: String,
    local_world: bool,
) -> Option<String> {
    if controller.relay.sessions.is_none() || local_world {
        return Some(address);
    }
    if let Err(message) = controller.prepare_client_random_id(menu.layout()) {
        controller.fail_join(menu, message);
        return None;
    }
    controller.relay.pending_join = Some(address);
    controller.connecting = true;
    menu.show_connecting();
    None
}

/// Opens the session of a join waiting on the relay once the relay has issued a session and
/// the login ID is ready; returns whether a relay join owned this poll, so the core join path
/// is skipped.
pub(super) fn poll_join(
    commands: &mut Commands,
    menu: &mut MenuRuntime,
    session: &mut SessionResources<'_>,
    cache: &BlobCache,
) -> bool {
    let controller = &mut *session.controller;
    let (Some(address), Some(sessions)) = (
        controller.relay.pending_join.take(),
        controller.relay.sessions.clone(),
    ) else {
        return false;
    };
    let Some(relay) = sessions.session() else {
        controller.relay.pending_join = Some(address);
        return true;
    };
    let client_random_id = match controller.poll_client_random_id() {
        Ok(Some(id)) => id,
        Ok(None) => {
            controller.relay.pending_join = Some(address);
            return true;
        }
        Err(message) => {
            controller.fail_join(menu, message);
            return true;
        }
    };
    // The relay dials addressed servers only, and only with a port.
    let target = match launcher_host::launcher_core::target_for(&address) {
        target @ bridge::ConnectTarget::RakNet(_) => target,
        _ => {
            controller.fail_join(
                menu,
                "Realms, friends' worlds and featured experiences can't be joined from a browser yet."
                    .to_owned(),
            );
            return true;
        }
    };
    let login_settings = super::login_settings(menu, session.input.as_deref());
    let endpoint = (bridge::CoreEndpoint::Relay(relay), Some(target));
    if let Err(error) = super::start_network(
        commands,
        menu,
        controller.generation,
        cache,
        endpoint,
        login_settings,
        client_random_id,
        session.actor_artwork.as_deref().map(|artwork| &artwork.0),
        session.ui_catalog.as_deref(),
    ) {
        controller.fail_join(menu, format!("Could not connect: {error}"));
    }
    true
}

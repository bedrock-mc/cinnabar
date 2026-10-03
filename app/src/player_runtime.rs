//! App orchestration owner for inventory and committed local-player facts.

use bevy::prelude::Resource;

/// Domain authority shared synchronously by network input, UI commands and movement.
#[derive(Clone, Debug, Resource)]
pub struct PlayerRuntime {
    pub inventory: inventory::InventorySession,
    pub facts: client_world::LocalPlayerFacts,
}

impl PlayerRuntime {
    /// Starts both domain owners at the same session generation.
    pub fn new(session: u64) -> Self {
        Self {
            inventory: inventory::InventorySession::new(session),
            facts: client_world::LocalPlayerFacts::new(session),
        }
    }

    /// Retires both owners together when the network session changes.
    pub fn begin_session(&mut self, session: u64) {
        self.inventory.begin_session(session);
        self.facts.begin_session(session);
    }
}

impl From<inventory::InventoryIngressError> for crate::ui_runtime::UiRuntimeError {
    /// Preserves the app's existing admission error reporting.
    fn from(error: inventory::InventoryIngressError) -> Self {
        match error {
            inventory::InventoryIngressError::WrongSession { expected, actual } => {
                Self::WrongSession { expected, actual }
            }
            inventory::InventoryIngressError::StaleFifoSequence { previous, actual } => {
                Self::StaleFifoSequence { previous, actual }
            }
            inventory::InventoryIngressError::InventoryQueueFull { maximum } => {
                Self::InventoryQueueFull { maximum }
            }
        }
    }
}

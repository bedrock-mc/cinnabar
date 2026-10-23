//! Data-driven UI: the server's synced data stores and the answers its screen requests receive.

use protocol::ddui::{
    DataDrivenScreenCloseReason, DataDrivenUiEvent, DataStores, StoreApply,
    data_driven_screen_closed_packet,
};

use super::{UiApplyOutcome, UiRuntime};

/// The session's DDUI data stores.
#[derive(Debug, Clone, Default)]
pub struct DataDrivenUi {
    stores: DataStores,
    ignored_store_changes: u64,
}

impl DataDrivenUi {
    /// The synced data stores the screens read.
    #[must_use]
    pub fn stores(&self) -> &DataStores {
        &self.stores
    }

    /// Server store changes skipped because they named no known property, were written against
    /// another replacement of their property, or did not follow a path inside it.
    #[must_use]
    pub fn ignored_store_changes(&self) -> u64 {
        self.ignored_store_changes
    }

    pub(super) fn clear(&mut self) {
        self.stores.clear();
    }
}

impl UiRuntime {
    /// Applies a DDUI event. `busy` is true while another screen holds the player's attention.
    ///
    /// Every screen request is answered so the server's script can finish: a busy client reports
    /// `UserBusy`, and otherwise the screen is closed at once because no DDUI renderer exists yet.
    pub(super) fn apply_data_driven_ui(
        &mut self,
        event: DataDrivenUiEvent,
        busy: bool,
    ) -> UiApplyOutcome {
        match event {
            DataDrivenUiEvent::Store(changes) => {
                for change in changes {
                    let outcome = self.data_driven.stores.apply(change);
                    if outcome != StoreApply::Applied {
                        self.data_driven.ignored_store_changes += 1;
                        tracing::debug!(target: "ddui", ?outcome, "data store change skipped");
                    }
                }
            }
            DataDrivenUiEvent::Show(show) => {
                let reason = if busy {
                    DataDrivenScreenCloseReason::UserBusy
                } else {
                    DataDrivenScreenCloseReason::ClientCanceled
                };
                tracing::info!(
                    target: "ddui",
                    screen = %show.screen_id,
                    form_id = show.form_id,
                    ?reason,
                    "data-driven screen declined"
                );
                if !self.queue_client_packet(data_driven_screen_closed_packet(show.form_id, reason))
                {
                    tracing::warn!(target: "ddui", form_id = show.form_id, "screen answer dropped");
                }
            }
            // No screen is ever showing, so a close has nothing to close and needs no answer.
            DataDrivenUiEvent::Close { .. } => {}
            // The client loads no DDUI definitions, so a reload changes nothing.
            DataDrivenUiEvent::Reload => {}
        }
        UiApplyOutcome::Applied
    }
}

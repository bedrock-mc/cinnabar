//! The display state that outbound producers may change within a frame.
use super::{PlayerInventoryLedger, UiRuntime, forms::ServerFormStore};

pub(super) struct PresentationInventory {
    ledger: PlayerInventoryLedger,
    forms: ServerFormStore,
    open: bool,
    pointer: Option<[f32; 2]>,
}

impl PresentationInventory {
    /// Exchanges only the render-visible fields written by outbound producers.
    fn swap(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        runtime: &mut UiRuntime,
    ) {
        std::mem::swap(&mut self.forms, &mut runtime.forms);
        std::mem::swap(&mut self.ledger, player_runtime.inventory.ledger_mut());
        std::mem::swap(&mut self.open, &mut runtime.inventory_open);
        std::mem::swap(&mut self.pointer, &mut runtime.inventory_pointer_gui);
    }
}

struct RestoreInventory<'a> {
    runtime: &'a mut UiRuntime,
    player_runtime: &'a mut crate::player_runtime::PlayerRuntime,
    after_send: PresentationInventory,
}

impl Drop for RestoreInventory<'_> {
    /// Restores authoritative post-send state even if rendering returns early or panics.
    fn drop(&mut self) {
        self.after_send.swap(self.player_runtime, self.runtime);
    }
}

impl UiRuntime {
    /// Captures inventory display state, sharing immutable item and creative catalogs.
    pub(super) fn capture_presentation_inventory(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
    ) -> PresentationInventory {
        PresentationInventory {
            ledger: player_runtime.inventory.ledger().clone(),
            forms: self.forms.clone(),
            open: self.inventory_open,
            pointer: self.inventory_pointer_gui,
        }
    }

    /// Gives rendering a read-only view of the pre-send inventory for this call alone.
    pub(super) fn with_presentation_inventory<T>(
        &mut self,
        player_runtime: &mut crate::player_runtime::PlayerRuntime,
        mut snapshot: PresentationInventory,
        render: impl FnOnce(&UiRuntime, &crate::player_runtime::PlayerRuntime) -> T,
    ) -> T {
        snapshot.swap(player_runtime, self);
        let restore = RestoreInventory {
            runtime: self,
            player_runtime,
            after_send: snapshot,
        };
        render(restore.runtime, restore.player_runtime)
    }
}

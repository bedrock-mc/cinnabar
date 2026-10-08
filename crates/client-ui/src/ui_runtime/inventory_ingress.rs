use protocol::{InventoryEvent, ItemRegistryEvent};

use super::{UiRuntime, UiRuntimeError};

pub use inventory::{InventoryAuthorityEvent, SequencedInventoryEvent};

impl UiRuntime {
    pub fn enqueue_inventory_event(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        session_generation: u64,
        fifo_sequence: u64,
        event: InventoryEvent,
    ) -> Result<(), UiRuntimeError> {
        player_runtime
            .inventory
            .enqueue_inventory_event(session_generation, fifo_sequence, event)
            .map_err(UiRuntimeError::from)
    }

    pub fn enqueue_item_registry_event(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        session_generation: u64,
        fifo_sequence: u64,
        event: ItemRegistryEvent,
    ) -> Result<(), UiRuntimeError> {
        player_runtime
            .inventory
            .enqueue_item_registry_event(session_generation, fifo_sequence, event)
            .map_err(UiRuntimeError::from)
    }

    pub fn pop_inventory_event(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
    ) -> Option<SequencedInventoryEvent> {
        player_runtime.inventory.pop_inventory_event()
    }

    pub fn synchronize_crafting_frontier(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        session: u64,
        identity: Option<(u64, u64, Option<u64>)>,
    ) {
        player_runtime
            .inventory
            .synchronize_crafting_frontier(session, identity)
    }

    pub fn publish_crafting_bootstrap(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        registry: Option<&ItemRegistryEvent>,
        authority: protocol::InventoryAuthority,
    ) {
        player_runtime
            .inventory
            .publish_crafting_bootstrap(registry, authority)
    }

    /// Borrowed display values only, with immutable credit owners retained by this runtime.
    /// Inactive crafting authority never allocates a request or sends a packet.
    pub fn crafting_preview<'a>(
        &self,
        player_runtime: &'a player_state::PlayerState,
    ) -> Option<super::CraftingPreview<'a>> {
        player_runtime.inventory.crafting_preview()
    }

    /// The recipe the presented crafting grid forms against the committed
    /// catalog; `Unavailable` without a catalog or known grid identities.
    #[must_use]
    pub fn crafting_match(
        &self,
        player_runtime: &player_state::PlayerState,
    ) -> inventory::CraftGridMatch {
        player_runtime.inventory.crafting_match()
    }

    /// Crafts the grid's unique recipe once into the cursor.
    pub fn begin_crafting(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
    ) -> Result<i32, super::inventory_ledger::InventoryGestureError> {
        player_runtime.inventory.begin_crafting()
    }

    /// Crafts the grid's unique recipe once into `sink`.
    pub fn begin_crafting_into(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        sink: super::inventory_ledger::CraftSink,
    ) -> Result<i32, super::inventory_ledger::InventoryGestureError> {
        player_runtime.inventory.begin_crafting_into(sink)
    }

    /// Crafts the grid's unique recipe as many times as it fits, shift-click style.
    pub fn begin_crafting_all(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
    ) -> Result<i32, super::inventory_ledger::InventoryGestureError> {
        player_runtime.inventory.begin_crafting_all()
    }
}

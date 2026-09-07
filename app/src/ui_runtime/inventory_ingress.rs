use protocol::{InventoryEvent, ItemRegistryEvent};

use super::{MAX_PENDING_INVENTORY_EVENTS, UiRuntime, UiRuntimeError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedInventoryEvent {
    pub session_generation: u64,
    pub fifo_sequence: u64,
    pub event: InventoryAuthorityEvent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryAuthorityEvent {
    Inventory(InventoryEvent),
    Registry(ItemRegistryEvent),
}

impl UiRuntime {
    pub(crate) fn enqueue_inventory_event(
        &mut self,
        session_generation: u64,
        fifo_sequence: u64,
        event: InventoryEvent,
    ) -> Result<(), UiRuntimeError> {
        self.enqueue_inventory_authority_event(
            session_generation,
            fifo_sequence,
            InventoryAuthorityEvent::Inventory(event),
        )
    }

    pub(crate) fn enqueue_item_registry_event(
        &mut self,
        session_generation: u64,
        fifo_sequence: u64,
        event: ItemRegistryEvent,
    ) -> Result<(), UiRuntimeError> {
        self.enqueue_inventory_authority_event(
            session_generation,
            fifo_sequence,
            InventoryAuthorityEvent::Registry(event),
        )
    }

    fn enqueue_inventory_authority_event(
        &mut self,
        session_generation: u64,
        fifo_sequence: u64,
        event: InventoryAuthorityEvent,
    ) -> Result<(), UiRuntimeError> {
        if session_generation != self.session_id {
            return Err(UiRuntimeError::WrongSession {
                expected: self.session_id,
                actual: session_generation,
            });
        }
        if let Some(previous) = self.last_inventory_sequence
            && fifo_sequence <= previous
        {
            return Err(UiRuntimeError::StaleFifoSequence {
                previous,
                actual: fifo_sequence,
            });
        }
        if self.pending_inventory.len() >= MAX_PENDING_INVENTORY_EVENTS {
            return Err(UiRuntimeError::InventoryQueueFull {
                maximum: MAX_PENDING_INVENTORY_EVENTS,
            });
        }
        self.pending_inventory.push_back(SequencedInventoryEvent {
            session_generation,
            fifo_sequence,
            event,
        });
        self.last_inventory_sequence = Some(fifo_sequence);
        Ok(())
    }

    pub fn pop_inventory_event(&mut self) -> Option<SequencedInventoryEvent> {
        self.pending_inventory.pop_front()
    }
}

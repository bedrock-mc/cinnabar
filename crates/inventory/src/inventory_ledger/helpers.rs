use protocol::{StackRequestAction, StackRequestContainer, StackRequestSlot};

use super::{Cell, ContainerIdentity, InventoryGestureError, PlayerInventoryLedger};

impl PlayerInventoryLedger {
    /// Binds admitted storage contents to the retained model's canonical request address.
    pub(super) fn bind_storage_request_identity(&mut self) {
        let identity = self.storage_request_identity();
        if let Some(storage) = &mut self.storage {
            storage.identity = identity;
        }
    }

    /// Uses the screen model's outbound address, independently of ignored packet descriptors.
    pub(super) fn storage_request_identity(&self) -> Option<ContainerIdentity> {
        let storage = self.storage.as_ref()?;
        let (container, _) = protocol::open_cell_request(storage.kind, 0, None, None)?;
        let name =
            if protocol::open_name_first_cell(storage.kind, protocol::CONTAINER_NAME_LEVEL_ENTITY)
                .is_some()
            {
                protocol::CONTAINER_NAME_LEVEL_ENTITY
            } else {
                match container {
                    StackRequestContainer::LevelEntity { .. } => {
                        protocol::CONTAINER_NAME_LEVEL_ENTITY
                    }
                    StackRequestContainer::OpenWindow { name, .. } => name,
                    _ => return None,
                }
            };
        Some(ContainerIdentity {
            window_id: Some(storage.window_id),
            slot_type: Some(name),
            dynamic_id: None,
        })
    }

    /// Even a predicted empty cell carries the owning request id in vanilla's
    /// sparse container. Bind both source and destination before the new write
    /// replaces that ownership.
    pub(super) fn bind_request_dependencies(
        &self,
        actions: &mut [StackRequestAction],
        request_id: i32,
    ) {
        for action in actions {
            for slot in action_slots_mut(action) {
                if slot.stack_network_id == request_id {
                    continue;
                }
                let Some(cell) = self.request_cell(*slot) else {
                    continue;
                };
                if let Some(owner) = self.queue.iter().rev().find(|pending| {
                    !pending.locally_committed()
                        && pending
                            .predicted
                            .iter()
                            .any(|prediction| prediction.cell == cell && prediction.active)
                }) {
                    slot.stack_network_id = owner.request_id;
                }
            }
        }
    }

    pub(super) fn request_cell(&self, slot: StackRequestSlot) -> Option<Cell> {
        Some(match slot.container {
            StackRequestContainer::PlayerInventory | StackRequestContainer::CombinedInventory => {
                Cell::Inventory(slot.slot)
            }
            StackRequestContainer::Cursor => Cell::Cursor,
            StackRequestContainer::Armor => Cell::Armor(slot.slot),
            StackRequestContainer::Offhand => Cell::Offhand,
            StackRequestContainer::CraftingInput => Cell::Craft(slot.slot),
            StackRequestContainer::CreatedOutput => Cell::CreatedOutput,
            StackRequestContainer::LevelEntity { .. } => Cell::Storage(slot.slot),
            StackRequestContainer::OpenWindow { name, dynamic_id } => {
                return self.retained_response_cell(
                    &ContainerIdentity {
                        window_id: None,
                        slot_type: Some(name),
                        dynamic_id,
                    },
                    u16::from(slot.slot),
                );
            }
        })
    }
}

/// The source and destination slots `action` names.
pub(super) fn action_slots_mut(
    action: &mut StackRequestAction,
) -> impl Iterator<Item = &mut StackRequestSlot> {
    let (first, second) = match action {
        StackRequestAction::Take {
            source,
            destination,
            ..
        }
        | StackRequestAction::Place {
            source,
            destination,
            ..
        }
        | StackRequestAction::Swap {
            source,
            destination,
        } => (Some(source), Some(destination)),
        StackRequestAction::Drop { source, .. }
        | StackRequestAction::Destroy { source, .. }
        | StackRequestAction::Consume { source, .. } => (Some(source), None),
        _ => (None, None),
    };
    first.into_iter().chain(second)
}

pub(super) const fn valid_raw_window_id(window_id: i32) -> bool {
    matches!(window_id, -128..=255)
}

pub(super) const fn valid_storage_window_id(window_id: i32) -> bool {
    window_id != 0 && valid_raw_window_id(window_id)
}

/// How the open window's own cells are named in a request.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct WindowAddress {
    pub(super) identity: ContainerIdentity,
    pub(super) kind: protocol::WindowKind,
}

/// `request_slot` for a player cell under the combined hotbar-and-inventory name.
pub(super) fn combined_request_slot(
    slot: u8,
    stack_network_id: i32,
) -> Result<StackRequestSlot, InventoryGestureError> {
    request_slot(Cell::Inventory(slot), stack_network_id, None).map(|slot| StackRequestSlot {
        container: StackRequestContainer::CombinedInventory,
        ..slot
    })
}

pub(super) fn request_slot(
    cell: Cell,
    stack_network_id: i32,
    window: Option<WindowAddress>,
) -> Result<StackRequestSlot, InventoryGestureError> {
    let (container, slot) = match cell {
        Cell::Inventory(slot) => (StackRequestContainer::PlayerInventory, slot),
        Cell::Cursor => (StackRequestContainer::Cursor, 0),
        Cell::Storage(slot) => {
            let window = window.ok_or(InventoryGestureError::InvalidRequest)?;
            protocol::open_cell_request(
                window.kind,
                slot,
                window.identity.dynamic_id,
                window.identity.slot_type,
            )
            .ok_or(InventoryGestureError::InvalidRequest)?
        }
        Cell::Armor(slot) => (StackRequestContainer::Armor, slot),
        Cell::Offhand => (StackRequestContainer::Offhand, 1),
        Cell::Craft(slot) => (
            protocol::ui_slot_request_container(slot)
                .ok_or(InventoryGestureError::InvalidRequest)?,
            slot,
        ),
        Cell::CreatedOutput => (
            StackRequestContainer::CreatedOutput,
            protocol::CREATED_OUTPUT_SLOT,
        ),
    };
    Ok(StackRequestSlot {
        container,
        slot,
        stack_network_id,
    })
}

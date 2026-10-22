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

    /// Names a chest-like window after the block entity it opened on: barrels and shulker
    /// boxes share the generic window type, but servers validate their own container names.
    pub fn name_storage_block(&mut self, position: [i32; 3], block_entity: Option<&str>) {
        let name = match block_entity {
            Some("Barrel") => Some(protocol_ids::ContainerName::Barrel.wire_value()),
            Some("ShulkerBox") => Some(protocol_ids::ContainerName::ShulkerBox.wire_value()),
            _ => None,
        };
        let Some(storage) = self
            .storage
            .as_mut()
            .filter(|storage| storage.position == position && storage.actor_unique_id == -1)
        else {
            return;
        };
        if storage.block_container == name {
            return;
        }
        storage.block_container = name;
        let Some(previous) = storage.identity else {
            return;
        };
        self.bind_storage_request_identity();
        // Requests already queued keep answering under the address they were sent with.
        let storage = self.storage.as_mut().expect("storage observed above");
        if storage.identity != Some(previous) {
            storage.superseded_identity = Some(previous);
        }
    }

    /// Uses the screen model's outbound address, independently of ignored packet descriptors.
    pub(super) fn storage_request_identity(&self) -> Option<ContainerIdentity> {
        let storage = self.storage.as_ref()?;
        let (container, _) = protocol::open_cell_request(storage.kind, 0, None, None)?;
        let block = storage
            .block_container
            .filter(|name| protocol::open_name_first_cell(storage.kind, *name).is_some());
        let name = if let Some(name) = block {
            name
        } else if protocol::open_name_first_cell(
            storage.kind,
            protocol::CONTAINER_NAME_LEVEL_ENTITY,
        )
        .is_some()
        {
            protocol::CONTAINER_NAME_LEVEL_ENTITY
        } else {
            match container {
                StackRequestContainer::LevelEntity { .. } => protocol::CONTAINER_NAME_LEVEL_ENTITY,
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
        self.rebind_actions_in_order(
            actions,
            request_id,
            |cell| {
                self.queue
                    .iter()
                    .rev()
                    .find(|pending| {
                        !pending.locally_committed()
                            && pending
                                .predicted
                                .iter()
                                .any(|prediction| prediction.cell == cell && prediction.active)
                    })
                    .map(|owner| owner.request_id)
            },
            |cell| self.view().get(cell).map(|held| held.stack.count),
        );
    }

    /// Rebinds `actions`' slots in order: `bind` gives a cell's stack id (`None` keeps the slot's
    /// own), except that a cell an earlier Consume in the same request emptied is addressed as
    /// empty. `count` is a cell's count before the request; slots naming `request_id` stay.
    pub(super) fn rebind_actions_in_order(
        &self,
        actions: &mut [StackRequestAction],
        request_id: i32,
        bind: impl Fn(Cell) -> Option<i32>,
        count: impl Fn(Cell) -> Option<u16>,
    ) {
        let mut emptied = Vec::new();
        for action in actions {
            let consumed = match action {
                StackRequestAction::Consume { amount, source } => Some((*amount, *source)),
                _ => None,
            };
            for slot in action_slots_mut(action) {
                if slot.stack_network_id == request_id {
                    continue;
                }
                let Some(cell) = self.request_cell(*slot) else {
                    continue;
                };
                if emptied.contains(&cell) {
                    slot.stack_network_id = 0;
                } else if let Some(id) = bind(cell) {
                    slot.stack_network_id = id;
                }
            }
            if let Some((amount, source)) = consumed
                && let Some(cell) = self.request_cell(source)
                && count(cell).is_some_and(|count| count <= u16::from(amount))
            {
                emptied.push(cell);
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

use protocol::{StackRequestAction, StackRequestContainer, StackRequestSlot};

use super::{Cell, ContainerIdentity, InventoryGestureError, PlayerInventoryLedger, StorageWindow};

impl PlayerInventoryLedger {
    /// A legacy full-container descriptor can be zero/default on a server chest.
    /// Resolve it only against the already-open generic window; zero remains a
    /// real screen-input name everywhere else, and dynamic names stay distinct.
    pub(super) fn storage_wire_identity(&self, identity: ContainerIdentity) -> ContainerIdentity {
        if matches!(identity.slot_type, None | Some(0))
            && identity.dynamic_id.is_none()
            && self.storage.as_ref().is_some_and(|storage| {
                identity.window_id == Some(storage.window_id)
                    && matches!(
                        storage.kind.open_cells(),
                        Some(protocol::OpenCells::Generic(_))
                    )
            })
        {
            ContainerIdentity {
                slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
                ..identity
            }
        } else {
            identity
        }
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
            for slot in first.into_iter().chain(second) {
                if slot.stack_network_id == request_id {
                    continue;
                }
                let Some(cell) = self.request_cell(*slot) else {
                    continue;
                };
                if let Some(owner) = self.queue.iter().rev().find(|pending| {
                    pending
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
            StackRequestContainer::PlayerInventory => Cell::Inventory(slot.slot),
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

pub(super) const fn valid_raw_window_id(window_id: i32) -> bool {
    matches!(window_id, -128..=255)
}

pub(super) const fn valid_storage_window_id(window_id: i32) -> bool {
    window_id != 0 && valid_raw_window_id(window_id)
}

pub(super) fn storage_slot_identity_matches(
    storage: &StorageWindow,
    identity: ContainerIdentity,
) -> bool {
    if identity.window_id != Some(storage.window_id) {
        return false;
    }
    match (identity.slot_type, identity.dynamic_id) {
        (None, None) => true,
        _ => storage.identity == Some(identity),
    }
}

/// Whether one container identity the projection left unrouted is exactly the
/// prior bare-window storage leg: no decoded container name, no dynamic id,
/// and a window id matching the one open generic-storage window. Named or
/// dynamic identities never qualify — they must project canonically first.
pub(super) fn bare_storage_window_matches(
    storage: Option<&StorageWindow>,
    identity: &ContainerIdentity,
) -> bool {
    identity.slot_type.is_none()
        && identity.dynamic_id.is_none()
        && storage.is_some_and(|storage| storage_slot_identity_matches(storage, *identity))
}

/// How the open window's own cells are named in a request.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct WindowAddress {
    pub(super) identity: ContainerIdentity,
    pub(super) kind: protocol::WindowKind,
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

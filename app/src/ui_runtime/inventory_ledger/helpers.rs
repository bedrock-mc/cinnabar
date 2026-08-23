use super::{
    Cell, CellSurface, ContainerIdentity, InventoryGestureError, StackRequestContainer,
    StackRequestSlot, StorageWindow,
};

pub(super) const fn cell_surface(cell: Cell) -> CellSurface {
    match cell {
        Cell::Inventory(_) => CellSurface::Player,
        Cell::Storage(_) => CellSurface::Storage,
        Cell::Cursor => CellSurface::Cursor,
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

pub(super) fn request_slot(
    cell: Cell,
    stack_network_id: i32,
    storage_identity: Option<ContainerIdentity>,
) -> Result<StackRequestSlot, InventoryGestureError> {
    Ok(match cell {
        Cell::Inventory(slot) => StackRequestSlot {
            container: StackRequestContainer::PlayerInventory,
            slot,
            stack_network_id,
        },
        Cell::Cursor => StackRequestSlot {
            container: StackRequestContainer::Cursor,
            slot: 0,
            stack_network_id,
        },
        Cell::Storage(slot) => StackRequestSlot {
            container: StackRequestContainer::LevelEntity {
                dynamic_id: storage_identity
                    .ok_or(InventoryGestureError::InvalidRequest)?
                    .dynamic_id,
            },
            slot,
            stack_network_id,
        },
    })
}

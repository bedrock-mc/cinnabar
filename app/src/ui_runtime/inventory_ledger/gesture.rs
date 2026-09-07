use protocol::{ContainerIdentity, NetworkItemStack, StackRequestAction};

use super::helpers::request_slot;
use super::registry::OccupiedStackRelation;
use super::{
    Cell, InventoryGestureError, InventoryPendingState, PLAYER_INVENTORY_SLOT_COUNT,
    PendingRequest, PlayerInventoryLedger, Prediction, StackResponseOverlay,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CellGesture {
    Click,
    TakeCount(u16),
    PlaceCount(u16),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum StackRequestActionKind {
    Take,
    Place,
}

#[allow(clippy::too_many_arguments)]
fn counted_transfer(
    kind: StackRequestActionKind,
    source: Cell,
    destination: Cell,
    stack: NetworkItemStack,
    overlay: Option<StackResponseOverlay>,
    source_revision: u64,
    destination_revision: u64,
    storage_identity: Option<ContainerIdentity>,
    requested_amount: Option<u16>,
) -> Result<(StackRequestAction, Prediction), InventoryGestureError> {
    let amount = requested_amount.unwrap_or(stack.count);
    let wire_amount = u8::try_from(amount)
        .ok()
        .filter(|amount| *amount != 0)
        .filter(|amount| u16::from(*amount) <= stack.count)
        .ok_or(InventoryGestureError::InvalidRequest)?;
    let partial = amount < stack.count;
    let mut transferred = stack.clone();
    transferred.count = amount;
    let residual = partial.then(|| {
        let mut residual = stack.clone();
        residual.count -= amount;
        residual
    });
    let source_slot = request_slot(source, stack.stack_network_id, storage_identity)?;
    let destination_slot = request_slot(destination, 0, storage_identity)?;
    let action = match kind {
        StackRequestActionKind::Take => StackRequestAction::Take {
            amount: wire_amount,
            source: source_slot,
            destination: destination_slot,
        },
        StackRequestActionKind::Place => StackRequestAction::Place {
            amount: wire_amount,
            source: source_slot,
            destination: destination_slot,
        },
    };
    Ok((
        action,
        Prediction {
            source,
            source_stack: residual,
            source_revision,
            destination,
            destination_stack: Some(transferred),
            destination_revision,
            source_overlay: if partial { overlay.clone() } else { None },
            destination_overlay: overlay,
            requires_distinct_stack_ids: partial,
            registry_bound_merge: false,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn counted_merge(
    kind: StackRequestActionKind,
    source: Cell,
    destination: Cell,
    source_stack: NetworkItemStack,
    destination_stack: NetworkItemStack,
    source_overlay: Option<StackResponseOverlay>,
    destination_overlay: Option<StackResponseOverlay>,
    source_revision: u64,
    destination_revision: u64,
    storage_identity: Option<ContainerIdentity>,
    amount: u16,
    capacity: u16,
) -> Result<(StackRequestAction, Prediction), InventoryGestureError> {
    let wire_amount = u8::try_from(amount)
        .ok()
        .filter(|amount| *amount != 0)
        .filter(|amount| u16::from(*amount) <= source_stack.count)
        .ok_or(InventoryGestureError::InvalidRequest)?;
    let destination_count = destination_stack
        .count
        .checked_add(amount)
        .filter(|count| *count <= capacity)
        .ok_or(InventoryGestureError::InvalidRequest)?;
    let partial = amount < source_stack.count;
    let residual = partial.then(|| {
        let mut residual = source_stack.clone();
        residual.count -= amount;
        residual
    });
    let mut merged = destination_stack.clone();
    merged.count = destination_count;
    let source_slot = request_slot(source, source_stack.stack_network_id, storage_identity)?;
    let destination_slot = request_slot(
        destination,
        destination_stack.stack_network_id,
        storage_identity,
    )?;
    let action = match kind {
        StackRequestActionKind::Take => StackRequestAction::Take {
            amount: wire_amount,
            source: source_slot,
            destination: destination_slot,
        },
        StackRequestActionKind::Place => StackRequestAction::Place {
            amount: wire_amount,
            source: source_slot,
            destination: destination_slot,
        },
    };
    Ok((
        action,
        Prediction {
            source,
            source_stack: residual,
            source_revision,
            destination,
            destination_stack: Some(merged),
            destination_revision,
            source_overlay: partial.then_some(source_overlay).flatten(),
            destination_overlay,
            requires_distinct_stack_ids: partial,
            registry_bound_merge: true,
        },
    ))
}

fn has_meaningful_overlay(overlay: Option<&StackResponseOverlay>) -> bool {
    overlay.is_some_and(|overlay| {
        overlay.custom_name.is_some()
            || overlay.filtered_custom_name.is_some()
            || overlay.durability_correction.is_some()
    })
}

impl PlayerInventoryLedger {
    pub fn begin_click(&mut self, slot: u8) -> Result<i32, InventoryGestureError> {
        self.begin_cell_gesture(Cell::Inventory(slot), CellGesture::Click)
    }

    pub fn begin_storage_click(&mut self, slot: u8) -> Result<i32, InventoryGestureError> {
        self.begin_cell_gesture(Cell::Storage(slot), CellGesture::Click)
    }

    pub fn begin_take_count(
        &mut self,
        slot: u8,
        amount: u16,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_cell_gesture(Cell::Inventory(slot), CellGesture::TakeCount(amount))
    }

    pub fn begin_place_count(
        &mut self,
        slot: u8,
        amount: u16,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_cell_gesture(Cell::Inventory(slot), CellGesture::PlaceCount(amount))
    }

    pub fn begin_storage_take_count(
        &mut self,
        slot: u8,
        amount: u16,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_cell_gesture(Cell::Storage(slot), CellGesture::TakeCount(amount))
    }

    pub fn begin_storage_place_count(
        &mut self,
        slot: u8,
        amount: u16,
    ) -> Result<i32, InventoryGestureError> {
        self.begin_cell_gesture(Cell::Storage(slot), CellGesture::PlaceCount(amount))
    }

    fn begin_cell_gesture(
        &mut self,
        target: Cell,
        gesture: CellGesture,
    ) -> Result<i32, InventoryGestureError> {
        if self.authority != Some(protocol::InventoryAuthority::Server) {
            return Err(InventoryGestureError::AuthorityUnavailable);
        }
        if self.resync_required() {
            return Err(InventoryGestureError::ResyncRequired);
        }
        if self.pending.is_some() {
            return Err(InventoryGestureError::Busy);
        }
        let personal_generation = if matches!(target, Cell::Inventory(_)) && self.storage.is_none()
        {
            Some(
                self.personal_generation_for_gesture()
                    .ok_or(InventoryGestureError::PersonalInventoryUnavailable)?,
            )
        } else {
            None
        };
        let (target_stack, target_revision) = match target {
            Cell::Inventory(slot) => {
                let index = usize::from(slot);
                if index >= PLAYER_INVENTORY_SLOT_COUNT {
                    return Err(InventoryGestureError::InvalidSlot(slot));
                }
                if !self.known[index] {
                    return Err(InventoryGestureError::UnknownSlot(slot));
                }
                (self.slots[index].clone(), self.slot_revisions[index])
            }
            Cell::Storage(slot) => {
                let storage = self
                    .storage
                    .as_ref()
                    .ok_or(InventoryGestureError::InvalidStorageSlot(slot))?;
                if storage.identity.is_none() || storage.resync_required {
                    return Err(InventoryGestureError::ResyncRequired);
                }
                let index = usize::from(slot);
                if index >= storage.slots.len() {
                    return Err(InventoryGestureError::InvalidStorageSlot(slot));
                }
                (storage.slots[index].clone(), storage.revisions[index])
            }
            Cell::Cursor => unreachable!("cursor is not a click target"),
        };
        let inventory = target_stack
            .as_ref()
            .filter(|stack| !stack.is_empty())
            .cloned();
        let cursor = self
            .cursor
            .as_ref()
            .filter(|stack| !stack.is_empty())
            .cloned();
        let inventory_cell = target;
        let inventory_revision = target_revision;
        let cursor_revision = self.cell_revision(Cell::Cursor);
        let storage_identity = self.storage_identity();
        let target_overlay = self.cell_overlay(inventory_cell).cloned();
        let cursor_overlay = self.cell_overlay(Cell::Cursor).cloned();
        let (action, prediction) = match gesture {
            CellGesture::Click => match (inventory, cursor) {
                (Some(stack), None) => counted_transfer(
                    StackRequestActionKind::Take,
                    inventory_cell,
                    Cell::Cursor,
                    stack,
                    target_overlay,
                    inventory_revision,
                    cursor_revision,
                    storage_identity,
                    None,
                )?,
                (None, Some(stack)) => counted_transfer(
                    StackRequestActionKind::Place,
                    Cell::Cursor,
                    inventory_cell,
                    stack,
                    cursor_overlay,
                    cursor_revision,
                    inventory_revision,
                    storage_identity,
                    None,
                )?,
                (Some(inventory), Some(cursor)) => {
                    match self.occupied_stack_relation(&cursor, &inventory) {
                        OccupiedStackRelation::Compatible { capacity }
                            if !has_meaningful_overlay(cursor_overlay.as_ref())
                                && !has_meaningful_overlay(target_overlay.as_ref()) =>
                        {
                            let amount = cursor.count.min(capacity.saturating_sub(inventory.count));
                            counted_merge(
                                StackRequestActionKind::Place,
                                Cell::Cursor,
                                inventory_cell,
                                cursor,
                                inventory,
                                cursor_overlay,
                                target_overlay,
                                cursor_revision,
                                inventory_revision,
                                storage_identity,
                                amount,
                                capacity,
                            )?
                        }
                        OccupiedStackRelation::Incompatible => (
                            StackRequestAction::Swap {
                                source: request_slot(
                                    Cell::Cursor,
                                    cursor.stack_network_id,
                                    storage_identity,
                                )?,
                                destination: request_slot(
                                    inventory_cell,
                                    inventory.stack_network_id,
                                    storage_identity,
                                )?,
                            },
                            Prediction {
                                source: Cell::Cursor,
                                source_stack: Some(inventory),
                                source_revision: cursor_revision,
                                destination: inventory_cell,
                                destination_stack: Some(cursor),
                                destination_revision: inventory_revision,
                                source_overlay: target_overlay,
                                destination_overlay: cursor_overlay,
                                requires_distinct_stack_ids: false,
                                registry_bound_merge: false,
                            },
                        ),
                        OccupiedStackRelation::Compatible { .. }
                        | OccupiedStackRelation::Unsupported => {
                            return Err(InventoryGestureError::InvalidRequest);
                        }
                    }
                }
                (None, None) => return Err(InventoryGestureError::EmptyGesture),
            },
            CellGesture::TakeCount(amount) => {
                let stack = inventory.ok_or(InventoryGestureError::EmptyGesture)?;
                if let Some(cursor) = cursor {
                    let OccupiedStackRelation::Compatible { capacity } =
                        self.occupied_stack_relation(&stack, &cursor)
                    else {
                        return Err(InventoryGestureError::InvalidRequest);
                    };
                    if has_meaningful_overlay(target_overlay.as_ref())
                        || has_meaningful_overlay(cursor_overlay.as_ref())
                    {
                        return Err(InventoryGestureError::InvalidRequest);
                    }
                    counted_merge(
                        StackRequestActionKind::Take,
                        inventory_cell,
                        Cell::Cursor,
                        stack,
                        cursor,
                        target_overlay,
                        cursor_overlay,
                        inventory_revision,
                        cursor_revision,
                        storage_identity,
                        amount,
                        capacity,
                    )?
                } else {
                    counted_transfer(
                        StackRequestActionKind::Take,
                        inventory_cell,
                        Cell::Cursor,
                        stack,
                        target_overlay,
                        inventory_revision,
                        cursor_revision,
                        storage_identity,
                        Some(amount),
                    )?
                }
            }
            CellGesture::PlaceCount(amount) => {
                let stack = cursor.ok_or(InventoryGestureError::EmptyGesture)?;
                if let Some(inventory) = inventory {
                    let OccupiedStackRelation::Compatible { capacity } =
                        self.occupied_stack_relation(&stack, &inventory)
                    else {
                        return Err(InventoryGestureError::InvalidRequest);
                    };
                    if has_meaningful_overlay(cursor_overlay.as_ref())
                        || has_meaningful_overlay(target_overlay.as_ref())
                    {
                        return Err(InventoryGestureError::InvalidRequest);
                    }
                    counted_merge(
                        StackRequestActionKind::Place,
                        Cell::Cursor,
                        inventory_cell,
                        stack,
                        inventory,
                        cursor_overlay,
                        target_overlay,
                        cursor_revision,
                        inventory_revision,
                        storage_identity,
                        amount,
                        capacity,
                    )?
                } else {
                    counted_transfer(
                        StackRequestActionKind::Place,
                        Cell::Cursor,
                        inventory_cell,
                        stack,
                        cursor_overlay,
                        cursor_revision,
                        inventory_revision,
                        storage_identity,
                        Some(amount),
                    )?
                }
            }
        };
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_sub(2)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        self.pending = Some(PendingRequest {
            request_id,
            action,
            prediction,
            state: InventoryPendingState::AwaitingTransport,
            transport_deadline_millis: None,
            deadline_millis: None,
            session_generation: self.session_generation,
            storage_generation: self.storage.as_ref().map(|storage| storage.generation),
            personal_generation,
            storage_identity: self.storage.as_ref().and_then(|storage| storage.identity),
        });
        Ok(request_id)
    }
}

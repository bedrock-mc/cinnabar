//! Normal complex transactions write their final descriptors on the client.
//! Vanilla client verification does not reject a stale source item.

use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::{
    EnumsInventorySourceType, InventoryAction, InventoryTransactionPacket,
    InventoryTransactionPacketTransaction,
};

use super::{
    ARMOR_SLOTS, ARMOR_WINDOW_ID, CONTAINER_NAME_CURSOR, CREATED_OUTPUT_SLOT, ContainerIdentity,
    InventoryEvent, InventorySlotEvent, InventoryTransactionEvent, MAX_CONTAINER_SLOTS,
    OFFHAND_WINDOW_ID, PLAYER_INVENTORY_SLOTS, PLAYER_INVENTORY_WINDOW_ID, SlotIdentity,
    UI_INVENTORY_WINDOW_ID, UI_SLOT_COUNT, normalize_item_descriptor,
};

pub(crate) fn normalize_transaction(packet: InventoryTransactionPacket) -> Option<InventoryEvent> {
    let InventoryTransactionPacketTransaction::NormalTransactionData(normal) = packet.transaction
    else {
        return None;
    };
    let mut slots = Vec::new();
    let mut skipped_actions = 0usize;
    for action in normal.actions.actions {
        // These are the known balancing legs, not inventory authority.
        if matches!(
            action.source.source_type,
            EnumsInventorySourceType::Worldinteraction
                | EnumsInventorySourceType::Creativeinventory
        ) {
            continue;
        }
        let Some(identity) = inventory_identity(&action) else {
            skipped_actions = skipped_actions.saturating_add(1);
            continue;
        };
        if slots.len() == MAX_CONTAINER_SLOTS {
            skipped_actions = skipped_actions.saturating_add(1);
            continue;
        }
        // Framing was decoded in full before normalization. An odd descriptor
        // is a counted semantic skip; it must not terminate the session.
        let Ok(stack) = normalize_item_descriptor(action.to_item) else {
            skipped_actions = skipped_actions.saturating_add(1);
            continue;
        };
        slots.push(InventorySlotEvent {
            identity,
            stack,
            storage_item: None,
        });
    }
    Some(InventoryEvent::Transaction(InventoryTransactionEvent {
        slots: Arc::from(slots),
        skipped_actions,
    }))
}

fn inventory_identity(action: &InventoryAction) -> Option<SlotIdentity> {
    if action.source.source_type != EnumsInventorySourceType::Containerinventory {
        return None;
    }
    let window = i32::from(action.source.container_id?);
    let slot = u16::try_from(action.slot).ok()?;
    let container = match window {
        PLAYER_INVENTORY_WINDOW_ID if slot < u16::from(PLAYER_INVENTORY_SLOTS) => {
            ContainerIdentity::window(window)
        }
        OFFHAND_WINDOW_ID if slot == 0 => ContainerIdentity::window(window),
        ARMOR_WINDOW_ID if slot < u16::from(ARMOR_SLOTS) => ContainerIdentity::window(window),
        UI_INVENTORY_WINDOW_ID if slot == 0 => ContainerIdentity {
            window_id: None,
            slot_type: Some(CONTAINER_NAME_CURSOR),
            dynamic_id: None,
        },
        UI_INVENTORY_WINDOW_ID
            if usize::from(slot) < UI_SLOT_COUNT && slot != u16::from(CREATED_OUTPUT_SLOT) =>
        {
            ContainerIdentity {
                window_id: Some(window),
                slot_type: Some(0),
                dynamic_id: None,
            }
        }
        // Vanilla defers UI output slot 50 to a pending transaction action;
        // treating that as an ordinary slot write would invent authority.
        _ => return None,
    };
    Some(SlotIdentity { container, slot })
}

#[cfg(test)]
mod tests;

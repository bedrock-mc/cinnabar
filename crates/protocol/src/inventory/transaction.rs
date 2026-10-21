//! Normal complex transactions write their final descriptors on the client.
//! Vanilla client verification does not reject a stale source item.

use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::InventorySourceType;
use bedrock_protocol_1_26_51::packets::InventoryTransaction;
use bedrock_protocol_1_26_51::types::{InventoryAction, InventoryTransactionPacketData};

use super::{
    ARMOR_SLOTS, ARMOR_WINDOW_ID, CONTAINER_NAME_CURSOR, CREATED_OUTPUT_SLOT, ContainerIdentity,
    InventoryEvent, InventorySlotEvent, InventoryTransactionEvent, MAX_CONTAINER_SLOTS,
    OFFHAND_WINDOW_ID, PLAYER_INVENTORY_SLOTS, PLAYER_INVENTORY_WINDOW_ID, SlotIdentity,
    UI_INVENTORY_WINDOW_ID, UI_SLOT_COUNT, normalize_item_descriptor,
};

pub(crate) fn normalize_transaction(packet: InventoryTransaction) -> Option<InventoryEvent> {
    let InventoryTransactionPacketData::NormalTransactionData { actions } = packet.transaction
    else {
        return None;
    };
    if actions.actions.iter().any(invalid_action) {
        return Some(InventoryEvent::Transaction(InventoryTransactionEvent {
            slots: Arc::from([]),
            skipped_actions: actions.actions.len(),
        }));
    }
    let mut slots = Vec::new();
    let mut skipped_actions = 0usize;
    for action in actions.actions {
        // These are the known balancing legs, not inventory authority.
        if matches!(
            action.source.source_type,
            InventorySourceType::WorldInteraction | InventorySourceType::CreativeInventory
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

/// Rejects a normal transaction before any supported leg can publish authority.
fn invalid_action(action: &InventoryAction) -> bool {
    match action.source.source_type {
        InventorySourceType::WorldInteraction | InventorySourceType::CreativeInventory => false,
        InventorySourceType::ContainerInventory => {
            let Some(window) = action.source.container_id.map(|id| i32::from(id.0)) else {
                return true;
            };
            let slot = action.slot.0;
            match window {
                PLAYER_INVENTORY_WINDOW_ID => slot >= u32::from(PLAYER_INVENTORY_SLOTS),
                OFFHAND_WINDOW_ID => slot != 0,
                // Transaction verification has a wider range than retained armor cells.
                ARMOR_WINDOW_ID => slot >= ARMOR_TRANSACTION_LIMIT,
                UI_INVENTORY_WINDOW_ID => slot >= UI_SLOT_COUNT as u32,
                _ => false,
            }
        }
        _ => true,
    }
}

/// The transaction verifier's equipment index bound, independent of UI armor cells.
const ARMOR_TRANSACTION_LIMIT: u32 = 14;

fn inventory_identity(action: &InventoryAction) -> Option<SlotIdentity> {
    if action.source.source_type != InventorySourceType::ContainerInventory {
        return None;
    }
    let window = i32::from(action.source.container_id?.0);
    let slot = u16::try_from(action.slot.0).ok()?;
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

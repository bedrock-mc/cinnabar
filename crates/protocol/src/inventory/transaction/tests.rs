use super::*;
use bedrock_protocol_1_26_51::types::{
    InventorySource, InventoryTransactionData, NetworkItemStackDescriptorSerializedData,
};
use bedrock_protocol_1_26_51::wire;

fn action(window: i32, slot: u32, count: u16) -> InventoryAction {
    InventoryAction {
        source: InventorySource {
            source_type: InventorySourceType::ContainerInventory,
            container_id: Some(wire::I8(i8::try_from(window).unwrap())),
            bit_flags: None,
        },
        slot: wire::VarUInt(slot),
        // Client verification explicitly ignores mismatched previous contents.
        from_item: NetworkItemStackDescriptorSerializedData {
            id: wire::I16LE(91),
            stack_size: wire::U16LE(77),
            net_id_variant: Some(wire::ZigZag32(900)),
            ..Default::default()
        },
        to_item: NetworkItemStackDescriptorSerializedData {
            id: wire::I16LE(3),
            stack_size: wire::U16LE(count),
            net_id_variant: Some(wire::ZigZag32(80)),
            aux_value: wire::VarUInt(7),
            block_runtime_id: wire::VarUInt(u32::MAX),
            user_data_buffer: vec![0; 10].into(),
        },
    }
}

fn transaction(actions: Vec<InventoryAction>) -> super::super::InventoryTransactionEvent {
    let packet = InventoryTransaction {
        transaction: InventoryTransactionPacketData::NormalTransactionData {
            actions: InventoryTransactionData { actions },
        },
        ..Default::default()
    };
    let Some(InventoryEvent::Transaction(event)) = normalize_transaction(packet) else {
        panic!()
    };
    event
}

#[test]
fn normal_transaction_routes_only_native_direct_write_surfaces_in_wire_order() {
    let event = transaction(vec![
        action(PLAYER_INVENTORY_WINDOW_ID, 6, 64),
        action(OFFHAND_WINDOW_ID, 0, 1),
        action(ARMOR_WINDOW_ID, 2, 1),
        action(UI_INVENTORY_WINDOW_ID, 0, 3),
        action(UI_INVENTORY_WINDOW_ID, 28, 4),
        action(PLAYER_INVENTORY_WINDOW_ID, 6, 63),
    ]);
    assert_eq!(event.slots.len(), 6);
    assert_eq!(event.skipped_actions, 0);
    let cells: Vec<_> = event
        .slots
        .iter()
        .map(|slot| {
            super::super::project_container_cell(&slot.identity.container, slot.identity.slot)
        })
        .collect();
    assert_eq!(
        cells[..4],
        [
            Some(super::super::CanonicalCell::PlayerInventory(6)),
            Some(super::super::CanonicalCell::Offhand),
            Some(super::super::CanonicalCell::Armor(2)),
            Some(super::super::CanonicalCell::Cursor),
        ]
    );
    assert_eq!(
        super::super::personal_craft_slot_index(
            &event.slots[4].identity.container,
            event.slots[4].identity.slot
        ),
        Some(0)
    );
    assert_eq!(event.slots[0].stack.count, 64);
    assert_eq!(event.slots[5].stack.count, 63);
    assert_eq!(event.slots[0].stack.metadata, 7);
    assert_eq!(event.slots[0].stack.block_runtime_id, -1);
    assert_eq!(event.slots[0].stack.stack_network_id, 80);
}

#[test]
fn invalid_normal_transaction_does_not_publish_valid_siblings() {
    let mut unknown = action(PLAYER_INVENTORY_WINDOW_ID, 0, 1);
    unknown.source.source_type = InventorySourceType::Unknown(111);
    let mut sentinel = action(PLAYER_INVENTORY_WINDOW_ID, 0, 1);
    sentinel.to_item.net_id_variant = Some(wire::ZigZag32(-77));
    let mut bad_extra = action(PLAYER_INVENTORY_WINDOW_ID, 0, 1);
    bad_extra.to_item.user_data_buffer = vec![1].into();
    let event = transaction(vec![
        action(
            PLAYER_INVENTORY_WINDOW_ID,
            u32::from(PLAYER_INVENTORY_SLOTS),
            1,
        ),
        action(OFFHAND_WINDOW_ID, 1, 1),
        action(ARMOR_WINDOW_ID, u32::from(ARMOR_SLOTS), 1),
        action(UI_INVENTORY_WINDOW_ID, UI_SLOT_COUNT as u32, 1),
        action(UI_INVENTORY_WINDOW_ID, u32::from(CREATED_OUTPUT_SLOT), 1),
        action(7, 0, 1),
        unknown,
        sentinel,
        bad_extra,
        action(PLAYER_INVENTORY_WINDOW_ID, 6, 64),
    ]);
    assert_eq!(event.skipped_actions, 10);
    assert!(event.slots.is_empty());
}

#[test]
fn normal_transaction_retention_uses_existing_inventory_limit() {
    let event = transaction(vec![
        action(PLAYER_INVENTORY_WINDOW_ID, 0, 1);
        MAX_CONTAINER_SLOTS + 1
    ]);
    assert_eq!(event.slots.len(), MAX_CONTAINER_SLOTS);
    assert_eq!(event.skipped_actions, 1);
}

#[test]
fn normal_transaction_preserves_final_charged_item_compound_not_previous_extra() {
    let mut extra = vec![255, 255, 1, 10, 0, 0, 10, 11, 0];
    extra.extend_from_slice(b"chargedItem");
    extra.extend_from_slice(&[8, 4, 0]);
    extra.extend_from_slice(b"Name");
    let name = b"minecraft:arrow";
    extra.extend_from_slice(&(name.len() as u16).to_le_bytes());
    extra.extend_from_slice(name);
    extra.extend_from_slice(&[0, 0]);
    extra.extend_from_slice(&[0; 8]);
    let mut update = action(PLAYER_INVENTORY_WINDOW_ID, 2, 1);
    update.to_item.user_data_buffer = extra.clone().into();
    let event = transaction(vec![update]);
    let stack = &event.slots[0].stack;
    assert_eq!(stack.extra_data.as_ref(), extra);
    use sha2::Digest;
    assert_eq!(
        stack.nbt_digest,
        <[u8; 32]>::from(sha2::Sha256::digest(&extra))
    );
    assert_eq!(stack.stack_network_id, 80);
}

/// A transaction with an invalid offhand index must not publish its otherwise valid leg.
#[test]
fn invalid_offhand_leg_does_not_publish_player_leg() {
    let event = transaction(vec![
        action(PLAYER_INVENTORY_WINDOW_ID, 0, 1),
        action(OFFHAND_WINDOW_ID, 1, 1),
    ]);
    assert!(event.slots.is_empty());
}

/// A known invalid source leg invalidates the complete native transaction.
#[test]
fn invalid_player_bound_rejects_whole() {
    let e = transaction(vec![
        action(0, 0, 1),
        action(0, u32::from(PLAYER_INVENTORY_SLOTS), 1),
    ]);
    assert!(e.slots.is_empty());
}

/// Armor index fourteen is outside the verifier's complete admitted range.
#[test]
fn invalid_armor_bound_rejects_whole() {
    let e = transaction(vec![
        action(0, 0, 1),
        action(ARMOR_WINDOW_ID, super::ARMOR_TRANSACTION_LIMIT, 1),
    ]);
    assert!(e.slots.is_empty());
}

/// UI index fifty-four is outside the native inventory's range.
#[test]
fn invalid_ui_bound_rejects_whole() {
    let e = transaction(vec![
        action(0, 0, 1),
        action(UI_INVENTORY_WINDOW_ID, UI_SLOT_COUNT as u32, 1),
    ]);
    assert!(e.slots.is_empty());
}

/// A valid UI output leg is deferred and does not invalidate a valid player leg.
#[test]
fn output_defer_and_stale_previous_control() {
    let e = transaction(vec![
        action(0, 0, 1),
        action(UI_INVENTORY_WINDOW_ID, u32::from(CREATED_OUTPUT_SLOT), 1),
    ]);
    assert_eq!(e.slots.len(), 1);
    assert_eq!(e.slots[0].stack.count, 1);
}

/// The decoded invalid-source fallback prevents publication of the whole normal transaction.
#[test]
fn unknown_source_does_not_publish_valid_leg() {
    let mut unknown = action(0, 0, 1);
    unknown.source.source_type = InventorySourceType::Unknown(111);
    unknown.source.container_id = None;
    let e = transaction(vec![action(0, 0, 1), unknown]);
    assert!(e.slots.is_empty());
}
/// Changing only the offhand index to zero preserves both valid client-authority legs.
#[test]
fn valid_offhand_leg_preserves_both_authoritative_legs() {
    let event = transaction(vec![
        action(PLAYER_INVENTORY_WINDOW_ID, 0, 1),
        action(OFFHAND_WINDOW_ID, 0, 1),
    ]);
    assert_eq!(event.skipped_actions, 0);
    assert_eq!(event.slots.len(), 2);
    assert_eq!(event.slots[0].stack.count, 1);
    assert_eq!(event.slots[1].stack.count, 1);
}

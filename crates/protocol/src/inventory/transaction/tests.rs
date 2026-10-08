use super::*;
use valentine::bedrock::version::v1_26_51::{
    CerealizerNetworkItemStackDescriptorSerializedData, InventorySource, InventoryTransaction,
    NormalTransactionData,
};

fn action(window: i32, slot: u32, count: u16) -> InventoryAction {
    InventoryAction {
        source: InventorySource {
            source_type: EnumsInventorySourceType::Containerinventory,
            container_id: Some(i8::try_from(window).unwrap()),
            bit_flags: None,
        },
        slot,
        // Client verification explicitly ignores mismatched previous contents.
        from_item: CerealizerNetworkItemStackDescriptorSerializedData {
            id: 91,
            stacksize: 77,
            net_id_variant: Some(900),
            ..Default::default()
        },
        to_item: CerealizerNetworkItemStackDescriptorSerializedData {
            id: 3,
            stacksize: count,
            net_id_variant: Some(80),
            auxvalue: 7,
            block_runtime_id: u32::MAX,
            user_data_buffer: vec![0; 10],
        },
    }
}

fn transaction(actions: Vec<InventoryAction>) -> super::super::InventoryTransactionEvent {
    let packet = InventoryTransactionPacket {
        transaction: InventoryTransactionPacketTransaction::NormalTransactionData(
            NormalTransactionData {
                actions: InventoryTransaction { actions },
            },
        ),
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
fn normal_transaction_counts_odd_actions_without_losing_valid_authority() {
    let mut unknown = action(PLAYER_INVENTORY_WINDOW_ID, 0, 1);
    unknown.source.source_type = EnumsInventorySourceType::Unknown(111);
    let mut sentinel = action(PLAYER_INVENTORY_WINDOW_ID, 0, 1);
    sentinel.to_item.net_id_variant = Some(-77);
    let mut bad_extra = action(PLAYER_INVENTORY_WINDOW_ID, 0, 1);
    bad_extra.to_item.user_data_buffer = vec![1];
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
    assert_eq!(event.skipped_actions, 9);
    assert_eq!(event.slots.len(), 1);
    assert_eq!(event.slots[0].stack.count, 64);
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
    update.to_item.user_data_buffer = extra.clone();
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

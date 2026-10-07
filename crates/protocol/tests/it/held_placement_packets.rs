use protocol::{
    BedrockSession, BlockUseRequest, ItemUseTrigger, NetworkItemStack, PredictedSlotChange,
    VerifiedNetworkItemStack, click_block_transaction_packet, decode_batch, encode,
    start_item_use_on_packet, stop_item_use_on_packet,
};
use valentine::bedrock::version::v1_26_51::{
    EnumsContainerEnumName, EnumsInventorySourceType,
    EnumsItemUseInventoryTransactionPredictedResult, EnumsItemUseInventoryTransactionTriggerType,
    EnumsPlayerActionType, InventoryTransactionPacketTransaction, McpePacketData,
};

/// A verified full block stack with an authoritative network identity.
fn block_stack(count: u16) -> VerifiedNetworkItemStack {
    let mut stack = NetworkItemStack::empty();
    stack.network_id = 1;
    stack.stack_network_id = 41;
    stack.count = count;
    stack.block_runtime_id = 77;
    let digest = stack.nbt_digest;
    VerifiedNetworkItemStack::try_new(stack, digest).unwrap()
}

/// Encodes and decodes the packet so assertions cover the outbound wire values.
fn round_trip(packet: protocol::Packet) -> McpePacketData {
    let session = BedrockSession { shield_item_id: 0 };
    decode_batch(encode(&packet, &session).unwrap(), &session)
        .unwrap()
        .remove(0)
        .data
}

#[test]
fn held_use_start_and_stop_carry_original_and_last_destinations() {
    let McpePacketData::PlayerActionPacket(start) =
        round_trip(start_item_use_on_packet(42, [3, 63, -2], [3, 63, -1], 3))
    else {
        panic!("start item use");
    };
    assert_eq!(start.action, EnumsPlayerActionType::Startitemuseon);
    assert_eq!(start.player_runtime_id.actor_runtime_id, 42);
    assert_eq!(
        [
            start.block_position.x,
            start.block_position.y,
            start.block_position.z
        ],
        [3, 63, -2]
    );
    assert_eq!(
        [start.result_pos.x, start.result_pos.y, start.result_pos.z],
        [3, 63, -1]
    );
    assert_eq!(start.face, 3);
    let McpePacketData::PlayerActionPacket(stop) =
        round_trip(stop_item_use_on_packet(42, [3, 63, 4]))
    else {
        panic!("stop item use");
    };
    assert_eq!(stop.action, EnumsPlayerActionType::Stopitemuseon);
    assert_eq!(
        [
            stop.block_position.x,
            stop.block_position.y,
            stop.block_position.z
        ],
        [3, 63, 4]
    );
    assert_eq!(
        [stop.result_pos.x, stop.result_pos.y, stop.result_pos.z],
        [0; 3]
    );
    assert_eq!(stop.face, 0);
}

#[test]
fn survival_press_and_repeat_record_stack_deltas_and_prediction_fields() {
    let mut held = block_stack(3);
    for (index, trigger, wire_trigger) in [
        (
            0,
            ItemUseTrigger::PlayerInput,
            EnumsItemUseInventoryTransactionTriggerType::Playerinput,
        ),
        (
            1,
            ItemUseTrigger::SimulationTick,
            EnumsItemUseInventoryTransactionTriggerType::Simulationtick,
        ),
        (
            2,
            ItemUseTrigger::SimulationTick,
            EnumsItemUseInventoryTransactionTriggerType::Simulationtick,
        ),
    ] {
        let id = -4 - index * 2;
        let next = held.less_one(id);
        let request = BlockUseRequest {
            block_position: [0, 63, index],
            face: 3,
            selected_slot: 2,
            selected_item: held.clone(),
            player_position: [0.5, 66.12, index as f32 + 0.5],
            relative_hit: [0.0; 3],
            block_runtime_id: 71,
        };
        let change = PredictedSlotChange {
            legacy_request_id: id,
            from: held,
            to: next.clone(),
        };
        let McpePacketData::InventoryTransactionPacket(packet) = round_trip(
            click_block_transaction_packet(request, trigger, true, Some(change)).unwrap(),
        ) else {
            panic!("transaction");
        };
        if next.is_empty() {
            assert_eq!(packet.legacy_request_id.id, 0);
            assert!(packet.legacy_set_item_slots.is_none());
        } else {
            assert_eq!(packet.legacy_request_id.id, id);
            let slots = packet.legacy_set_item_slots.as_ref().unwrap();
            assert_eq!(
                slots[0].container_enum,
                EnumsContainerEnumName::Inventorycontainer
            );
            assert_eq!(slots[0].slots, [2]);
        }
        let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction) =
            packet.transaction
        else {
            panic!("item use");
        };
        assert_eq!(transaction.trigger_type, wire_trigger);
        assert_eq!(
            transaction.client_interact_prediction,
            EnumsItemUseInventoryTransactionPredictedResult::Success
        );
        assert_eq!(transaction.target_block_id, 71);
        assert_eq!(
            [
                transaction.click_position.x,
                transaction.click_position.y,
                transaction.click_position.z
            ],
            [0.0; 3]
        );
        assert_eq!(transaction.actions.actions.len(), 1);
        let action = &transaction.actions.actions[0];
        assert_eq!(
            action.source.source_type,
            EnumsInventorySourceType::Containerinventory
        );
        assert_eq!(action.source.container_id, Some(0));
        assert_eq!(action.slot, 2);
        assert_eq!(action.from_item.stacksize, 3 - index as u16);
        assert_eq!(action.to_item.stacksize, 2 - index as u16);
        held = next;
    }
}

#[test]
fn failed_repeat_preserves_target_without_inventory_prediction() {
    let request = BlockUseRequest {
        block_position: [0, 63, 2],
        face: 3,
        selected_slot: 2,
        selected_item: block_stack(3),
        player_position: [0.5, 66.12, 2.5],
        relative_hit: [-1.0, 2.0, 0.0],
        block_runtime_id: 71,
    };
    let McpePacketData::InventoryTransactionPacket(packet) = round_trip(
        click_block_transaction_packet(request, ItemUseTrigger::SimulationTick, false, None)
            .unwrap(),
    ) else {
        panic!("transaction");
    };
    assert_eq!(packet.legacy_request_id.id, 0);
    assert!(packet.legacy_set_item_slots.is_none());
    let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction) =
        packet.transaction
    else {
        panic!("item use");
    };
    assert!(transaction.actions.actions.is_empty());
    assert_eq!(transaction.item.stacksize, 3);
    assert_eq!(transaction.target_block_id, 71);
    assert_eq!(
        transaction.trigger_type,
        EnumsItemUseInventoryTransactionTriggerType::Simulationtick
    );
    assert_eq!(
        transaction.client_interact_prediction,
        EnumsItemUseInventoryTransactionPredictedResult::Failure
    );
    assert_eq!(
        [
            transaction.click_position.x,
            transaction.click_position.y,
            transaction.click_position.z
        ],
        [-1.0, 2.0, 0.0]
    );
}

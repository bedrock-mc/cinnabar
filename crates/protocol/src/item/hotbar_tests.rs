use bedrock_protocol_1_26_51::packets;

use super::*;
use crate::InventoryPacketError;

/// Builds one non-empty stack with retained bytes suitable for round-trip tests.
fn selected_stack() -> NetworkItemStack {
    let extra_data: Arc<[u8]> = Arc::from([0_u8, 0, 0, 0, 0, 0, 0, 0]);
    NetworkItemStack {
        network_id: 7,
        metadata: 3,
        stack_network_id: 13,
        count: 4,
        nbt_digest: Sha256::digest(&extra_data).into(),
        block_runtime_id: 92,
        extra_data,
    }
}

#[test]
fn selected_stack_survives_mob_equipment_encode_decode_and_normalize() {
    let expected = selected_stack();
    let packet = select_hotbar_slot_packet(4242, 3, &expected).unwrap();
    let encoded = crate::encode(&packet).unwrap();
    let mut decoded = crate::decode_batch(encoded).unwrap();
    let packets::Packet::MobEquipment(packet) = *decoded.remove(0).data else {
        panic!("hotbar selection must build a MobEquipment packet, not PlayerHotbar");
    };
    assert_eq!(packet.target_runtime_id.0, 4242);
    assert_eq!(packet.slot.0, 3);
    assert_eq!(packet.selected_slot.0, 3);
    assert_eq!(packet.container_id.0, 0);
    assert_eq!(packet.item.user_data_buffer, expected.extra_data.as_ref());
    let mut untracked = expected;
    untracked.stack_network_id = -1;
    assert_eq!(normalize_equipment(packet).unwrap().stack, untracked);
}

/// A predicted request id is never written, so the packet builds before the server answers.
#[test]
fn hotbar_selection_omits_the_stack_network_id() {
    let mut predicted = selected_stack();
    predicted.stack_network_id = -3;
    let packets::Packet::MobEquipment(packet) =
        *select_hotbar_slot_packet(1, 0, &predicted).unwrap().data
    else {
        panic!("hotbar selection must build a MobEquipment packet");
    };
    assert_eq!(packet.item.net_id_variant, None);
    assert_eq!(packet.item.stack_size.0, 4);
}

#[test]
fn known_empty_hotbar_slot_encodes_the_canonical_empty_descriptor() {
    let packets::Packet::MobEquipment(packet) =
        *select_hotbar_slot_packet(1, 2, &NetworkItemStack::empty())
            .unwrap()
            .data
    else {
        panic!("hotbar selection must build a MobEquipment packet");
    };
    assert_eq!(packet.item, ItemStackDescriptor::default());
}

#[test]
fn invalid_hotbar_slot_fails_instead_of_clamping() {
    assert_eq!(
        select_hotbar_slot_packet(1, HOTBAR_SLOT_COUNT, &NetworkItemStack::empty()).unwrap_err(),
        InventoryPacketError::InvalidSelectedSlot(i32::from(HOTBAR_SLOT_COUNT))
    );
}

#[test]
fn invalid_hotbar_stack_digest_fails_instead_of_sending_air() {
    let mut stack = selected_stack();
    stack.nbt_digest = [0; 32];
    assert_eq!(
        select_hotbar_slot_packet(1, 0, &stack).unwrap_err(),
        InventoryPacketError::DigestMismatch
    );
}

#[test]
fn invalid_hotbar_stack_shape_fails_instead_of_sending_air() {
    let mut stack = selected_stack();
    stack.network_id = i32::from(i16::MAX) + 1;
    assert_eq!(
        select_hotbar_slot_packet(1, 0, &stack).unwrap_err(),
        InventoryPacketError::InvalidItemNetworkId(stack.network_id)
    );
}

#[test]
fn offhand_update_round_trips_its_equipment_address_and_clear() {
    for stack in [selected_stack(), NetworkItemStack::empty()] {
        let packet = offhand_equipment_packet(4242, &stack).unwrap();
        let mut decoded = crate::decode_batch(crate::encode(&packet).unwrap()).unwrap();
        let packets::Packet::MobEquipment(packet) = *decoded.remove(0).data else {
            panic!("offhand update must build equipment");
        };
        assert_eq!(packet.target_runtime_id.0, 4242);
        assert_eq!(packet.slot.0, 1);
        assert_eq!(packet.selected_slot.0, 0);
        assert_eq!(i32::from(packet.container_id.0), crate::OFFHAND_WINDOW_ID);
        let mut expected = stack;
        expected.stack_network_id = -1;
        let event = normalize_equipment(packet).unwrap();
        assert_eq!(event.handedness, Some(ActorHandedness::Left));
        assert_eq!(event.stack, expected);
    }
}

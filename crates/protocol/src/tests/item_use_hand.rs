//! Independent byte expectations for the item-use hand and its surrounding fields.

use protocol::{
    BlockAction, BlockActionKind, BlockActions, BlockItemInteraction, BlockUseRequest,
    HeldItemRequest, ItemUseHand, NetworkItemStack, PlayerAuthInputInteractions,
    PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode, VerifiedNetworkItemStack,
};

/// Supplies different authoritative stack identities for the two logical hands.
fn stack(hand: ItemUseHand) -> VerifiedNetworkItemStack {
    let mut stack = NetworkItemStack::empty();
    (stack.network_id, stack.count, stack.stack_network_id) = match hand {
        ItemUseHand::Mainhand => (11, 7, 74),
        ItemUseHand::Offhand => (13, 3, 85),
    };
    stack.metadata = 3;
    stack.block_runtime_id = 139;
    let digest = stack.nbt_digest;
    VerifiedNetworkItemStack::try_new(stack, digest).unwrap()
}

/// Keeps a nonzero hotbar selection beside the explicitly supplied hand and stack.
fn block_request(hand: ItemUseHand) -> BlockUseRequest {
    BlockUseRequest {
        hand,
        block_position: [13, 71, -29],
        face: 5,
        selected_slot: 7,
        selected_item: stack(hand),
        player_position: [13.25, 72.625, -28.75],
        relative_hit: [0.25, 0.75, 0.5],
        block_runtime_id: 123_456,
    }
}

/// Uses the same authoritative selection for air-use and item-attack builders.
fn held_request(hand: ItemUseHand) -> HeldItemRequest {
    let request = block_request(hand);
    HeldItemRequest {
        hand,
        selected_slot: request.selected_slot,
        selected_item: request.selected_item,
        player_position: request.player_position,
    }
}

/// Writes little-endian floats without using the packet codec.
fn floats(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}

/// Authors the complete transaction body directly from the protocol field order.
fn expected_use(hand: ItemUseHand, action: u8) -> Vec<u8> {
    // https://mojang.github.io/bedrock-protocol-docs/1.26.51/types/item-use-inventory-transaction/
    let mut out = vec![0, action * 2, u8::from(action == 0)];
    if action == 0 || action == 2 {
        out.extend([26, 142, 1, 57]); // Zigzag block position.
    } else {
        out.extend([0, 0, 0]);
    }
    out.push(match action {
        1 => 255,
        3 => 0,
        _ => 5,
    });
    match hand {
        ItemUseHand::Mainhand => {
            out.extend([14, 0]); // Slot seven, then mainhand byte.
            out.extend([11, 0, 7, 0, 3, 0, 139, 1, 0]);
        }
        ItemUseHand::Offhand => {
            out.extend([0, 1]); // Slot zero, then offhand byte.
            out.extend([13, 0, 3, 0, 3, 0, 139, 1, 0]);
        }
    }
    floats(&mut out, &[13.25, 72.625, -28.75]);
    floats(
        &mut out,
        &match action {
            0 => [0.25, 0.75, 0.5],
            3 => [13.5, 73.375, -28.25],
            _ => [0.0; 3],
        },
    );
    if action == 0 {
        out.extend([192, 196, 7]); // Target block runtime ID.
    } else {
        out.push(0);
    }
    out.extend([0, u8::from(action == 3)]); // Failed prediction, attack cooldown.
    out
}

/// Reads only batch framing; transaction fields never use the generated decoder.
fn varuint(input: &mut &[u8]) -> u32 {
    let mut value = 0;
    for shift in (0..35).step_by(7) {
        let byte = input[0];
        *input = &input[1..];
        value |= u32::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return value;
        }
    }
    panic!("invalid varuint framing");
}

/// Removes the independently read batch length and packet header.
fn packet_body(packet: protocol::Packet) -> Vec<u8> {
    let encoded = protocol::encode(&packet).unwrap();
    assert_eq!(encoded[0], 0xfe);
    let mut input = &encoded[1..];
    let length = varuint(&mut input) as usize;
    assert_eq!(input.len(), length);
    assert_eq!(varuint(&mut input), packet.header.id as u32);
    input.to_vec()
}

#[test]
fn item_use_hand_standalone_bytes_preserve_hand_stack_and_following_fields() {
    for hand in [ItemUseHand::Mainhand, ItemUseHand::Offhand] {
        for action in 0..=3 {
            let packet = match action {
                0 => protocol::click_block_packet(block_request(hand)),
                1 => protocol::click_air_packet(held_request(hand), None),
                2 => protocol::destroy_block_packet(block_request(hand)),
                3 => {
                    protocol::use_item_as_attack_packet(held_request(hand), [0.25, 0.75, 0.5], true)
                }
                _ => unreachable!(),
            }
            .unwrap();
            let mut expected = vec![0, 0, 2]; // Legacy ID, absent slots, item-use selector.
            expected.extend(expected_use(hand, action));
            assert_eq!(packet_body(packet), expected, "{hand:?}, action {action}");
        }
    }
}

/// Supplies nonzero fields after the embedded transaction to detect wire misalignment.
fn movement() -> PlayerAuthInputSnapshot {
    PlayerAuthInputSnapshot {
        tick: 41,
        position: [0.0; 3],
        delta: [0.0; 3],
        move_vector: [0.0; 2],
        analogue_move_vector: [0.25, -0.5],
        raw_move_vector: [-0.75, 1.0],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        camera_orientation: [0.0, 0.0, -1.0],
        flags: PlayerInputFlags::NONE,
        input_mode: PlayerInputMode::Mouse,
    }
}

#[test]
fn item_use_hand_embedded_bytes_preserve_hand_stack_and_following_fields() {
    for hand in [ItemUseHand::Mainhand, ItemUseHand::Offhand] {
        for action in [0, 2] {
            let request = block_request(hand);
            let mut block_actions = BlockActions::new();
            block_actions
                .push(BlockAction {
                    kind: BlockActionKind::CrackBlock,
                    position: [1, 2, -3],
                    face: 4,
                })
                .unwrap();
            let packet = protocol::player_auth_input_with_interactions(
                movement(),
                &PlayerAuthInputInteractions {
                    block_actions,
                    block_interaction: Some(if action == 0 {
                        BlockItemInteraction::Use(request)
                    } else {
                        BlockItemInteraction::Destroy(request, None)
                    }),
                },
            )
            .unwrap();
            let mut expected = vec![0; 32]; // Rotation, position, movement and head yaw.
            expected.extend([2, 68, 70, 1, 0, 2]); // Flags, mouse, normal, crosshair.
            floats(&mut expected, &[0.0; 2]); // Interaction rotation.
            expected.push(41);
            floats(&mut expected, &[0.0; 3]); // Position delta.
            expected.extend([1, 0, 0]); // Present use, legacy ID zero, absent legacy slots.
            expected.extend(expected_use(hand, action));
            expected.extend([0, 1, 1, 36, 2, 4, 5, 8, 0, 0]); // Requests, block action, vehicle.
            floats(&mut expected, &[0.25, -0.5, 0.0, 0.0, -1.0, -0.75, 1.0]);
            assert_eq!(packet_body(packet), expected, "{hand:?}, action {action}");
        }
    }
}

#[test]
fn item_use_hand_repeat_preserves_trigger_and_success_for_both_hands() {
    for hand in [ItemUseHand::Mainhand, ItemUseHand::Offhand] {
        let packet = protocol::click_block_transaction_packet(
            block_request(hand),
            protocol::ItemUseTrigger::SimulationTick,
            true,
            None,
        )
        .unwrap();
        let mut transaction = expected_use(hand, 0);
        transaction[2] = 2; // Simulation tick trigger.
        let prediction = transaction.len() - 2;
        transaction[prediction] = 1;
        let mut expected = vec![0, 0, 2];
        expected.extend(transaction);
        assert_eq!(packet_body(packet), expected, "{hand:?}");
    }
}

#[test]
fn item_use_hand_offhand_slot_does_not_depend_on_hotbar_validation() {
    let mut block = block_request(ItemUseHand::Offhand);
    block.selected_slot = u8::MAX;
    assert!(protocol::click_block_packet(block).is_ok());
    let mut held = held_request(ItemUseHand::Offhand);
    held.selected_slot = u8::MAX;
    assert!(protocol::click_air_packet(held, None).is_ok());

    let mut block = block_request(ItemUseHand::Mainhand);
    block.selected_slot = protocol::HOTBAR_SLOT_COUNT;
    assert_eq!(
        protocol::click_block_packet(block),
        Err(protocol::BlockUsePacketError::InvalidSelectedSlot(
            protocol::HOTBAR_SLOT_COUNT
        ))
    );
    let mut held = held_request(ItemUseHand::Mainhand);
    held.selected_slot = protocol::HOTBAR_SLOT_COUNT;
    assert_eq!(
        protocol::click_air_packet(held, None),
        Err(protocol::BlockUsePacketError::InvalidSelectedSlot(
            protocol::HOTBAR_SLOT_COUNT
        ))
    );
}

#[test]
fn item_use_hand_offhand_predictions_and_release_fail_explicitly() {
    let change = protocol::PredictedSlotChange {
        legacy_request_id: -3,
        from: stack(ItemUseHand::Offhand),
        to: stack(ItemUseHand::Offhand).less_one(-3),
    };
    assert_eq!(
        protocol::click_air_packet(held_request(ItemUseHand::Offhand), Some(change.clone())),
        Err(protocol::BlockUsePacketError::UnsupportedOffhandPrediction)
    );
    assert_eq!(
        protocol::click_block_transaction_packet(
            block_request(ItemUseHand::Offhand),
            protocol::ItemUseTrigger::PlayerInput,
            true,
            Some(change),
        ),
        Err(protocol::BlockUsePacketError::UnsupportedOffhandPrediction)
    );
    assert_eq!(
        protocol::release_item_packet(held_request(ItemUseHand::Offhand)),
        Err(protocol::BlockUsePacketError::UnsupportedOffhandRelease)
    );
}

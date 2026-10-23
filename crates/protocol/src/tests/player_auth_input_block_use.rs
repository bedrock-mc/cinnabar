use bedrock_protocol_1_26_51::enums::{
    InputData, ItemUseInventoryTransactionActionType,
    ItemUseInventoryTransactionClientCooldownState, ItemUseInventoryTransactionPredictedResult,
    ItemUseInventoryTransactionTriggerType,
};
use bedrock_protocol_1_26_51::packets;
use protocol::{
    BlockItemInteraction, BlockUseRequest, NetworkItemStack, PlayerAuthInputInteractions,
    PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode, VerifiedNetworkItemStack,
    player_auth_input_with_interactions,
};

fn empty_hand() -> VerifiedNetworkItemStack {
    let stack = NetworkItemStack::empty();
    VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap()
}

fn movement() -> PlayerAuthInputSnapshot {
    PlayerAuthInputSnapshot {
        tick: 41,
        position: [0.5, 65.620_01, 0.5],
        delta: [0.0; 3],
        move_vector: [0.0; 2],
        analogue_move_vector: [0.0; 2],
        raw_move_vector: [0.0; 2],
        pitch: 0.0,
        yaw: 180.0,
        head_yaw: 180.0,
        camera_orientation: [0.0, 0.0, -1.0],
        flags: PlayerInputFlags::NONE,
        input_mode: PlayerInputMode::Mouse,
    }
}

fn request() -> BlockUseRequest {
    BlockUseRequest {
        hand: protocol::ItemUseHand::Mainhand,
        block_position: [0, 64, -2],
        face: 3,
        selected_slot: 2,
        selected_item: empty_hand(),
        player_position: movement().position,
        relative_hit: [0.5, 0.75, 1.0],
        block_runtime_id: 9,
    }
}

#[test]
fn independently_authored_empty_hand_use_is_one_embedded_pai_interaction() {
    let packet = player_auth_input_with_interactions(
        movement(),
        &PlayerAuthInputInteractions {
            block_actions: protocol::BlockActions::new(),
            block_interaction: Some(BlockItemInteraction::Use(request())),
        },
    )
    .unwrap();
    let packets::Packet::PlayerAuthInput(input) = *packet.data else {
        panic!("block use must not produce a standalone InventoryTransaction");
    };
    let input = *input;
    let input_data = input.input_data;
    assert!(input_data.contains(&InputData::Performiteminteraction));
    assert!(!input_data.contains(&InputData::Performblockactions));
    let packed = input
        .item_use_transaction
        .expect("one embedded item interaction");
    let transaction = packed.item_use_transaction;
    assert!(transaction.actions.actions.is_empty());
    assert_eq!(
        transaction.action_type,
        ItemUseInventoryTransactionActionType::Place
    );
    assert_eq!(
        transaction.trigger_type,
        ItemUseInventoryTransactionTriggerType::PlayerInput
    );
    assert_eq!(transaction.position.x.0, 0);
    assert_eq!(transaction.position.y.0, 64);
    assert_eq!(transaction.position.z.0, -2);
    assert_eq!(transaction.face.0, 3);
    assert_eq!(transaction.slot.0, 2);
    assert_eq!(transaction.item.id.0, 0);
    assert_eq!(transaction.item.stack_size.0, 0);
    assert_eq!(transaction.item.aux_value.0, 0);
    assert_eq!(transaction.item.block_runtime_id.0, 0);
    assert_eq!(transaction.item.net_id_variant, None);
    assert_eq!(transaction.from_position.x, movement().position[0]);
    assert_eq!(transaction.from_position.y, movement().position[1]);
    assert_eq!(transaction.from_position.z, movement().position[2]);
    assert_eq!(transaction.click_position.x, 0.5);
    assert_eq!(transaction.click_position.y, 0.75);
    assert_eq!(transaction.click_position.z, 1.0);
    assert_eq!(transaction.target_block_id.0, 9);
    assert_eq!(
        transaction.client_interact_prediction,
        ItemUseInventoryTransactionPredictedResult::Failure
    );
    assert_eq!(
        transaction.client_cooldown_state,
        ItemUseInventoryTransactionClientCooldownState::Off
    );
}

/// A destroy that wore the held tool records it under the legacy request, as a used stack does.
#[test]
fn embedded_destroy_carries_the_worn_tool_slot_change() {
    let tool = VerifiedNetworkItemStack::try_new(
        NetworkItemStack {
            network_id: 300,
            count: 1,
            stack_network_id: 12,
            ..NetworkItemStack::empty()
        },
        NetworkItemStack::empty().nbt_digest,
    )
    .unwrap();
    let worn = tool.worn(7, Some(250), -6).unwrap();
    let packet = player_auth_input_with_interactions(
        movement(),
        &PlayerAuthInputInteractions {
            block_actions: protocol::BlockActions::new(),
            block_interaction: Some(BlockItemInteraction::Destroy(
                BlockUseRequest {
                    selected_item: tool.clone(),
                    ..request()
                },
                Some(protocol::PredictedSlotChange {
                    legacy_request_id: -6,
                    from: tool,
                    to: worn,
                }),
            )),
        },
    )
    .unwrap();
    let encoded = protocol::encode(&packet).unwrap();
    let decoded = protocol::decode_batch(encoded).unwrap().remove(0);
    let packets::Packet::PlayerAuthInput(input) = *decoded.data else {
        panic!("expected movement input");
    };
    let packed = input.item_use_transaction.unwrap();
    assert_eq!(packed.legacy_request_id.0, -6);
    let slots = packed.legacy_set_item_slots.expect("legacy slot record");
    assert_eq!(slots.len(), 1);
    assert_eq!(
        slots[0].container_enum,
        bedrock_protocol_1_26_51::enums::ContainerEnumName::Inventorycontainer
    );
    assert_eq!(slots[0].slots.to_vec(), [request().selected_slot]);
    let actions = &packed.item_use_transaction.actions.actions;
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].source.container_id.map(|id| id.0), Some(0));
    assert_eq!(actions[0].slot.0, u32::from(request().selected_slot));
    assert_eq!(actions[0].from_item.id.0, 300);
    assert_eq!(actions[0].from_item.net_id_variant, None);
    assert_eq!(actions[0].to_item.id.0, 300);
    assert_eq!(actions[0].to_item.net_id_variant, None);
}

#[test]
fn embedded_destroy_clears_placement_fields_and_preserves_selection_and_pose() {
    let packet = player_auth_input_with_interactions(
        movement(),
        &PlayerAuthInputInteractions {
            block_actions: protocol::BlockActions::new(),
            block_interaction: Some(BlockItemInteraction::Destroy(request(), None)),
        },
    )
    .unwrap();
    let encoded = protocol::encode(&packet).unwrap();
    let decoded = protocol::decode_batch(encoded).unwrap().remove(0);
    let packets::Packet::PlayerAuthInput(input) = *decoded.data else {
        panic!("expected movement input");
    };
    assert!(
        input
            .input_data
            .contains(&InputData::Performiteminteraction)
    );
    let packed = input.item_use_transaction.unwrap();
    assert_eq!(packed.legacy_request_id.0, 0);
    assert!(packed.legacy_set_item_slots.is_none());
    let transaction = packed.item_use_transaction;
    assert_eq!(
        transaction.action_type,
        ItemUseInventoryTransactionActionType::Destroy
    );
    assert_eq!(
        transaction.trigger_type,
        ItemUseInventoryTransactionTriggerType::Unknown
    );
    assert_eq!(transaction.click_position.to_array(), [0.0; 3]);
    assert_eq!(transaction.target_block_id.0, 0);
    assert_eq!(
        transaction.from_position.to_array(),
        request().player_position
    );
    assert_eq!(transaction.position.x.0, request().block_position[0]);
    assert_eq!(transaction.position.y.0, request().block_position[1]);
    assert_eq!(transaction.position.z.0, request().block_position[2]);
    assert_eq!(transaction.face.0, request().face);
    assert_eq!(transaction.slot.0, i32::from(request().selected_slot));
    assert_eq!(transaction.item.id.0, 0);
    assert!(transaction.actions.actions.is_empty());
    assert_eq!(
        transaction.client_interact_prediction,
        ItemUseInventoryTransactionPredictedResult::Failure
    );
    assert_eq!(
        transaction.client_cooldown_state,
        ItemUseInventoryTransactionClientCooldownState::Off
    );
}

#[test]
fn destroy_and_use_are_one_mutually_exclusive_payload_slot() {
    let use_interaction = BlockItemInteraction::Use(request());
    let destroy_interaction = BlockItemInteraction::Destroy(request(), None);
    assert_ne!(use_interaction, destroy_interaction);
}

#[test]
fn provisional_use_reuses_bounded_block_request_validation() {
    let mut invalid = request();
    invalid.face = 6;
    assert_eq!(
        player_auth_input_with_interactions(
            movement(),
            &PlayerAuthInputInteractions {
                block_actions: protocol::BlockActions::new(),
                block_interaction: Some(BlockItemInteraction::Use(invalid)),
            },
        ),
        Err(protocol::PlayerAuthInputError::Interaction(
            protocol::InteractionEncodeError::InvalidBlockUse(
                protocol::BlockUsePacketError::InvalidFace(6),
            ),
        )),
    );
}

#[test]
fn filled_use_matches_independent_pinned_movement_fixture() {
    use sha2::{Digest, Sha256};
    use std::sync::Arc;
    let fixture = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/player_auth_input_use_block.bin"
    ))
    .expect("generate the pinned embedded filled Use fixture before verification");
    let packets = protocol::decode_batch(bytes::Bytes::from(fixture.clone())).unwrap();
    let decoded = &packets[0];
    let packets::Packet::PlayerAuthInput(input) = &*decoded.data else {
        panic!("expected PAI");
    };
    let packed = input.item_use_transaction.as_ref().unwrap();
    let transaction = &packed.item_use_transaction;
    assert_eq!(transaction.item.id.0, 5);
    assert_eq!(transaction.item.stack_size.0, 37);
    assert_eq!(
        transaction.item.net_id_variant,
        Some(bedrock_protocol_1_26_51::wire::ZigZag32(41))
    );
    assert_eq!(transaction.item.block_runtime_id.0, 0x8765_4321);
    assert!(transaction.actions.actions.is_empty());
    let extra: Arc<[u8]> = Arc::from(transaction.item.user_data_buffer.as_ref());
    let digest = Sha256::digest(&extra).into();
    let selected = NetworkItemStack {
        network_id: 5,
        metadata: 3,
        stack_network_id: 41,
        count: 37,
        nbt_digest: digest,
        block_runtime_id: i32::from_ne_bytes(0x8765_4321_u32.to_ne_bytes()),
        extra_data: extra,
    };
    let interactions = PlayerAuthInputInteractions {
        block_actions: protocol::BlockActions::new(),
        block_interaction: Some(BlockItemInteraction::Use(BlockUseRequest {
            hand: protocol::ItemUseHand::Mainhand,
            block_position: [13, 71, -29],
            face: 5,
            selected_slot: 7,
            selected_item: VerifiedNetworkItemStack::try_new(selected, digest).unwrap(),
            player_position: [13.25, 72.625, -28.75],
            relative_hit: [0.125, 0.875, 0.625],
            block_runtime_id: 123456,
        })),
    };
    let snapshot = PlayerAuthInputSnapshot {
        tick: 1234,
        position: [1.25, 64.0, -2.5],
        delta: [0.25, 0.0, -0.5],
        move_vector: [-1.0, 1.0],
        analogue_move_vector: [-1.0, 1.0],
        raw_move_vector: [-1.0, 1.0],
        pitch: 10.5,
        yaw: 20.25,
        head_yaw: 30.75,
        camera_orientation: [0.25, -0.5, -0.75],
        flags: PlayerInputFlags::UP
            | PlayerInputFlags::LEFT
            | PlayerInputFlags::JUMPING
            | PlayerInputFlags::SPRINTING,
        input_mode: PlayerInputMode::Mouse,
    };
    let mut rebuilt =
        protocol::player_auth_input_with_interactions(snapshot, &interactions).unwrap();
    rebuilt.header.from_subclient = 1;
    rebuilt.header.to_subclient = 2;
    let expected = super::selected_descriptors::without_selected_identity(
        protocol::decode_batch(bytes::Bytes::from(fixture))
            .unwrap()
            .remove(0),
    );
    assert_eq!(
        protocol::encode(&rebuilt).unwrap(),
        protocol::encode(&expected).unwrap()
    );
}

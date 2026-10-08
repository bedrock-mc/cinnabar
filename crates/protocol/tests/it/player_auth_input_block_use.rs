use protocol::{
    BlockItemInteraction, BlockUseRequest, NetworkItemStack, PlayerAuthInputInteractions,
    PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode, VerifiedNetworkItemStack,
    player_auth_input_with_interactions,
};
use valentine::bedrock::version::v1_26_51::{
    EnumsItemUseInventoryTransactionActionType,
    EnumsItemUseInventoryTransactionClientCooldownState,
    EnumsItemUseInventoryTransactionPredictedResult, EnumsItemUseInventoryTransactionTriggerType,
    EnumsPlayerAuthInputPacketPayloadInputData as InputData, McpePacketData,
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
    let McpePacketData::PlayerAuthInputPacket(input) = packet.data else {
        panic!("block use must not produce a standalone InventoryTransaction");
    };
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
        EnumsItemUseInventoryTransactionActionType::Place
    );
    assert_eq!(
        transaction.trigger_type,
        EnumsItemUseInventoryTransactionTriggerType::Playerinput
    );
    assert_eq!(transaction.position.x, 0);
    assert_eq!(transaction.position.y, 64);
    assert_eq!(transaction.position.z, -2);
    assert_eq!(transaction.face, 3);
    assert_eq!(transaction.slot, 2);
    assert_eq!(transaction.item.id, 0);
    assert_eq!(transaction.item.stacksize, 0);
    assert_eq!(transaction.item.auxvalue, 0);
    assert_eq!(transaction.item.block_runtime_id, 0);
    assert_eq!(transaction.item.net_id_variant, None);
    assert_eq!(transaction.from_position.x, movement().position[0]);
    assert_eq!(transaction.from_position.y, movement().position[1]);
    assert_eq!(transaction.from_position.z, movement().position[2]);
    assert_eq!(transaction.click_position.x, 0.5);
    assert_eq!(transaction.click_position.y, 0.75);
    assert_eq!(transaction.click_position.z, 1.0);
    assert_eq!(transaction.target_block_id, 9);
    assert_eq!(
        transaction.client_interact_prediction,
        EnumsItemUseInventoryTransactionPredictedResult::Failure
    );
    assert_eq!(
        transaction.client_cooldown_state,
        EnumsItemUseInventoryTransactionClientCooldownState::Off
    );
}

#[test]
fn destroy_and_use_are_one_mutually_exclusive_payload_slot() {
    let use_interaction = BlockItemInteraction::Use(request());
    let destroy_interaction = BlockItemInteraction::Destroy(request());
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
    let session = protocol::BedrockSession { shield_item_id: 0 };
    let packets = protocol::decode_batch(bytes::Bytes::from(fixture.clone()), &session).unwrap();
    let decoded = &packets[0];
    let McpePacketData::PlayerAuthInputPacket(input) = &decoded.data else {
        panic!("expected PAI");
    };
    let packed = input.item_use_transaction.as_ref().unwrap();
    let transaction = &packed.item_use_transaction;
    assert_eq!(transaction.item.id, 5);
    assert_eq!(transaction.item.stacksize, 37);
    assert_eq!(transaction.item.net_id_variant, Some(41));
    assert_eq!(transaction.item.block_runtime_id, 0x8765_4321);
    assert!(transaction.actions.actions.is_empty());
    let extra: Arc<[u8]> = Arc::from(transaction.item.user_data_buffer.clone());
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
    assert_eq!(
        protocol::encode(&rebuilt, &session).unwrap().as_ref(),
        fixture
    );
}

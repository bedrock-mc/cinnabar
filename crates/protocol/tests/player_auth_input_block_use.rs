use protocol::{
    BlockItemInteraction, BlockUseRequest, NetworkItemStack, PlayerAuthInputInteractions,
    PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode, VerifiedNetworkItemStack,
    player_auth_input_with_interactions,
};
use valentine::bedrock::version::v1_26_44::{
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
    let input_data = input.input_data.unwrap();
    assert!(input_data.contains(&InputData::PerformItemInteraction));
    assert!(!input_data.contains(&InputData::PerformBlockActions));
    let packed = input
        .item_use_transaction
        .and_then(|outer| outer)
        .expect("one embedded item interaction");
    let transaction = packed
        .item_use_transaction
        .expect("one item-use transaction");
    assert_eq!(transaction.actions.actions, None);
    assert_eq!(
        transaction.action_type,
        EnumsItemUseInventoryTransactionActionType::Place
    );
    assert_eq!(
        transaction.trigger_type,
        EnumsItemUseInventoryTransactionTriggerType::PlayerInput
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

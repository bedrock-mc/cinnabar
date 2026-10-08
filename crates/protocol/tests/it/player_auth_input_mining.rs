use bytes::Bytes;
use protocol::{
    BedrockSession, BlockAction, BlockActionKind, MineBlockRequest, MineBlockRequestError,
    PlayerAuthInputInteractions, PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode,
    decode_batch, encode, player_auth_input_with_interactions,
    player_auth_input_with_mining_request,
};
use valentine::bedrock::{
    codec::BedrockCodec,
    version::v1_26_51::{
        EnumsItemStackRequestActionType, EnumsPlayerAuthInputPacketPayloadInputData as InputData,
        ItemStackRequestCerealRequestDataActionsItem, McpePacketData,
    },
};

fn snapshot() -> PlayerAuthInputSnapshot {
    PlayerAuthInputSnapshot {
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
    }
}

fn interactions(predict: bool) -> PlayerAuthInputInteractions {
    let mut value = PlayerAuthInputInteractions::default();
    if predict {
        value
            .block_actions
            .push(BlockAction {
                kind: BlockActionKind::PredictDestroy,
                position: [13, 71, -29],
                face: 5,
            })
            .unwrap();
    }
    value
}

#[test]
fn mining_request_admission_is_bounded_not_current_item_authority() {
    for id in [-2, -1, 0, 1, i32::MIN] {
        assert_eq!(
            MineBlockRequest::new(id, 0, 0, 1),
            Err(MineBlockRequestError::InvalidRequestId)
        );
    }
    for slot in [9, u8::MAX] {
        assert_eq!(
            MineBlockRequest::new(-3, slot, 0, 1),
            Err(MineBlockRequestError::InvalidHotbarSlot)
        );
    }
    assert_eq!(
        MineBlockRequest::new(-3, 0, -1, 1),
        Err(MineBlockRequestError::InvalidPredictedDurability)
    );
    for id in [i32::MIN, -1, 0] {
        assert_eq!(
            MineBlockRequest::new(-3, 0, 0, id),
            Err(MineBlockRequestError::InvalidStackNetworkId)
        );
    }
    assert!(MineBlockRequest::new(i32::MIN + 1, 8, i32::MAX, i32::MAX).is_ok());
}

#[test]
fn mining_request_flags_and_independent_optional_prediction_match_pinned_fixtures() {
    let session = BedrockSession { shield_item_id: 0 };
    for (predict, fixture) in [
        (
            false,
            include_bytes!("../../fixtures/player_auth_input_mine_block.bin").as_slice(),
        ),
        (
            true,
            include_bytes!("../../fixtures/player_auth_input_mine_block_and_predict.bin")
                .as_slice(),
        ),
    ] {
        let mut built = player_auth_input_with_mining_request(
            snapshot(),
            &interactions(predict),
            Some(MineBlockRequest::new(-3, 2, 7, 12345).unwrap()),
        )
        .unwrap();
        built.header.from_subclient = 1;
        built.header.to_subclient = 2;
        let mut canonical_flags = vec![
            InputData::Jumping,
            InputData::Up,
            InputData::Left,
            InputData::Sprinting,
        ];
        if predict {
            canonical_flags.push(InputData::Performblockactions);
        }
        canonical_flags.push(InputData::Performitemstackrequest);
        let McpePacketData::PlayerAuthInputPacket(built_input) = &built.data else {
            panic!("expected input")
        };
        assert_eq!(&built_input.input_data, &canonical_flags);
        let canonical_bytes = encode(&built, &session).unwrap();
        let canonical_decoded = decode_batch(canonical_bytes.clone(), &session).unwrap();
        assert_eq!(canonical_decoded.len(), 1);
        assert_eq!(canonical_decoded[0], built);
        assert_eq!(
            encode(&canonical_decoded[0], &session).unwrap(),
            canonical_bytes
        );
        let decoded = decode_batch(Bytes::copy_from_slice(fixture), &session).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(encode(&decoded[0], &session).unwrap().as_ref(), fixture);
        let McpePacketData::PlayerAuthInputPacket(input) = &decoded[0].data else {
            panic!("expected input")
        };
        let flags = &input.input_data;
        // The pinned producer appends request then prediction; the snapshot
        // encoder emits ascending IDs. Both unique lists preserve their order.
        let mut fixture_flags = canonical_flags.clone();
        if predict {
            fixture_flags.swap(4, 5);
        }
        assert_eq!(flags, &fixture_flags);
        let mut fixture_order_built = built.clone();
        let McpePacketData::PlayerAuthInputPacket(fixture_order_input) =
            &mut fixture_order_built.data
        else {
            panic!("expected input")
        };
        fixture_order_input.input_data = fixture_flags;
        assert_eq!(fixture_order_built, decoded[0]);
        assert_eq!(
            encode(&fixture_order_built, &session).unwrap().as_ref(),
            fixture
        );
        assert!(flags.contains(&InputData::Performitemstackrequest));
        assert_eq!(flags.contains(&InputData::Performblockactions), predict);
        assert!(!flags.contains(&InputData::Performiteminteraction));
        assert_eq!(input.item_use_transaction, None);
        let request = input.item_stack_request.as_ref().unwrap();
        assert_eq!(request.client_request_id.id, -3);
        assert_eq!(request.actions.len(), 1);
        assert!(request.strings_to_filter.is_empty());
        let ItemStackRequestCerealRequestDataActionsItem::MineBlockActionData(action) =
            &request.actions[0]
        else {
            panic!("expected mining action")
        };
        assert_eq!(
            action.actiontype,
            EnumsItemStackRequestActionType::Screenhudmineblock
        );
        assert_eq!(
            (
                action.slot,
                action.predicted_durability,
                action.net_id_variant
            ),
            (2, 7, 12345)
        );
        let mut encoded = Vec::new();
        request.actions[0].encode(&mut encoded).unwrap();
        assert_eq!(&encoded[..2], &[9, 11]);
    }
}

#[test]
fn absent_mining_request_preserves_existing_input_and_prediction_bytes() {
    let session = BedrockSession { shield_item_id: 0 };
    for predict in [false, true] {
        let value = interactions(predict);
        let old = player_auth_input_with_interactions(snapshot(), &value).unwrap();
        let absent = player_auth_input_with_mining_request(snapshot(), &value, None).unwrap();
        assert_eq!(
            encode(&old, &session).unwrap(),
            encode(&absent, &session).unwrap()
        );
        let McpePacketData::PlayerAuthInputPacket(input) = absent.data else {
            panic!("expected input")
        };
        assert_eq!(input.item_stack_request, None);
        assert!(
            !input
                .input_data
                .contains(&InputData::Performitemstackrequest)
        );
    }
    let mut absent =
        player_auth_input_with_mining_request(snapshot(), &interactions(false), None).unwrap();
    absent.header.from_subclient = 1;
    absent.header.to_subclient = 2;
    assert_eq!(
        encode(&absent, &session).unwrap().as_ref(),
        include_bytes!("../../fixtures/player_auth_input.bin")
    );
}

#[test]
fn caller_cannot_assert_request_presence_without_encoder_ownership() {
    let mut value = snapshot();
    value.flags |= PlayerInputFlags::PERFORM_ITEM_STACK_REQUEST;
    for request in [None, Some(MineBlockRequest::new(-3, 0, 0, 1).unwrap())] {
        assert!(
            player_auth_input_with_mining_request(value, &interactions(false), request).is_err()
        );
    }
}

#[test]
fn start_game_block_breaking_mode_preserves_explicit_false() {
    let mut game_data = protocol::GameData {
        start_game: Default::default(),
        item_registry: Default::default(),
        biome_definitions: None,
        entity_identifiers: None,
        creative_content: None,
    };
    game_data
        .start_game
        .movement_settings
        .server_authoritative_block_breaking = true;
    assert!(protocol::server_authoritative_block_breaking(&game_data));
    game_data
        .start_game
        .movement_settings
        .server_authoritative_block_breaking = false;
    assert!(!protocol::server_authoritative_block_breaking(&game_data));
}

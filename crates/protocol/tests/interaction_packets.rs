use std::sync::Arc;

use bytes::Bytes;
use protocol::{
    ActorUseAction, ActorUsePacketError, ActorUseRequest, BedrockSession, BlockUsePacketError,
    BlockUseRequest, InventoryPacketError, NetworkItemStack, VerifiedNetworkItemStack,
    click_block_packet, decode_batch, destroy_block_packet, encode, use_actor_packet,
};
use sha2::{Digest, Sha256};
use valentine::bedrock::version::v1_26_44::{
    ContainerClosePacket, EnumsItemUseInventoryTransactionActionType,
    EnumsItemUseInventoryTransactionClientCooldownState,
    EnumsItemUseInventoryTransactionPredictedResult, EnumsItemUseInventoryTransactionTriggerType,
    EnumsItemUseOnActorInventoryTransactionActionType, InventoryTransactionPacketTransaction,
    McpePacketData, McpePacketName,
};

const CLICK_BLOCK: &[u8] = include_bytes!("../fixtures/inventory_transaction_click_block.bin");
const CLICK_BLOCK_EMPTY_HAND: &[u8] =
    include_bytes!("../fixtures/inventory_transaction_click_block_empty_hand.bin");
const DESTROY_BLOCK: &[u8] = include_bytes!("../fixtures/inventory_transaction_destroy_block.bin");
const DESTROY_BLOCK_EMPTY_HAND: &[u8] =
    include_bytes!("../fixtures/inventory_transaction_destroy_block_empty_hand.bin");
const ATTACK_ACTOR: &[u8] = include_bytes!("../fixtures/inventory_transaction_attack_actor.bin");
const ATTACK_ACTOR_EMPTY_HAND: &[u8] =
    include_bytes!("../fixtures/inventory_transaction_attack_actor_empty_hand.bin");
const INTERACT_ACTOR: &[u8] =
    include_bytes!("../fixtures/inventory_transaction_interact_actor.bin");
const INTERACT_ACTOR_EMPTY_HAND: &[u8] =
    include_bytes!("../fixtures/inventory_transaction_interact_actor_empty_hand.bin");
const CONTAINER_CLOSE: &[u8] = include_bytes!("../fixtures/container_close.bin");

fn session() -> BedrockSession {
    BedrockSession { shield_item_id: 0 }
}

fn decode_one(fixture: &'static [u8], id: McpePacketName) -> protocol::Packet {
    let packets = decode_batch(Bytes::from_static(fixture), &session()).expect("decode fixture");
    assert_eq!(packets.len(), 1);
    let packet = packets.into_iter().next().expect("one packet");
    assert_eq!(packet.header.id, id);
    assert_eq!(packet.header.from_subclient, 1);
    assert_eq!(packet.header.to_subclient, 2);
    packet
}

fn assert_click_block_constants(packet: &protocol::Packet) {
    assert_block_use_constants(packet, EnumsItemUseInventoryTransactionActionType::Place);
}

fn assert_block_use_constants(
    packet: &protocol::Packet,
    expected_action: EnumsItemUseInventoryTransactionActionType,
) {
    let McpePacketData::InventoryTransactionPacket(packet) = &packet.data else {
        panic!("expected inventory transaction");
    };
    assert_eq!(packet.legacy_request_id.id, 0);
    assert!(packet.legacy_set_item_slots.is_none());
    let Some(InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction)) =
        &packet.transaction
    else {
        panic!("expected item-use transaction");
    };
    assert_eq!(transaction.actions.actions, Some(Vec::new()));
    assert_eq!(transaction.action_type, expected_action);
    assert_eq!(
        transaction.trigger_type,
        EnumsItemUseInventoryTransactionTriggerType::PlayerInput
    );
    assert_eq!(
        transaction.client_interact_prediction,
        EnumsItemUseInventoryTransactionPredictedResult::Failure
    );
    assert_eq!(
        transaction.client_cooldown_state,
        EnumsItemUseInventoryTransactionClientCooldownState::Off
    );
}

fn verified_fixture_item(packet: &protocol::Packet) -> VerifiedNetworkItemStack {
    let McpePacketData::InventoryTransactionPacket(packet) = &packet.data else {
        panic!("expected inventory transaction");
    };
    let Some(InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction)) =
        &packet.transaction
    else {
        panic!("expected item-use transaction");
    };
    let item = &transaction.item;
    let digest: [u8; 32] = Sha256::digest(&item.user_data_buffer).into();
    VerifiedNetworkItemStack::try_new(
        NetworkItemStack {
            network_id: i32::from(item.id),
            metadata: u32::from_ne_bytes(item.auxvalue.to_ne_bytes()),
            stack_network_id: item.net_id_variant.unwrap_or(-1),
            count: item.stacksize,
            nbt_digest: digest,
            block_runtime_id: i32::from_ne_bytes(item.block_runtime_id.to_ne_bytes()),
            extra_data: Arc::from(item.user_data_buffer.clone()),
        },
        digest,
    )
    .expect("fixture item is verified")
}

fn verified_actor_fixture_item(packet: &protocol::Packet) -> VerifiedNetworkItemStack {
    let McpePacketData::InventoryTransactionPacket(packet) = &packet.data else {
        panic!("expected inventory transaction");
    };
    let Some(InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
        transaction,
    )) = &packet.transaction
    else {
        panic!("expected item-use-on-actor transaction");
    };
    let item = &transaction.item;
    let digest: [u8; 32] = Sha256::digest(&item.user_data_buffer).into();
    VerifiedNetworkItemStack::try_new(
        NetworkItemStack {
            network_id: i32::from(item.id),
            metadata: u32::from_ne_bytes(item.auxvalue.to_ne_bytes()),
            stack_network_id: item.net_id_variant.unwrap_or(-1),
            count: item.stacksize,
            nbt_digest: digest,
            block_runtime_id: i32::from_ne_bytes(item.block_runtime_id.to_ne_bytes()),
            extra_data: Arc::from(item.user_data_buffer.clone()),
        },
        digest,
    )
    .expect("fixture item is verified")
}

fn assert_actor_use_constants(packet: &protocol::Packet, action: ActorUseAction) {
    let McpePacketData::InventoryTransactionPacket(packet) = &packet.data else {
        panic!("expected inventory transaction");
    };
    assert_eq!(packet.legacy_request_id.id, 0);
    assert!(packet.legacy_set_item_slots.is_none());
    let Some(InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
        transaction,
    )) = &packet.transaction
    else {
        panic!("expected item-use-on-actor transaction");
    };
    assert_eq!(transaction.actions.actions, Some(Vec::new()));
    assert_eq!(
        transaction.action_type,
        match action {
            ActorUseAction::Attack => EnumsItemUseOnActorInventoryTransactionActionType::Attack,
            ActorUseAction::Interact => EnumsItemUseOnActorInventoryTransactionActionType::Interact,
        }
    );
}

fn assert_actor_builder_matches_fixture(
    fixture: &'static [u8],
    action: ActorUseAction,
    actor_runtime_id: u64,
    selected_slot: u8,
    player_position: [f32; 3],
    hit_position: [f32; 3],
) {
    let decoded = decode_one(fixture, McpePacketName::InventoryTransactionPacket);
    assert_actor_use_constants(&decoded, action);
    let mut built = use_actor_packet(
        ActorUseRequest {
            actor_runtime_id,
            action,
            selected_slot,
            selected_item: verified_actor_fixture_item(&decoded),
            player_position,
            hit_position,
        },
        &session(),
    )
    .expect("valid actor-use request");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    assert_eq!(encode(&built, &session()).unwrap().as_ref(), fixture);
}

fn assert_builder_matches_fixture(fixture: &'static [u8], request: BlockUseRequest) {
    let mut built = click_block_packet(request, &session()).expect("valid click-block request");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    let encoded = encode(&built, &session()).expect("encode builder packet");
    assert_eq!(encoded.as_ref(), fixture);
}

fn assert_destroy_builder_matches_fixture(fixture: &'static [u8], request: BlockUseRequest) {
    let mut built = destroy_block_packet(request, &session()).expect("valid destroy-block request");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    let encoded = encode(&built, &session()).expect("encode builder packet");
    assert_eq!(encoded.as_ref(), fixture);
}

#[test]
fn filled_click_block_fixture_cross_decodes_and_builder_matches_exactly() {
    let packet = decode_one(CLICK_BLOCK, McpePacketName::InventoryTransactionPacket);
    assert_click_block_constants(&packet);
    let selected_item = verified_fixture_item(&packet);
    assert_builder_matches_fixture(
        CLICK_BLOCK,
        BlockUseRequest {
            block_position: [13, 71, -29],
            face: 5,
            selected_slot: 7,
            selected_item,
            player_position: [13.25, 72.625, -28.75],
            relative_hit: [0.125, 0.875, 0.625],
            block_runtime_id: 123_456,
        },
    );
}

#[test]
fn empty_hand_click_block_fixture_cross_decodes_and_builder_matches_exactly() {
    let packet = decode_one(
        CLICK_BLOCK_EMPTY_HAND,
        McpePacketName::InventoryTransactionPacket,
    );
    assert_click_block_constants(&packet);
    assert_builder_matches_fixture(
        CLICK_BLOCK_EMPTY_HAND,
        BlockUseRequest {
            block_position: [-8, 63, 21],
            face: 0,
            selected_slot: 0,
            selected_item: verified_fixture_item(&packet),
            player_position: [-7.75, 64.5, 21.875],
            relative_hit: [0.75, 0.25, 0.5],
            block_runtime_id: u64::from(u32::MAX),
        },
    );
}

#[test]
fn destroy_block_fixtures_cross_decode_and_build_byte_exactly() {
    let filled = decode_one(DESTROY_BLOCK, McpePacketName::InventoryTransactionPacket);
    assert_block_use_constants(&filled, EnumsItemUseInventoryTransactionActionType::Destroy);
    assert_destroy_builder_matches_fixture(
        DESTROY_BLOCK,
        BlockUseRequest {
            block_position: [24, 68, -41],
            face: 3,
            selected_slot: 5,
            selected_item: verified_fixture_item(&filled),
            player_position: [24.625, 69.5, -40.125],
            relative_hit: [0.625, 0.375, 0.875],
            block_runtime_id: 654_321,
        },
    );

    let empty = decode_one(
        DESTROY_BLOCK_EMPTY_HAND,
        McpePacketName::InventoryTransactionPacket,
    );
    assert_block_use_constants(&empty, EnumsItemUseInventoryTransactionActionType::Destroy);
    assert_destroy_builder_matches_fixture(
        DESTROY_BLOCK_EMPTY_HAND,
        BlockUseRequest {
            block_position: [-17, 92, 6],
            face: 1,
            selected_slot: 0,
            selected_item: verified_fixture_item(&empty),
            player_position: [-16.5, 93.625, 6.25],
            relative_hit: [0.5, 1.0, 0.25],
            block_runtime_id: u64::from(u32::MAX),
        },
    );
}

#[test]
fn actor_use_fixtures_cross_decode_and_build_byte_exactly() {
    assert_actor_builder_matches_fixture(
        ATTACK_ACTOR,
        ActorUseAction::Attack,
        0x0102_0304_0506_0708,
        8,
        [10.25, 65.625, -4.75],
        [0.375, 1.25, -0.125],
    );
    assert_actor_builder_matches_fixture(
        ATTACK_ACTOR_EMPTY_HAND,
        ActorUseAction::Attack,
        u64::MAX,
        0,
        [-12.5, 70.0, 31.75],
        [-0.5, 0.625, 1.5],
    );
    assert_actor_builder_matches_fixture(
        INTERACT_ACTOR,
        ActorUseAction::Interact,
        123_456_789,
        3,
        [2.5, 63.875, 9.125],
        [0.25, 0.75, 0.5],
    );
    assert_actor_builder_matches_fixture(
        INTERACT_ACTOR_EMPTY_HAND,
        ActorUseAction::Interact,
        1,
        6,
        [-1.25, 80.5, -16.75],
        [1.125, -0.25, 0.875],
    );
}

#[test]
fn container_close_fixture_cross_decodes_and_round_trips_exactly() {
    let packet = decode_one(CONTAINER_CLOSE, McpePacketName::ContainerClosePacket);
    assert_eq!(
        packet.data,
        McpePacketData::ContainerClosePacket(ContainerClosePacket {
            container_id: 5,
            container_type: 0,
            server_initiated_close: false,
        })
    );
    assert_eq!(
        encode(&packet, &session()).unwrap().as_ref(),
        CONTAINER_CLOSE
    );
}

fn base_request() -> BlockUseRequest {
    let digest: [u8; 32] = Sha256::digest([]).into();
    BlockUseRequest {
        block_position: [0, 64, 0],
        face: 1,
        selected_slot: 4,
        selected_item: VerifiedNetworkItemStack::try_new(NetworkItemStack::empty(), digest)
            .expect("canonical empty item is verified"),
        player_position: [0.5, 65.62, 0.5],
        relative_hit: [0.5, 1.0, 0.5],
        block_runtime_id: 1,
    }
}

#[test]
fn click_block_builder_rejects_invalid_face_slot_position_and_runtime_id() {
    let mut request = base_request();
    request.face = 6;
    assert_eq!(
        click_block_packet(request, &session()).unwrap_err(),
        BlockUsePacketError::InvalidFace(6)
    );

    let mut request = base_request();
    request.selected_slot = 9;
    assert_eq!(
        click_block_packet(request, &session()).unwrap_err(),
        BlockUsePacketError::InvalidSelectedSlot(9)
    );

    let mut request = base_request();
    request.player_position[1] = f32::INFINITY;
    assert_eq!(
        click_block_packet(request, &session()).unwrap_err(),
        BlockUsePacketError::NonFinitePlayerPosition
    );

    let mut request = base_request();
    request.relative_hit[2] = f32::NAN;
    assert_eq!(
        click_block_packet(request, &session()).unwrap_err(),
        BlockUsePacketError::NonFiniteRelativeHit
    );

    let mut request = base_request();
    request.relative_hit[0] = f32::INFINITY;
    assert_eq!(
        click_block_packet(request, &session()).unwrap_err(),
        BlockUsePacketError::NonFiniteRelativeHit
    );

    let mut request = base_request();
    request.block_runtime_id = u64::from(u32::MAX) + 1;
    assert_eq!(
        click_block_packet(request, &session()).unwrap_err(),
        BlockUsePacketError::BlockRuntimeIdOutOfRange(u64::from(u32::MAX) + 1)
    );

    let mut request = base_request();
    request.selected_item = oversized_wire_item();
    assert_eq!(
        click_block_packet(request, &session()).unwrap_err(),
        BlockUsePacketError::InvalidSelectedItem(InventoryPacketError::InvalidItemNetworkId(
            i32::from(i16::MAX) + 1,
        ))
    );
}

fn oversized_wire_item() -> VerifiedNetworkItemStack {
    let digest: [u8; 32] = Sha256::digest([]).into();
    VerifiedNetworkItemStack::try_new(
        NetworkItemStack {
            network_id: i32::from(i16::MAX) + 1,
            metadata: 0,
            stack_network_id: 1,
            count: 1,
            nbt_digest: digest,
            block_runtime_id: 0,
            extra_data: Arc::from([]),
        },
        digest,
    )
    .expect("item is valid before conversion to the protocol-2168 i16 carrier")
}

#[test]
fn destroy_block_builder_shares_block_use_validation() {
    let invalid_requests = [
        {
            let mut request = base_request();
            request.face = 6;
            request
        },
        {
            let mut request = base_request();
            request.selected_slot = 9;
            request
        },
        {
            let mut request = base_request();
            request.player_position[1] = f32::INFINITY;
            request
        },
        {
            let mut request = base_request();
            request.relative_hit[2] = f32::NAN;
            request
        },
        {
            let mut request = base_request();
            request.block_runtime_id = u64::from(u32::MAX) + 1;
            request
        },
        {
            let mut request = base_request();
            request.selected_item = oversized_wire_item();
            request
        },
    ];
    let expected_errors = [
        BlockUsePacketError::InvalidFace(6),
        BlockUsePacketError::InvalidSelectedSlot(9),
        BlockUsePacketError::NonFinitePlayerPosition,
        BlockUsePacketError::NonFiniteRelativeHit,
        BlockUsePacketError::BlockRuntimeIdOutOfRange(u64::from(u32::MAX) + 1),
        BlockUsePacketError::InvalidSelectedItem(InventoryPacketError::InvalidItemNetworkId(
            i32::from(i16::MAX) + 1,
        )),
    ];

    for (request, expected) in invalid_requests.into_iter().zip(expected_errors) {
        assert_eq!(
            destroy_block_packet(request, &session()).unwrap_err(),
            expected
        );
    }
}

#[test]
fn block_use_builders_reject_finite_relative_hits_outside_block_bounds() {
    for relative_hit in [
        [-f32::EPSILON, 0.375, 1.0],
        [0.0, 0.375, 1.0 + f32::EPSILON],
    ] {
        let mut request = base_request();
        request.relative_hit = relative_hit;
        assert_eq!(
            click_block_packet(request.clone(), &session()).unwrap_err(),
            BlockUsePacketError::RelativeHitOutOfRange
        );
        assert_eq!(
            destroy_block_packet(request, &session()).unwrap_err(),
            BlockUsePacketError::RelativeHitOutOfRange
        );
    }
}

#[test]
fn click_block_builder_preserves_exact_relative_hit_boundaries() {
    let mut request = base_request();
    request.relative_hit = [0.0, 1.0, 0.0];
    let packet = click_block_packet(request, &session()).expect("boundary request");
    let McpePacketData::InventoryTransactionPacket(packet) = packet.data else {
        panic!("expected inventory transaction");
    };
    let Some(InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction)) =
        packet.transaction
    else {
        panic!("expected item-use transaction");
    };
    assert_eq!(transaction.click_position.x.to_bits(), 0.0_f32.to_bits());
    assert_eq!(transaction.click_position.y.to_bits(), 1.0_f32.to_bits());
    assert_eq!(transaction.click_position.z.to_bits(), 0.0_f32.to_bits());
}

fn base_actor_request() -> ActorUseRequest {
    ActorUseRequest {
        actor_runtime_id: 1,
        action: ActorUseAction::Attack,
        selected_slot: 4,
        selected_item: base_request().selected_item,
        player_position: [0.5, 65.62, 0.5],
        hit_position: [0.0, 1.25, 0.0],
    }
}

#[test]
fn actor_use_builder_rejects_zero_runtime_invalid_slot_and_non_finite_positions() {
    let mut request = base_actor_request();
    request.actor_runtime_id = 0;
    assert_eq!(
        use_actor_packet(request, &session()).unwrap_err(),
        ActorUsePacketError::InvalidActorRuntimeId
    );

    let mut request = base_actor_request();
    request.selected_slot = 9;
    assert_eq!(
        use_actor_packet(request, &session()).unwrap_err(),
        ActorUsePacketError::InvalidSelectedSlot(9)
    );

    let mut request = base_actor_request();
    request.player_position[0] = f32::NEG_INFINITY;
    assert_eq!(
        use_actor_packet(request, &session()).unwrap_err(),
        ActorUsePacketError::NonFinitePlayerPosition
    );

    let mut request = base_actor_request();
    request.hit_position[1] = f32::NAN;
    assert_eq!(
        use_actor_packet(request, &session()).unwrap_err(),
        ActorUsePacketError::NonFiniteHitPosition
    );
}

#[test]
fn actor_use_builder_preserves_finite_out_of_unit_hit_offsets() {
    let request = base_actor_request();
    let packet = use_actor_packet(request.clone(), &session()).unwrap();
    let McpePacketData::InventoryTransactionPacket(packet) = packet.data else {
        panic!("expected inventory transaction");
    };
    let Some(InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
        transaction,
    )) = packet.transaction
    else {
        panic!("expected item-use-on-actor transaction");
    };
    assert_eq!(transaction.hit_position.x, request.hit_position[0]);
    assert_eq!(transaction.hit_position.y, request.hit_position[1]);
    assert_eq!(transaction.hit_position.z, request.hit_position[2]);
}

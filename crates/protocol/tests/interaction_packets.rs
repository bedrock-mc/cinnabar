use std::sync::Arc;

use bytes::Bytes;
use protocol::{
    BedrockSession, BlockUsePacketError, BlockUseRequest, NetworkItemStack,
    VerifiedNetworkItemStack, click_block_packet, decode_batch, encode,
};
use sha2::{Digest, Sha256};
use valentine::bedrock::version::v1_26_40::{
    ContainerClosePacket, InventoryTransactionPacketTransaction,
    ItemUseInventoryTransactionActionType, ItemUseInventoryTransactionClientCooldownState,
    ItemUseInventoryTransactionClientInteractPrediction, ItemUseInventoryTransactionTriggerType,
    McpePacketData, McpePacketName,
};

const CLICK_BLOCK: &[u8] = include_bytes!("../fixtures/inventory_transaction_click_block.bin");
const CLICK_BLOCK_EMPTY_HAND: &[u8] =
    include_bytes!("../fixtures/inventory_transaction_click_block_empty_hand.bin");
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
    let McpePacketData::InventoryTransactionPacket(packet) = &packet.data else {
        panic!("expected inventory transaction");
    };
    assert_eq!(packet.legacy_request_id.id, 0);
    assert!(packet.legacy_set_item_slots.is_none());
    assert!(packet.constant_2, "transaction type presence must be true");

    let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction) =
        &packet.transaction
    else {
        panic!("expected item-use transaction");
    };
    assert!(
        transaction.actions.constant_0,
        "actions presence must be true"
    );
    assert!(transaction.actions.actions.is_empty());
    assert_eq!(
        transaction.action_type,
        ItemUseInventoryTransactionActionType::Place
    );
    assert_eq!(
        transaction.trigger_type,
        ItemUseInventoryTransactionTriggerType::PlayerInput
    );
    assert_eq!(
        transaction.client_interact_prediction,
        ItemUseInventoryTransactionClientInteractPrediction::Failure
    );
    assert_eq!(
        transaction.client_cooldown_state,
        ItemUseInventoryTransactionClientCooldownState::Off
    );
}

fn verified_fixture_item(packet: &protocol::Packet) -> VerifiedNetworkItemStack {
    let McpePacketData::InventoryTransactionPacket(packet) = &packet.data else {
        panic!("expected inventory transaction");
    };
    let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction) =
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
            block_runtime_id: item.block_runtime_id,
            extra_data: Arc::from(item.user_data_buffer.clone()),
        },
        digest,
    )
    .expect("fixture item is verified")
}

fn assert_builder_matches_fixture(fixture: &'static [u8], request: BlockUseRequest) {
    let mut built = click_block_packet(request, &session()).expect("valid click-block request");
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
    request.block_runtime_id = u64::from(u32::MAX) + 1;
    assert_eq!(
        click_block_packet(request, &session()).unwrap_err(),
        BlockUsePacketError::BlockRuntimeIdOutOfRange(u64::from(u32::MAX) + 1)
    );
}

#[test]
fn click_block_builder_clamps_finite_relative_hit_to_block_bounds() {
    let mut request = base_request();
    request.relative_hit = [-0.25, 0.375, 1.5];
    let packet = click_block_packet(request, &session()).expect("finite request");
    let McpePacketData::InventoryTransactionPacket(packet) = packet.data else {
        panic!("expected inventory transaction");
    };
    let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(transaction) =
        packet.transaction
    else {
        panic!("expected item-use transaction");
    };
    assert_eq!(transaction.click_position.x, 0.0);
    assert_eq!(transaction.click_position.y, 0.375);
    assert_eq!(transaction.click_position.z, 1.0);
}

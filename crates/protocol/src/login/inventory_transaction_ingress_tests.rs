//! Exact local vanilla BDS pickup body, captured 2026-10-02. No assets or tokens.

use bytes::{Buf, Bytes, BytesMut};
use valentine::bedrock::{context::BedrockSession, version::v1_26_51::*};

use crate::{InventoryEvent, WorldEvent};

fn pickup_body() -> Vec<u8> {
    let batch: Vec<u8> = include_str!("../../fixtures/inventory_transaction_pickup.hex")
        .split_whitespace()
        .map(|byte| u8::from_str_radix(byte, 16).unwrap())
        .collect();
    let mut frame = Bytes::from(batch);
    frame.advance(1); // Strip the uncompressed fixture's batch marker.
    jolyne::raw::decode_packet_raw(&mut frame)
        .unwrap()
        .body()
        .to_vec()
}

fn raw(body: &[u8]) -> jolyne::raw::RawPacket {
    let mut payload = BytesMut::new();
    valentine::protocol::wire::write_var_u32(
        &mut payload,
        McpePacketName::InventoryTransactionPacket as u32,
    );
    payload.extend_from_slice(body);
    let mut frame = BytesMut::new();
    valentine::protocol::wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    jolyne::raw::decode_packet_raw(&mut frame.freeze()).unwrap()
}

#[test]
fn vanilla_bds_pickup_enters_raw_ingress_with_absolute_count_id_and_extra() {
    let session = BedrockSession { shield_item_id: 0 };
    let event = super::decode_world_raw_with(raw(&pickup_body()), 0, |raw| raw.decode(&session))
        .unwrap()
        .expect("normal transaction is admitted");
    let WorldEvent::Inventory(InventoryEvent::Transaction(transaction)) = event else {
        panic!("pickup must be one atomic inventory event");
    };
    assert_eq!(
        transaction.skipped_actions, 0,
        "known world balancing leg is not odd data"
    );
    let [slot] = transaction.slots.as_ref() else {
        panic!("one real inventory write");
    };
    assert_eq!(
        slot.identity.container.window_id,
        Some(crate::PLAYER_INVENTORY_WINDOW_ID)
    );
    assert_eq!(slot.identity.slot, 6);
    assert_eq!(slot.stack.network_id, 3);
    assert_eq!(slot.stack.count, 64);
    assert_eq!(slot.stack.stack_network_id, 81);
    assert_eq!(slot.stack.extra_data.as_ref(), &[0; 10]);
    assert_ne!(slot.stack.nbt_digest, [0; 32]);
}

#[test]
fn truncated_pickup_is_a_fatal_wire_fault() {
    let session = BedrockSession { shield_item_id: 0 };
    let body = pickup_body();
    for length in 0..body.len() {
        let error =
            super::decode_world_raw_with(raw(&body[..length]), 0, |raw| raw.decode(&session))
                .expect_err("truncation cannot become semantic skip");
        let mut skipped = 0;
        assert!(
            super::skip_semantic_world_error(error, &mut skipped).is_err(),
            "length {length}"
        );
        assert_eq!(skipped, 0);
    }
}

#[test]
fn trailing_pickup_bytes_are_a_fatal_wire_fault() {
    let session = BedrockSession { shield_item_id: 0 };
    let mut body = pickup_body();
    body.push(0);
    let error =
        super::decode_world_raw_with(raw(&body), 0, |raw| raw.decode(&session)).unwrap_err();
    assert!(super::skip_semantic_world_error(error, &mut 0).is_err());
}

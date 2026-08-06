use bytes::{Bytes, BytesMut};
use jolyne::{
    batch::{decode_batch_raw, encode_batch_multi},
    error::JolyneError,
    raw::{RawPacket, decode_packet_raw},
    valentine::{
        MaterialReducerDataEntry, MaterialReducerDataEntryView, McpePacketArgs, McpePacketData,
        McpePacketName,
    },
};
use protocol::BedrockSession;
use valentine::bedrock::error::DecodeError;
use valentine::protocol::wire;

const GOPHERTUNNEL_MATERIAL_REDUCER: &[u8] = include_bytes!("../fixtures/material_reducer.bin");
const REDUCER_BODY: &[u8] = &[0x86, 0x80, 0xd0, 0x02, 0x02, 0x0e, 0x04, 0x11, 0x08];
const MAX_REDUCER_OUTPUTS: usize = 4_096;

/// The empty typed-vector counts that precede `MaterialReducers` on the wire.
///
/// gophertunnel `packet/crafting_data.go` at commit
/// be6713da4dc051a4197f897d04835e89e9c54321 marshals eight typed recipe slices
/// (shaped, shapeless, multi, shulker box, shapeless chemistry, shaped
/// chemistry, smithing transform, smithing trim) plus potion recipes and potion
/// container change recipes before `FuncSlice(io, &pk.MaterialReducers, ...)`.
/// Protocol 1001 had a single fused `Recipes` slice, so this prefix was three
/// zero counts rather than ten.
const EMPTY_RECIPE_VECTOR_COUNTS: [u8; 10] = [0; 10];

fn raw_fixture() -> RawPacket {
    let mut batch = Bytes::from_static(GOPHERTUNNEL_MATERIAL_REDUCER);
    decode_batch_raw(&mut batch, false, Some(1024))
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
}

fn raw_crafting_data_body(body: &[u8]) -> RawPacket {
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::CraftingDataPacket as u32);
    payload.extend_from_slice(body);
    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    decode_packet_raw(&mut frame.freeze()).expect("raw CraftingData frame")
}

fn assert_two_outputs(data: &McpePacketData) {
    let McpePacketData::CraftingDataPacket(packet) = data else {
        panic!("expected CraftingData, got {:?}", data.packet_id());
    };
    assert_eq!(packet.material_reducers.len(), 1);
    // 1.26.40 models the reducer directly instead of through the prismarine
    // `outputs` wrapper, so these are structural assertions rather than the
    // `Debug`-string probes protocol 1001 needed.
    let reducer = &packet.material_reducers[0];
    // gophertunnel `MaterialReducer.Marshal` packs the input item as
    // `NetworkID<<16 | MetadataValue`; the fixture uses network ID 42, meta 3.
    assert_eq!(reducer.from_item_key, (42 << 16) | 3);
    assert_eq!(reducer.item_idsand_counts.len(), 2);
    assert_eq!(reducer.item_idsand_counts[0].item_id, 7);
    assert_eq!(reducer.item_idsand_counts[0].item_count, 2);
    assert_eq!(reducer.item_idsand_counts[1].item_id, -9);
    assert_eq!(reducer.item_idsand_counts[1].item_count, 4);
}

#[test]
fn pinned_gophertunnel_material_reducer_owned_decodes_and_round_trips_exactly() {
    let packet = raw_fixture()
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("owned CraftingData decode");
    assert_two_outputs(&packet.data);
    let encoded = encode_batch_multi(&[packet], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_MATERIAL_REDUCER);
}

#[test]
fn pinned_gophertunnel_material_reducer_borrowed_materializes() {
    let borrowed = raw_fixture()
        .decode_borrowed()
        .expect("borrowed CraftingData decode");
    let owned = borrowed
        .data
        .into_owned(McpePacketArgs)
        .expect("materialize borrowed CraftingData");
    assert_two_outputs(&owned);
}

#[test]
fn material_reducer_view_decodes_the_complete_output_vector() {
    let mut body = Bytes::from_static(REDUCER_BODY);
    let view = MaterialReducerDataEntryView::decode(&mut body).expect("borrowed reducer decode");
    assert!(
        body.is_empty(),
        "borrowed reducer left {} bytes",
        body.len()
    );
    let owned = MaterialReducerDataEntry::from(view);
    assert_eq!(owned.item_idsand_counts.len(), 2);
    assert_eq!(owned.item_idsand_counts[0].item_id, 7);
    assert_eq!(owned.item_idsand_counts[0].item_count, 2);
    assert_eq!(owned.item_idsand_counts[1].item_id, -9);
    assert_eq!(owned.item_idsand_counts[1].item_count, 4);
}

/// A reducer output count above gophertunnel's slice ceiling must still fail.
///
/// REGRESSION - see the module header of `world_collection_bounds.rs`. Under
/// protocol 1001 this failed with `DecodeError::ArrayLengthExceeded { declared:
/// 4097, available: 4096 }` before allocating, matching gophertunnel's
/// `maxSliceLength = 4096` guard in `minecraft/protocol/io.go` at commit
/// be6713da4dc051a4197f897d04835e89e9c54321. The 1.26.40 generated crate emits
/// no collection ceilings, so the count is reserved first and the read only
/// fails once the outputs turn out to be absent. The `ArrayLengthExceeded` arm
/// below is the tripwire for the ceiling coming back.
#[test]
fn material_reducer_rejects_count_above_gophertunnel_slice_limit() {
    let mut body = EMPTY_RECIPE_VECTOR_COUNTS.to_vec();
    body.push(1);
    body.extend_from_slice(&REDUCER_BODY[..4]);
    body.extend_from_slice(&[0x81, 0x20]);
    let error = raw_crafting_data_body(&body)
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect_err("4,097 outputs must not decode");
    let JolyneError::PacketDecode { source, .. } = &error else {
        panic!("unexpected error: {error:?}");
    };
    match source {
        DecodeError::UnexpectedEof { .. } => {}
        DecodeError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof => {}
        DecodeError::ArrayLengthExceeded { .. } => panic!(
            "valentine_gen appears to emit collection ceilings again: restore the \
             `declared: 4_097, available: {MAX_REDUCER_OUTPUTS}` assertion here"
        ),
        other => panic!("unexpected decode error: {other:?}"),
    }
}

#[test]
fn material_reducer_rejects_truncated_output_vector() {
    let mut body = EMPTY_RECIPE_VECTOR_COUNTS.to_vec();
    body.push(1);
    body.extend_from_slice(&REDUCER_BODY[..REDUCER_BODY.len() - 1]);
    let error = raw_crafting_data_body(&body)
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect_err("truncated second output must fail");
    let is_truncated = match &error {
        JolyneError::PacketDecode {
            source: DecodeError::UnexpectedEof { .. },
            ..
        } => true,
        JolyneError::PacketDecode {
            source: DecodeError::Io(source),
            ..
        } => source.kind() == std::io::ErrorKind::UnexpectedEof,
        _ => false,
    };
    assert!(is_truncated, "{error:?}");
}

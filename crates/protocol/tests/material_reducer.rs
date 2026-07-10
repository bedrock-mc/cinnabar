use bytes::{Bytes, BytesMut};
use jolyne::{
    batch::{decode_batch_raw, encode_batch_multi},
    error::JolyneError,
    raw::{RawPacket, decode_packet_raw},
    valentine::{MaterialReducerView, McpePacketArgs, McpePacketData, McpePacketName},
};
use protocol::BedrockSession;
use valentine::bedrock::error::DecodeError;
use valentine::protocol::wire;

const GOPHERTUNNEL_MATERIAL_REDUCER: &[u8] = include_bytes!("../fixtures/material_reducer.bin");
const REDUCER_BODY: &[u8] = &[0x86, 0x80, 0xd0, 0x02, 0x02, 0x0e, 0x04, 0x11, 0x08];
const MAX_REDUCER_OUTPUTS: usize = 4_096;

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
    wire::write_var_u32(&mut payload, McpePacketName::PacketCraftingData as u32);
    payload.extend_from_slice(body);
    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    decode_packet_raw(&mut frame.freeze()).expect("raw CraftingData frame")
}

fn assert_two_outputs(data: &McpePacketData) {
    let McpePacketData::PacketCraftingData(packet) = data else {
        panic!("expected CraftingData, got {:?}", data.packet_id());
    };
    assert_eq!(packet.material_reducers.len(), 1);
    let reducer = format!("{:?}", packet.material_reducers[0]);
    assert!(reducer.contains("outputs"), "{reducer}");
    assert!(reducer.contains("network_id: 7, count: 2"), "{reducer}");
    assert!(reducer.contains("network_id: -9, count: 4"), "{reducer}");
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
        .into_owned(McpePacketArgs { shield_item_id: 0 })
        .expect("materialize borrowed CraftingData");
    assert_two_outputs(&owned);
}

#[test]
fn material_reducer_view_decodes_the_complete_output_vector() {
    let mut body = Bytes::from_static(REDUCER_BODY);
    let view = MaterialReducerView::decode(&mut body).expect("borrowed reducer decode");
    assert!(
        body.is_empty(),
        "borrowed reducer left {} bytes",
        body.len()
    );
    let owned = format!("{:?}", jolyne::valentine::MaterialReducer::from(view));
    assert!(owned.contains("network_id: 7, count: 2"), "{owned}");
    assert!(owned.contains("network_id: -9, count: 4"), "{owned}");
}

#[test]
fn material_reducer_rejects_count_above_gophertunnel_slice_limit() {
    let mut body = vec![0, 0, 0, 1];
    body.extend_from_slice(&REDUCER_BODY[..4]);
    body.extend_from_slice(&[0x81, 0x20]);
    let error = raw_crafting_data_body(&body)
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect_err("4,097 outputs must fail before allocation");
    assert!(matches!(
        error,
        JolyneError::PacketDecode {
            source: DecodeError::ArrayLengthExceeded {
                declared: 4_097,
                available: MAX_REDUCER_OUTPUTS,
            },
            ..
        }
    ));
}

#[test]
fn material_reducer_rejects_truncated_output_vector() {
    let mut body = vec![0, 0, 0, 1];
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

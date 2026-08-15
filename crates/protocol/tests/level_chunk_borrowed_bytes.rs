use bytes::{Bytes, BytesMut};
use valentine::bedrock::codec::{BedrockCodec, BedrockSized, VarUInt};
use valentine::bedrock::error::DecodeError;
use valentine::bedrock::version::v1_26_40::{
    LevelChunkPacket, LevelChunkPacketPayloadSubChunkMetadata, LevelChunkPacketView,
};

fn packet(payload_len: usize) -> LevelChunkPacket {
    LevelChunkPacket {
        cache_enabled: true,
        cache_metadata: vec![
            LevelChunkPacketPayloadSubChunkMetadata {
                blob_id: 0x1122_3344_5566_7788,
            },
            LevelChunkPacketPayloadSubChunkMetadata {
                blob_id: 0x8877_6655_4433_2211,
            },
        ],
        serialized_chunk_data: (0..payload_len).map(|index| index as u8).collect(),
        ..LevelChunkPacket::default()
    }
}

fn encode(packet: &LevelChunkPacket) -> Bytes {
    let mut wire = BytesMut::with_capacity(packet.encoded_size());
    packet.encode(&mut wire).expect("encode LevelChunk");
    assert_eq!(wire.len(), packet.encoded_size());
    wire.freeze()
}

#[test]
fn view_aliases_wire_round_trips_and_converts_to_distinct_owned_storage() {
    let packet = packet(4097);
    let wire = encode(&packet);
    let wire_start = wire.as_ptr() as usize;
    let wire_end = wire_start + wire.len();
    let mut input = wire.clone();
    let view = LevelChunkPacketView::decode(&mut input).expect("decode LevelChunk view");

    assert!(input.is_empty());
    assert_eq!(
        view.serialized_chunk_data.as_ref(),
        packet.serialized_chunk_data
    );
    let payload_pointer = view.serialized_chunk_data.as_ptr() as usize;
    assert!((wire_start..wire_end).contains(&payload_pointer));
    assert_eq!(
        view.cache_metadata
            .iter()
            .map(|item| item.blob_id)
            .collect::<Vec<_>>(),
        packet
            .cache_metadata
            .iter()
            .map(|item| item.blob_id)
            .collect::<Vec<_>>(),
    );

    let mut reencoded = BytesMut::new();
    view.encode(&mut reencoded).expect("encode LevelChunk view");
    assert_eq!(view.encoded_size(), wire.len());
    assert_eq!(reencoded.as_ref(), wire.as_ref());

    let owned: LevelChunkPacket = view.into();
    assert_eq!(owned, packet);
    assert_ne!(
        owned.serialized_chunk_data.as_ptr() as usize,
        payload_pointer
    );
}

#[test]
fn view_accepts_payload_near_the_transport_envelope_without_a_global_cap() {
    const NEAR_TRANSPORT_ENVELOPE: usize = 16 * 1024 * 1024 - 1024;
    let packet = packet(NEAR_TRANSPORT_ENVELOPE);
    let wire = encode(&packet);
    let wire_start = wire.as_ptr() as usize;
    let wire_end = wire_start + wire.len();
    let mut input = wire.clone();
    let view = LevelChunkPacketView::decode(&mut input).expect("decode near-envelope LevelChunk");

    assert_eq!(view.serialized_chunk_data.len(), NEAR_TRANSPORT_ENVELOPE);
    assert!((wire_start..wire_end).contains(&(view.serialized_chunk_data.as_ptr() as usize)));
    assert!(input.is_empty());
}

#[test]
fn view_rejects_adversarial_payload_and_metadata_lengths() {
    let mut overflowing_payload = encode(&packet(0)).to_vec();
    assert_eq!(overflowing_payload.pop(), Some(0));
    overflowing_payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x10]);
    let error = LevelChunkPacketView::decode(&mut Bytes::from(overflowing_payload))
        .expect_err("payload length beyond u32 must fail");
    assert!(
        matches!(error, DecodeError::VarIntTooLarge),
        "unexpected error: {error:?}"
    );

    let mut truncated_payload = encode(&packet(0)).to_vec();
    assert_eq!(truncated_payload.pop(), Some(0));
    VarUInt(1024)
        .encode(&mut truncated_payload)
        .expect("encode declared length");
    truncated_payload.extend_from_slice(&[1, 2, 3]);
    let error = LevelChunkPacketView::decode(&mut Bytes::from(truncated_payload))
        .expect_err("payload length beyond remaining bytes must fail");
    assert!(matches!(
        error,
        DecodeError::ArrayLengthExceeded {
            declared: 1024,
            available: 3
        }
    ));

    let packet = LevelChunkPacket {
        cache_enabled: true,
        ..LevelChunkPacket::default()
    };
    let mut oversized_metadata = encode(&packet).to_vec();
    assert_eq!(oversized_metadata.pop(), Some(0));
    assert_eq!(oversized_metadata.pop(), Some(0));
    VarUInt(i32::MAX as u32)
        .encode(&mut oversized_metadata)
        .expect("encode oversized metadata length");
    let error = LevelChunkPacketView::decode(&mut Bytes::from(oversized_metadata))
        .expect_err("metadata length beyond remaining bytes must fail");
    assert!(
        matches!(
            error,
            DecodeError::UnexpectedEof {
                needed: 8,
                available: 0
            }
        ),
        "unexpected error: {error:?}"
    );
}

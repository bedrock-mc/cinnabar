//! LevelChunk payloads decode as slices of the received buffer rather than copies.

use bedrock_protocol_1_26_51::packets::LevelChunk;
use bedrock_protocol_1_26_51::types::SubChunkMetadata;
use bedrock_protocol_1_26_51::wire::{self, Decode, Encode};
use bytes::Bytes;

fn packet(payload_len: usize) -> LevelChunk {
    LevelChunk {
        cache_enabled: true,
        cache_metadata: vec![
            SubChunkMetadata(0x1122_3344_5566_7788),
            SubChunkMetadata(0x8877_6655_4433_2211),
        ],
        serialized_chunk_data: (0..payload_len).map(|index| index as u8).collect(),
        ..LevelChunk::default()
    }
}

fn encode(packet: &LevelChunk) -> Bytes {
    Bytes::from(packet.encode_to_vec().expect("encode LevelChunk"))
}

fn decode_shared(wire: &Bytes) -> wire::DecodeResult<LevelChunk> {
    LevelChunk::decode_shared(wire, &wire::Limits::unbounded())
}

#[test]
fn shared_decode_aliases_the_wire_and_round_trips() {
    let packet = packet(4097);
    let wire = encode(&packet);
    let wire_start = wire.as_ptr() as usize;
    let wire_end = wire_start + wire.len();
    let decoded = decode_shared(&wire).expect("decode LevelChunk");

    assert_eq!(decoded, packet);
    let payload_pointer = decoded.serialized_chunk_data.as_ptr() as usize;
    assert!((wire_start..wire_end).contains(&payload_pointer));
    assert_eq!(decoded.encode_to_vec().expect("re-encode"), wire.as_ref());
}

#[test]
fn shared_decode_accepts_payload_near_the_transport_envelope_without_a_global_cap() {
    const NEAR_TRANSPORT_ENVELOPE: usize = 16 * 1024 * 1024 - 1024;
    let wire = encode(&packet(NEAR_TRANSPORT_ENVELOPE));
    let wire_start = wire.as_ptr() as usize;
    let wire_end = wire_start + wire.len();
    let decoded = decode_shared(&wire).expect("decode near-envelope LevelChunk");

    assert_eq!(decoded.serialized_chunk_data.len(), NEAR_TRANSPORT_ENVELOPE);
    assert!((wire_start..wire_end).contains(&(decoded.serialized_chunk_data.as_ptr() as usize)));
}

#[test]
fn shared_decode_rejects_adversarial_payload_and_metadata_lengths() {
    let mut overflowing_payload = encode(&packet(0)).to_vec();
    assert_eq!(overflowing_payload.pop(), Some(0));
    overflowing_payload.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x10]);
    let error = decode_shared(&Bytes::from(overflowing_payload))
        .expect_err("payload length beyond u32 must fail");
    assert!(
        matches!(error, wire::DecodeError::VarIntTooLarge),
        "unexpected error: {error:?}"
    );

    let mut truncated_payload = encode(&packet(0)).to_vec();
    assert_eq!(truncated_payload.pop(), Some(0));
    wire::Writer::new(&mut truncated_payload)
        .write_var_u32(1024)
        .expect("encode declared length");
    truncated_payload.extend_from_slice(&[1, 2, 3]);
    let error = decode_shared(&Bytes::from(truncated_payload))
        .expect_err("payload length beyond remaining bytes must fail");
    assert!(
        matches!(
            error,
            wire::DecodeError::LengthNotRepresentable {
                needed: 1024,
                remaining: 3,
                ..
            }
        ),
        "unexpected error: {error:?}"
    );

    // A hash count the input cannot hold is truncation, checked before the schema bound.
    let packet = LevelChunk {
        cache_enabled: true,
        ..LevelChunk::default()
    };
    let mut oversized_metadata = encode(&packet).to_vec();
    assert_eq!(oversized_metadata.pop(), Some(0));
    assert_eq!(oversized_metadata.pop(), Some(0));
    wire::Writer::new(&mut oversized_metadata)
        .write_var_u32(i32::MAX as u32)
        .expect("encode oversized metadata length");
    let error = decode_shared(&Bytes::from(oversized_metadata))
        .expect_err("metadata length beyond the input must fail");
    assert!(
        matches!(error, wire::DecodeError::LengthNotRepresentable { .. }),
        "unexpected error: {error:?}"
    );
}

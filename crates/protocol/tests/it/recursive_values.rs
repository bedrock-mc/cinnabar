//! Recursive value limits at the public batch decoder boundary.

use bedrock_protocol_1_26_51::{
    packets::ClientboundDataStore,
    types::{BedrockDDUI, DynamicValue},
    wire::{self, Encode},
};
use protocol::{ProtocolError, decode_batch};

/// Encodes a data-store change using the generated encoder, independently of play routing.
fn batch(depth: usize) -> bytes::Bytes {
    let mut value = DynamicValue::None;
    for _ in 0..depth {
        value = DynamicValue::List(vec![value]);
    }
    let data = ClientboundDataStore {
        updates: vec![BedrockDDUI::DataStoreChange {
            data_store_name: "test".into(),
            property: "value".into(),
            update_count: wire::U32LE(1),
            the_new_property_value: value,
        }],
    };
    let mut out = vec![wire::BATCH_HEADER];
    wire::encode_frame_with(
        &mut out,
        wire::PacketHeader {
            id: ClientboundDataStore::ID,
            sender_subclient: 0,
            target_subclient: 0,
        },
        usize::MAX,
        |writer| data.encode(writer),
    )
    .unwrap();
    out.into()
}

#[test]
fn public_batch_decoder_rejects_overdeep_recursive_values() {
    assert_eq!(
        decode_batch(batch(wire::MAX_RECURSIVE_VALUE_DEPTH))
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        decode_batch(batch(wire::MAX_RECURSIVE_VALUE_DEPTH + 1)),
        Err(ProtocolError::Decode(_))
    ));
}

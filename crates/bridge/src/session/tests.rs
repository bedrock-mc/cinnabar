mod cache;

use bytes::{Bytes, BytesMut};
use tokio_util::codec::Decoder;

use super::*;
use crate::framed::BridgeCodec;

const CONNECT_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/session/connect.bin");
const CORE_STREAM_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/session/core_stream.bin");

/// The Go core decodes exactly these Connect bytes.
#[test]
fn connect_encoding_matches_the_core_fixture() {
    let request = ConnectRequest {
        protocol: 2193,
        target: Some(ConnectTarget::RakNet("play.example.net:19132".into())),
        client_cache: true,
        client_data: serde_json::json!({"DeviceOS": 7, "GameVersion": "1.26.50"}),
    };
    assert_eq!(&encode_connect(&request).unwrap()[..], CONNECT_FIXTURE);

    let targetless = ConnectRequest {
        target: None,
        ..request
    };
    let frame = encode_connect(&targetless).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&frame[1..]).unwrap();
    assert!(
        body.get("target").is_none(),
        "a targetless Connect omits the target"
    );
}

/// The frames the Go core writes for a handoff, its pack, a batch and a transfer decode in order.
#[test]
fn core_stream_fixture_decodes_in_order() {
    let mut wire = BytesMut::from(CORE_STREAM_FIXTURE);
    let mut codec = BridgeCodec::new();
    let mut messages = Vec::new();
    while let Some(frame) = codec.decode(&mut wire).unwrap() {
        messages.push(decode_core_message(frame).unwrap());
    }
    assert!(wire.is_empty());
    let [handoff, pack, batch, transfer] = <[CoreMessage; 4]>::try_from(messages).unwrap();

    let CoreMessage::Handoff(handoff) = handoff else {
        panic!("first message is not the handoff")
    };
    assert_eq!(
        handoff.identity,
        SessionIdentity {
            display_name: "Steve".into(),
            xuid: "2535400000000000".into(),
            uuid: "00000000-0000-4000-8000-000000000001".into(),
        }
    );
    assert!(handoff.client_cache && handoff.packs_required);
    let [selected] = &handoff.packs[..] else {
        panic!("one pack expected")
    };
    assert_eq!(
        (
            selected.uuid.as_str(),
            selected.version.as_str(),
            selected.sub_pack.as_str()
        ),
        ("00112233-4455-6677-8899-aabbccddeeff", "1.0.0", "high")
    );
    assert_eq!((selected.content_key.expose(), selected.size), ("key", 3));
    assert_eq!(
        handoff.startup,
        [
            Bytes::from_static(&[0xb4, 0x01, 0x09]),
            Bytes::from_static(&[0x0b, 0x01, 0x02])
        ]
    );

    let mut receiver = HandoffPackReceiver::new(&handoff, None).unwrap();
    assert!(!receiver.is_complete());
    let CoreMessage::PackData { index, data } = pack else {
        panic!("pack data expected")
    };
    receiver.accept(index, &data).unwrap();
    assert_eq!(receiver.into_archives().unwrap(), [b"abc".to_vec()]);

    let CoreMessage::Batch(packets) = batch else {
        panic!("batch expected")
    };
    assert_eq!(packets, [Bytes::from_static(&[0x09, 0x00])]);
    let CoreMessage::Transfer(transfer) = transfer else {
        panic!("transfer expected")
    };
    assert_eq!(
        transfer,
        SessionTransfer {
            address: "play.example.net".into(),
            port: 19132,
            reload_world: false
        }
    );
}

#[test]
fn batches_round_trip_with_multi_byte_lengths() {
    let large = vec![7u8; 300];
    let frame = encode_batch([&[1u8, 2][..], &large[..]]).unwrap();
    assert_eq!(&frame[..4], [KIND_BATCH, 2, 1, 2]);
    assert_eq!(&frame[4..6], [0xac, 0x02], "300 is a two-byte varuint32");
    let CoreMessage::Batch(packets) = decode_core_message(frame).unwrap() else {
        panic!("batch expected")
    };
    assert_eq!(
        packets,
        [Bytes::from_static(&[1, 2]), Bytes::from(large.clone())]
    );

    assert!(encode_batch(Vec::<&[u8]>::new()).is_err());
    assert!(encode_batch([&[][..]]).is_err());
}

#[test]
fn malformed_core_frames_are_rejected() {
    for frame in [
        &[][..],
        &[0x09],
        &[KIND_BATCH],
        &[KIND_BATCH, 0],
        &[KIND_BATCH, 3, 1, 2],
        &[KIND_BATCH, 0x80],
        &[KIND_BATCH, 0xff, 0xff, 0xff, 0xff, 0x7f],
        &[KIND_PACK_DATA, 0, 0, 0, 0],
        &[KIND_HANDOFF, 0, 0],
        &[KIND_HANDOFF, 0, 0, 0, 9, b'{'],
        &[KIND_TRANSFER, b'{'],
        &[KIND_CONNECT, b'{', b'}'],
    ] {
        let error = decode_core_message(Bytes::copy_from_slice(frame))
            .expect_err("malformed frame must fail");
        assert!(
            matches!(
                error,
                BridgeError::InvalidSessionMessage { .. } | BridgeError::SessionJson(_)
            ),
            "{frame:?}: {error}"
        );
    }
}

#[test]
fn handoff_requires_startup_packets_and_known_fields() {
    let metadata = br#"{"identity":{"display_name":"a","xuid":"","uuid":""},"client_cache":false,"packs_required":false,"packs":[]}"#;
    let mut frame = vec![KIND_HANDOFF];
    frame.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    frame.extend_from_slice(metadata);
    assert!(decode_core_message(Bytes::from(frame.clone())).is_err());

    let mut wrong_last = frame.clone();
    wrong_last.extend_from_slice(&[1, 0x0b, 1, 0x09]);
    assert!(
        matches!(
            decode_core_message(Bytes::from(wrong_last)),
            Err(BridgeError::InvalidSessionMessage { .. })
        ),
        "startup must end with StartGame"
    );
    let mut sub_client = frame.clone();
    sub_client.extend_from_slice(&[2, 0x0b | 0x80, 0x10]);
    assert!(
        decode_core_message(Bytes::from(sub_client)).is_ok(),
        "sub-client bits do not change the packet ID"
    );

    frame.extend_from_slice(&[1, 0x0b]);
    let CoreMessage::Handoff(handoff) = decode_core_message(Bytes::from(frame)).unwrap() else {
        panic!("handoff expected")
    };
    assert_eq!(handoff.startup, [Bytes::from_static(&[0x0b])]);
    assert!(
        HandoffPackReceiver::new(&handoff, None)
            .unwrap()
            .is_complete()
    );

    let unknown = br#"{"identity":{"display_name":"a","xuid":"","uuid":""},"client_cache":false,"packs_required":false,"packs":[],"extra":1}"#;
    let mut frame = vec![KIND_HANDOFF];
    frame.extend_from_slice(&(unknown.len() as u32).to_be_bytes());
    frame.extend_from_slice(unknown);
    frame.extend_from_slice(&[1, 0x0b]);
    assert!(matches!(
        decode_core_message(Bytes::from(frame)),
        Err(BridgeError::SessionJson(_))
    ));
}

fn handoff_with_sizes(sizes: &[u64]) -> SessionHandoff {
    SessionHandoff {
        identity: SessionIdentity {
            display_name: String::new(),
            xuid: String::new(),
            uuid: String::new(),
        },
        client_cache: false,
        packs_required: false,
        packs: sizes
            .iter()
            .map(|&size| HandoffPack {
                uuid: String::new(),
                version: String::new(),
                sub_pack: String::new(),
                content_key: PackContentKey("secret-key".into()),
                size,
                cache: None,
            })
            .collect(),
        startup: Vec::new(),
    }
}

/// Chunks continue the first incomplete archive; empty archives need no frames.
#[test]
fn pack_receiver_assembles_chunks_in_handoff_order() {
    let handoff = handoff_with_sizes(&[0, 5, 0, 2]);
    let mut receiver = HandoffPackReceiver::new(&handoff, None).unwrap();
    assert!(
        receiver.accept(0, b"x").is_err(),
        "an empty archive takes no bytes"
    );
    receiver.accept(1, b"abc").unwrap();
    assert!(
        receiver.accept(3, b"zz").is_err(),
        "archive 1 is still open"
    );
    receiver.accept(1, b"de").unwrap();
    assert!(!receiver.is_complete());
    receiver.accept(3, b"zz").unwrap();
    assert!(receiver.is_complete());
    assert!(
        receiver.accept(3, b"!").is_err(),
        "a complete archive takes no more"
    );
    assert_eq!(
        receiver.into_archives().unwrap(),
        [Vec::new(), b"abcde".to_vec(), Vec::new(), b"zz".to_vec()]
    );
}

#[test]
fn pack_receiver_rejects_overflow_and_incomplete_archives() {
    let handoff = handoff_with_sizes(&[3]);
    let mut receiver = HandoffPackReceiver::new(&handoff, None).unwrap();
    assert!(receiver.accept(0, b"abcd").is_err());
    assert!(receiver.accept(0, b"").is_err());
    receiver.accept(0, b"ab").unwrap();
    assert!(
        HandoffPackReceiver::new(&handoff, None)
            .unwrap()
            .into_archives()
            .is_err()
    );
    assert!(receiver.into_archives().is_err());
}

#[test]
fn content_keys_never_reach_debug_output() {
    let handoff = handoff_with_sizes(&[1]);
    let debug = format!("{handoff:?}");
    assert!(!debug.contains("secret-key"), "{debug}");
    assert!(debug.contains("redacted"));
}

/// An outgoing Bedrock batch becomes a Batch frame with the same packets, and a Batch frame's body
/// is the Bedrock batch body without copying.
#[test]
fn bedrock_batches_map_to_batch_frames_both_ways() {
    let frame = batch_frame_from_bedrock(&[0xfe, 2, 0x09, 0x00]).unwrap();
    assert_eq!(&frame[..], [KIND_BATCH, 2, 0x09, 0x00]);
    let CoreMessage::Batch(packets) = decode_core_message(frame.clone()).unwrap() else {
        panic!("batch expected")
    };
    assert_eq!(packets, [Bytes::from_static(&[0x09, 0x00])]);
    let body = batch_frame_body(&frame).unwrap();
    assert_eq!(&body[..], [2, 0x09, 0x00]);
    assert_eq!(
        body.as_ptr(),
        frame[1..].as_ptr(),
        "the body shares the frame"
    );

    for invalid in [&[][..], &[0xfe], &[0x00, 1, 1]] {
        assert!(batch_frame_from_bedrock(invalid).is_err(), "{invalid:?}");
    }
    assert!(batch_frame_body(&Bytes::from_static(&[KIND_TRANSFER, b'{'])).is_none());
}

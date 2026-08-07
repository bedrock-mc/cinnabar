use bytes::{BufMut, Bytes, BytesMut};
use protocol::{BedrockSession, ProtocolError, decode_batch, encode};
use valentine::bedrock::version::v1_26_40::{
    BorrowedMcpePacket, BorrowedMcpePacketData, McpePacketArgs, McpePacketData, McpePacketName,
};
use valentine::protocol::wire;

const RESERVED: &[(u32, McpePacketName)] = &[
    (65, McpePacketName::Opaque65Packet),
    (96, McpePacketName::Opaque96Packet),
    (98, McpePacketName::Unavailable98Packet),
    (99, McpePacketName::Unavailable99Packet),
    (109, McpePacketName::Unavailable109Packet),
    (137, McpePacketName::Unavailable137Packet),
    (150, McpePacketName::Unavailable150Packet),
    (169, McpePacketName::Unavailable169Packet),
    (170, McpePacketName::Unavailable170Packet),
    (171, McpePacketName::Unavailable171Packet),
    (173, McpePacketName::Unavailable173Packet),
    (178, McpePacketName::Unavailable178Packet),
    (181, McpePacketName::Unavailable181Packet),
    (183, McpePacketName::Unavailable183Packet),
    (304, McpePacketName::Unavailable304Packet),
];

fn session() -> BedrockSession {
    BedrockSession { shield_item_id: 0 }
}

fn inner_frame(id: u32, from_subclient: u32, to_subclient: u32, body: &[u8]) -> Bytes {
    let header = id | (from_subclient << 10) | (to_subclient << 12);
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, header);
    payload.extend_from_slice(body);

    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    frame.freeze()
}

fn batch(frames: &[Bytes]) -> Bytes {
    let mut batch = BytesMut::new();
    batch.put_u8(0xfe);
    for frame in frames {
        batch.extend_from_slice(frame);
    }
    batch.freeze()
}

#[test]
fn reserved_packet_ids_retain_their_numeric_discriminants() {
    for &(id, name) in RESERVED {
        assert_eq!(name as u32, id);
    }
}

#[test]
fn reserved_packets_round_trip_arbitrary_owned_bodies() {
    let bodies: &[&[u8]] = &[&[], &[0x00], &[0xff, 0x80, 0x01, 0x00, 0x7f]];
    for &(id, name) in RESERVED {
        for body in bodies {
            let fixture = batch(&[inner_frame(id, 2, 3, body)]);
            let packets =
                decode_batch(fixture.clone(), &session()).expect("decode reserved packet");
            let packet = packets.first().expect("one packet");
            assert_eq!(packet.header.id, name);
            assert_eq!(packet.header.from_subclient, 2);
            assert_eq!(packet.header.to_subclient, 3);
            let McpePacketData::OpaquePacket(unavailable) = &packet.data else {
                panic!("reserved packet did not decode opaquely");
            };
            assert_eq!(unavailable.id, name);
            assert_eq!(unavailable.payload.as_ref(), *body);
            assert_eq!(
                encode(packet, &session()).expect("encode reserved packet"),
                fixture
            );
        }
    }
}

#[test]
fn borrowed_reserved_packets_stay_raw_and_convert_to_owned() {
    for &(id, name) in RESERVED {
        let body = [id as u8, 0x00, 0xff, 0x80];
        let mut frame = inner_frame(id, 1, 2, &body);
        let borrowed = BorrowedMcpePacket::decode_inner(&mut frame).expect("borrowed decode");
        assert_eq!(borrowed.header.from_subclient, 1);
        assert_eq!(borrowed.header.to_subclient, 2);
        let BorrowedMcpePacketData::Raw {
            name: raw_name,
            payload,
        } = &borrowed.data
        else {
            panic!("reserved borrowed packet must remain raw");
        };
        assert_eq!(*raw_name, name);
        assert_eq!(payload.as_ref(), body);

        let owned = borrowed
            .into_owned(McpePacketArgs)
            .expect("convert to owned");
        let McpePacketData::OpaquePacket(unavailable) = owned.data else {
            panic!("reserved borrowed packet did not become opaque owned data");
        };
        assert_eq!(unavailable.id, name);
        assert_eq!(unavailable.payload.as_ref(), body);
    }
}

#[test]
fn reserved_packet_cannot_consume_an_adjacent_frame() {
    let first = inner_frame(173, 0, 0, &[0x80, 0x80, 0x80]);
    let second = inner_frame(109, 3, 1, &[0x01, 0x02]);
    let fixture = batch(&[first, second]);
    let packets = decode_batch(fixture.clone(), &session()).expect("decode adjacent frames");
    assert_eq!(packets.len(), 2);
    assert_eq!(packets[0].header.id, McpePacketName::Unavailable173Packet);
    assert_eq!(packets[1].header.id, McpePacketName::Unavailable109Packet);

    let mut encoded = BytesMut::new();
    encoded.put_u8(0xfe);
    for packet in &packets {
        encoded.extend_from_slice(&encode(packet, &session()).expect("encode packet")[1..]);
    }
    assert_eq!(encoded.freeze(), fixture);
}

#[test]
fn reserved_packet_declared_length_truncation_is_fatal() {
    let mut fixture = BytesMut::new();
    fixture.put_u8(0xfe);
    wire::write_var_u32(&mut fixture, 8);
    wire::write_var_u32(&mut fixture, 173);
    fixture.extend_from_slice(&[0x01, 0x02]);

    assert!(matches!(
        decode_batch(fixture.freeze(), &session()),
        Err(ProtocolError::TruncatedPacket { .. })
    ));
}

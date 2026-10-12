use bedrock_protocol_1_26_51::packets::PacketId;
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;
use protocol::{ProtocolError, decode_batch};

/// Packet IDs the client once kept opaque; each now decodes with its generated packet.
const FORMER_RESERVED: &[(u32, PacketId)] = &[
    (65, PacketId::LegacyTelemetryEvent),
    (96, PacketId::SetLastHurtBy),
    (98, PacketId::NpcRequest),
    (99, PacketId::PhotoTransfer),
    (109, PacketId::LabTable),
    (137, PacketId::EducationSettings),
    (150, PacketId::CodeBuilder),
    (169, PacketId::NpcDialogue),
    (170, PacketId::EduUriResource),
    (171, PacketId::CreatePhoto),
    (178, PacketId::CodeBuilderSource),
    (181, PacketId::AgentActionEvent),
    (183, PacketId::LessonProgress),
    (304, PacketId::AgentAnimation),
];

const UNASSIGNED: &[u32] = &[173, 301];

fn var_u32(out: &mut Vec<u8>, value: u32) {
    wire::Writer::new(out).write_var_u32(value).unwrap();
}

fn inner_frame(id: u32, from_subclient: u32, to_subclient: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    var_u32(
        &mut payload,
        id | (from_subclient << 10) | (to_subclient << 12),
    );
    payload.extend_from_slice(body);

    let mut frame = Vec::new();
    var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    frame
}

fn batch(frames: &[Vec<u8>]) -> Bytes {
    let mut batch = vec![0xfe];
    for frame in frames {
        batch.extend_from_slice(frame);
    }
    batch.into()
}

#[test]
fn former_reserved_packet_ids_use_generated_packets() {
    for &(id, packet) in FORMER_RESERVED {
        assert_eq!(packet as u32, id);
        assert_eq!(PacketId::from_raw(id), Some(packet));
    }
}

#[test]
fn generated_packets_reject_arbitrary_former_reserved_bodies() {
    for &(id, packet) in FORMER_RESERVED {
        let fixture = batch(&[inner_frame(id, 2, 3, &[0xff, 0x80, 0x01, 0x00, 0x7f])]);
        let error = decode_batch(fixture)
            .expect_err("generated packet must not decode arbitrary legacy opaque body");
        assert!(
            matches!(
                error,
                ProtocolError::Decode(_)
                    | ProtocolError::TrailingPacketBytes { .. }
                    | ProtocolError::Ui(_)
            ),
            "{packet:?} produced unexpected error {error:?}"
        );
    }
}

#[test]
fn unassigned_packet_ids_are_rejected() {
    for &id in UNASSIGNED {
        assert_eq!(PacketId::from_raw(id), None);
        assert!(matches!(
            decode_batch(batch(&[inner_frame(id, 1, 2, &[0x00])])),
            Err(ProtocolError::Decode(_))
        ));
    }
}

#[test]
fn unassigned_packet_cannot_consume_an_adjacent_frame() {
    let first = inner_frame(173, 0, 0, &[0x80, 0x80, 0x80]);
    let second = inner_frame(109, 3, 1, &[0x01, 0x02]);
    let fixture = batch(&[first, second]);
    assert!(matches!(
        decode_batch(fixture),
        Err(ProtocolError::Decode(_))
    ));
}

#[test]
fn unassigned_packet_declared_length_truncation_is_fatal() {
    let mut fixture = vec![0xfe];
    var_u32(&mut fixture, 8);
    var_u32(&mut fixture, 173);
    fixture.extend_from_slice(&[0x01, 0x02]);

    assert!(matches!(
        decode_batch(fixture.into()),
        Err(ProtocolError::TruncatedPacket { .. })
    ));
}

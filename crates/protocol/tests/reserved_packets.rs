use bytes::{BufMut, Bytes, BytesMut};
use protocol::{BedrockSession, ProtocolError, decode_batch};
use valentine::bedrock::version::v1_26_40::{BorrowedMcpePacket, McpePacketName};
use valentine::protocol::wire;

const FORMER_RESERVED: &[(u32, McpePacketName)] = &[
    (65, McpePacketName::LegacyTelemetryEventPacket),
    (96, McpePacketName::SetLastHurtByPacket),
    (98, McpePacketName::NpcRequestPacket),
    (99, McpePacketName::PhotoTransferPacket),
    (109, McpePacketName::LabTablePacket),
    (137, McpePacketName::EducationSettingsPacket),
    (150, McpePacketName::CodeBuilderPacket),
    (169, McpePacketName::NpcDialoguePacket),
    (170, McpePacketName::EduUriResourcePacket),
    (171, McpePacketName::CreatePhotoPacket),
    (178, McpePacketName::CodeBuilderSourcePacket),
    (181, McpePacketName::AgentActionEventPacket),
    (183, McpePacketName::LessonProgressPacket),
    (304, McpePacketName::AgentAnimationPacket),
];

const UNASSIGNED: &[u32] = &[173, 301];

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
fn former_reserved_packet_ids_now_use_generated_numeric_discriminants() {
    for &(id, name) in FORMER_RESERVED {
        assert_eq!(name as u32, id);
    }
}

#[test]
fn generated_packet_names_reject_arbitrary_reserved_bodies_without_opacity() {
    for &(id, name) in FORMER_RESERVED {
        let fixture = batch(&[inner_frame(id, 2, 3, &[0xff, 0x80, 0x01, 0x00, 0x7f])]);
        let error = decode_batch(fixture, &session()).expect_err("generated packet must not decode arbitrary legacy opaque body");
        assert!(
            matches!(error, ProtocolError::Decode(_) | ProtocolError::TrailingPacketBytes { .. }),
            "{name:?} produced unexpected error {error:?}"
        );
    }
}

#[test]
fn borrowed_unassigned_packet_ids_fail_before_owned_materialization() {
    for &id in UNASSIGNED {
        let mut frame = inner_frame(id, 1, 2, &[0x00]);
        assert!(BorrowedMcpePacket::decode_inner(&mut frame).is_err());
    }
}

#[test]
fn unassigned_packet_cannot_consume_an_adjacent_frame() {
    let first = inner_frame(173, 0, 0, &[0x80, 0x80, 0x80]);
    let second = inner_frame(109, 3, 1, &[0x01, 0x02]);
    let fixture = batch(&[first, second]);
    assert!(matches!(
        decode_batch(fixture, &session()),
        Err(ProtocolError::Decode(_))
    ));
}

#[test]
fn unassigned_packet_declared_length_truncation_is_fatal() {
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

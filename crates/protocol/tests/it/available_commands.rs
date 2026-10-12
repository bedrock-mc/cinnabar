use bedrock_protocol_1_26_51::packets::{self, PacketId};
use bedrock_protocol_1_26_51::wire::{self, DecodeError};
use bytes::Bytes;

use crate::SessionError;
use crate::codec::SessionFailure;
use crate::raw_packet::{RawPacket, decode_packet_raw};

const GOPHERTUNNEL_AVAILABLE_COMMANDS: &[u8] =
    include_bytes!("../../fixtures/available_commands.bin");
const LIVE_BODY_LENGTH_REGRESSION: &[u8] =
    include_bytes!("../../fixtures/available_commands_live_356513.bin");
const MAX_COMMAND_VALUES: usize = 4_096;

fn raw_fixture(fixture: &'static [u8]) -> RawPacket {
    crate::raw_packet::decode_batch(Bytes::from_static(fixture), 1024 * 1024)
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
}

fn raw_available_commands_body(body: &[u8]) -> RawPacket {
    let mut payload = Vec::new();
    wire::Writer::new(&mut payload)
        .write_var_u32(PacketId::AvailableCommands as u32)
        .unwrap();
    payload.extend_from_slice(body);
    let mut frame = Vec::new();
    wire::Writer::new(&mut frame)
        .write_var_u32(payload.len() as u32)
        .unwrap();
    frame.extend_from_slice(&payload);
    decode_packet_raw(&mut Bytes::from(frame)).expect("raw AvailableCommands frame")
}

fn assert_every_section(data: &packets::Packet) {
    let packets::Packet::AvailableCommands(packet) = data else {
        panic!("expected AvailableCommands, got {:?}", data.id());
    };
    // 1.26.40 renames the collections but keeps gophertunnel's field order from
    // `packet/available_commands.go`: enum values, chained subcommand values,
    // suffixes, enums, chained subcommands, commands, dynamic enums,
    // constraints. `suffixes` is `post_fixes`, `enums` is `enum_data`,
    // `chained_subcommands` is `chained_subcommand_data`, `command_data` is
    // `commands`, `dynamic_enums` is `soft_enums` and `enum_constraints` is
    // `constraints`.
    assert_eq!(packet.enum_values[..2], ["alpha", "beta"]);
    assert_eq!(packet.chained_subcommand_values, ["chain"]);
    assert_eq!(packet.post_fixes, ["suffix"]);
    assert_eq!(packet.enum_data.len(), 1);
    assert_eq!(packet.chained_subcommand_data.len(), 1);
    assert_eq!(packet.commands.len(), 1);
    assert_eq!(packet.soft_enums.len(), 1);
    assert_eq!(packet.constraints.len(), 1);
}

#[test]
fn pinned_gophertunnel_available_commands_owned_decodes_every_section_and_round_trips() {
    let packet = raw_fixture(GOPHERTUNNEL_AVAILABLE_COMMANDS)
        .decode()
        .expect("owned AvailableCommands decode");
    assert_every_section(&packet.data);
    let encoded = crate::encode(&packet).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_AVAILABLE_COMMANDS);
}

#[test]
fn observed_356513_byte_live_body_decodes_and_round_trips_exactly() {
    let raw = raw_fixture(LIVE_BODY_LENGTH_REGRESSION);
    assert_eq!(raw.body().len(), 356_513);
    let packet = raw
        .decode()
        .expect("large live-length AvailableCommands decode");
    assert_every_section(&packet.data);
    let encoded = crate::encode(&packet).expect("re-encode");
    assert_eq!(encoded.as_ref(), LIVE_BODY_LENGTH_REGRESSION);
}

#[test]
fn available_commands_rejects_malformed_shared_count() {
    let error = raw_available_commands_body(&[0x02, 0x01, b'a'])
        .decode()
        .expect_err("count declaring a missing second string must fail");
    assert!(matches!(
        error,
        SessionError(SessionFailure::PacketDecode {
            source: DecodeError::UnexpectedEof { .. },
            ..
        })
    ));
}

/// An impossible count remains fatal as truncated wire: the declared count is checked against
/// the remaining input before any value is allocated, without a global element ceiling.
#[test]
fn available_commands_rejects_count_above_gophertunnel_slice_limit() {
    let error = raw_available_commands_body(&[0x81, 0x20])
        .decode()
        .expect_err("4,097 values must not decode");
    let SessionError(SessionFailure::PacketDecode { source, .. }) = &error else {
        panic!("unexpected error: {error:?}");
    };
    assert!(matches!(
        source,
        DecodeError::LengthNotRepresentable {
            needed: 4_097,
            remaining: 0,
            ..
        }
    ));
}

/// Pins that the generated encoder applies no slice ceiling: it writes a collection longer than
/// gophertunnel's `maxSliceLength` (4096) would read back. Restore a rejection assertion if
/// the codec contract adds one.
#[test]
fn available_commands_encoding_no_longer_rejects_more_than_gophertunnel_slice_limit() {
    let mut packet = raw_fixture(GOPHERTUNNEL_AVAILABLE_COMMANDS)
        .decode()
        .expect("decode fixture before mutation");
    let packets::Packet::AvailableCommands(content) = &mut *packet.data else {
        panic!("expected AvailableCommands");
    };
    content
        .enum_values
        .resize(MAX_COMMAND_VALUES + 1, "overflow".into());

    crate::encode(&packet).expect("the generated encoder applies no slice ceiling");
}

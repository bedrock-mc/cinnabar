use bytes::{Bytes, BytesMut};
use jolyne::{
    batch::{decode_batch_raw, encode_batch_multi},
    error::JolyneError,
    raw::{RawPacket, decode_packet_raw},
    valentine::{McpePacketArgs, McpePacketData, McpePacketName},
};
use protocol::BedrockSession;
use valentine::bedrock::error::DecodeError;
use valentine::protocol::wire;

const GOPHERTUNNEL_AVAILABLE_COMMANDS: &[u8] = include_bytes!("../fixtures/available_commands.bin");
const LIVE_BODY_LENGTH_REGRESSION: &[u8] =
    include_bytes!("../fixtures/available_commands_live_356513.bin");
const MAX_COMMAND_VALUES: usize = 4_096;

fn raw_fixture(fixture: &'static [u8]) -> RawPacket {
    let mut batch = Bytes::from_static(fixture);
    decode_batch_raw(&mut batch, false, Some(1024 * 1024))
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
}

fn raw_available_commands_body(body: &[u8]) -> RawPacket {
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::PacketAvailableCommands as u32);
    payload.extend_from_slice(body);
    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    decode_packet_raw(&mut frame.freeze()).expect("raw AvailableCommands frame")
}

fn assert_every_section(data: &McpePacketData) {
    let McpePacketData::PacketAvailableCommands(packet) = data else {
        panic!("expected AvailableCommands, got {:?}", data.packet_id());
    };
    assert_eq!(packet.enum_values[..2], ["alpha", "beta"]);
    assert_eq!(packet.chained_subcommand_values, ["chain"]);
    assert_eq!(packet.suffixes, ["suffix"]);
    assert_eq!(packet.enums.len(), 1);
    assert_eq!(packet.chained_subcommands.len(), 1);
    assert_eq!(packet.command_data.len(), 1);
    assert_eq!(packet.dynamic_enums.len(), 1);
    assert_eq!(packet.enum_constraints.len(), 1);
}

#[test]
fn pinned_gophertunnel_available_commands_owned_decodes_every_section_and_round_trips() {
    let packet = raw_fixture(GOPHERTUNNEL_AVAILABLE_COMMANDS)
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("owned AvailableCommands decode");
    assert_every_section(&packet.data);
    let encoded = encode_batch_multi(&[packet], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_AVAILABLE_COMMANDS);
}

#[test]
fn pinned_gophertunnel_available_commands_borrowed_materializes() {
    let borrowed = raw_fixture(GOPHERTUNNEL_AVAILABLE_COMMANDS)
        .decode_borrowed()
        .expect("borrowed AvailableCommands decode");
    let owned = borrowed
        .data
        .into_owned(McpePacketArgs { shield_item_id: 0 })
        .expect("materialize borrowed AvailableCommands");
    assert_every_section(&owned);
}

#[test]
fn observed_356513_byte_live_body_decodes_and_round_trips_exactly() {
    let raw = raw_fixture(LIVE_BODY_LENGTH_REGRESSION);
    assert_eq!(raw.body().len(), 356_513);
    let packet = raw
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("large live-length AvailableCommands decode");
    assert_every_section(&packet.data);
    let encoded = encode_batch_multi(&[packet], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), LIVE_BODY_LENGTH_REGRESSION);
}

#[test]
fn available_commands_rejects_malformed_shared_count() {
    let error = raw_available_commands_body(&[0x02, 0x01, b'a'])
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect_err("count declaring a missing second string must fail");
    assert!(matches!(
        error,
        JolyneError::PacketDecode {
            source: DecodeError::UnexpectedEof { .. },
            ..
        }
    ));
}

#[test]
fn available_commands_rejects_count_above_gophertunnel_slice_limit() {
    let error = raw_available_commands_body(&[0x81, 0x20])
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect_err("4,097 values must fail before allocation");
    assert!(matches!(
        error,
        JolyneError::PacketDecode {
            source: DecodeError::ArrayLengthExceeded {
                declared: 4_097,
                available: MAX_COMMAND_VALUES,
            },
            ..
        }
    ));
}

#[test]
fn available_commands_encoding_rejects_more_than_gophertunnel_slice_limit() {
    let mut packet = raw_fixture(GOPHERTUNNEL_AVAILABLE_COMMANDS)
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("decode fixture before mutation");
    let McpePacketData::PacketAvailableCommands(content) = &mut packet.data else {
        panic!("expected AvailableCommands");
    };
    content
        .enum_values
        .resize(MAX_COMMAND_VALUES + 1, "overflow".to_owned());

    let error = encode_batch_multi(&[packet], false, 0, 0, true)
        .expect_err("oversized enum value collection must not be encoded");
    assert!(matches!(
        error,
        JolyneError::Io(ref error) if error.kind() == std::io::ErrorKind::InvalidInput
    ));
}

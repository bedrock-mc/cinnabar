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
    wire::write_var_u32(&mut payload, McpePacketName::AvailableCommandsPacket as u32);
    payload.extend_from_slice(body);
    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    decode_packet_raw(&mut frame.freeze()).expect("raw AvailableCommands frame")
}

fn assert_every_section(data: &McpePacketData) {
    let McpePacketData::AvailableCommandsPacket(packet) = data else {
        panic!("expected AvailableCommands, got {:?}", data.packet_id());
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
        .into_owned(McpePacketArgs)
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

/// A count above gophertunnel's slice ceiling must still fail the read.
///
/// REGRESSION - see the module header of `world_collection_bounds.rs`. Under
/// protocol 1001 this failed with `DecodeError::ArrayLengthExceeded { declared:
/// 4097, available: 4096 }` *before* allocating, matching gophertunnel's
/// `maxSliceLength = 4096` guard in `minecraft/protocol/io.go`
/// (`limit.SliceLength(l, maxSliceLength)` at commit
/// be6713da4dc051a4197f897d04835e89e9c54321). The 1.26.40 generated crate emits
/// no collection ceilings at all, so the count is reserved with
/// `Vec::with_capacity` first and the read only fails once the strings turn out
/// to be absent. Restoring the ceiling in valentine_gen trips the
/// `ArrayLengthExceeded` arm below, which is the signal to restore the stricter
/// declared/available assertion.
#[test]
fn available_commands_rejects_count_above_gophertunnel_slice_limit() {
    let error = raw_available_commands_body(&[0x81, 0x20])
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect_err("4,097 values must not decode");
    let JolyneError::PacketDecode { source, .. } = &error else {
        panic!("unexpected error: {error:?}");
    };
    match source {
        DecodeError::UnexpectedEof { .. } => {}
        DecodeError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof => {}
        DecodeError::ArrayLengthExceeded { .. } => panic!(
            "valentine_gen appears to emit collection ceilings again: restore the \
             `declared: 4_097, available: {MAX_COMMAND_VALUES}` assertion here"
        ),
        other => panic!("unexpected decode error: {other:?}"),
    }
}

/// Tripwire for the missing *encode*-side slice ceiling.
///
/// REGRESSION - protocol 1001 refused to encode a collection longer than
/// gophertunnel would agree to read back (`maxSliceLength = 4096`,
/// `minecraft/protocol/io.go` at be6713da4dc051a4197f897d04835e89e9c54321),
/// failing with `ErrorKind::InvalidInput` before writing a byte. The 1.26.40
/// generated encoder writes the length varint unconditionally, so we can now
/// emit a frame no vanilla client will accept.
///
/// This test pins that fact deliberately rather than dropping the coverage: the
/// moment valentine_gen restores the ceiling the encode below starts failing and
/// this test must go back to asserting the rejection.
#[test]
fn available_commands_encoding_no_longer_rejects_more_than_gophertunnel_slice_limit() {
    let mut packet = raw_fixture(GOPHERTUNNEL_AVAILABLE_COMMANDS)
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("decode fixture before mutation");
    let McpePacketData::AvailableCommandsPacket(content) = &mut packet.data else {
        panic!("expected AvailableCommands");
    };
    content
        .enum_values
        .resize(MAX_COMMAND_VALUES + 1, "overflow".to_owned());

    match encode_batch_multi(&[packet], false, 0, 0, true) {
        Ok(_) => {}
        Err(JolyneError::Io(ref error)) if error.kind() == std::io::ErrorKind::InvalidInput => {
            panic!(
                "valentine_gen appears to enforce encode-side slice ceilings again: restore the \
                 `expect_err` assertion here"
            )
        }
        Err(other) => panic!("unexpected encode error: {other:?}"),
    }
}

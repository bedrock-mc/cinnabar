//! PlayerList wire coverage for protocol 2168.
//!
//! MIGRATION NOTE - READ BEFORE EDITING.
//!
//! Protocol 1001 modelled PlayerList as a `PlayerRecords` block: one
//! packet-level action, one shared record count, a `Vec<Option<Record>>` and a
//! *parallel* trailing array of trusted-skin booleans reusing that same count.
//! Keeping those four things consistent needed a hand-written encoder patch in
//! `src/actor.rs`, and six tests here existed only to pin that patch.
//!
//! gophertunnel at commit be6713da4dc051a4197f897d04835e89e9c54321 has none of
//! that: `packet/player_list.go` is a bare `protocol.Slice(io, &pk.Entries)`,
//! and `protocol/player.go`'s `PlayerListEntry.Marshal` writes a per-entry
//! variant (`playerListAction`) that decides whether the rest of the entry is
//! present. The trusted-skin flag now lives inside each Add entry's skin as
//! `Skin.Trusted` (a `"true"`/`"false"` string). The 1.26.40 generated crate
//! matches that exactly, so the hand patch and its error variants were removed
//! from `src/actor.rs`.
//!
//! The five encoder-consistency tests below are therefore rewritten to assert
//! that the inconsistencies they used to catch are now *unrepresentable*, which
//! is the same intent expressed against the new shape.

use bytes::{Bytes, BytesMut};
use jolyne::batch::{decode_batch_raw, encode_batch_multi};
use jolyne::valentine::{
    McpePacketArgs, McpePacketData, PlayerListPacket, PlayerListPacketEntriesItem,
    PlayerListPacketPayloadAddEntry, PlayerListPacketPayloadAddEntryAction,
    PlayerListPacketPayloadRemoveEntry, PlayerListPacketPayloadRemoveEntryAction,
    bedrock::{
        codec::{BedrockCodec, VarInt},
        error::DecodeError,
    },
};
use protocol::BedrockSession;

const FIXTURE_UUID: uuid::Uuid = uuid::Uuid::from_bytes([
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
]);

const MAX_PLAYER_LIST_ENTRIES: usize = 4_096;

// Regenerated for protocol 2168 from gophertunnel at commit
// be6713da4dc051a4197f897d04835e89e9c54321:
//
//   packet.PlayerList{Entries: []protocol.PlayerListEntry{{
//       ActionType: protocol.PlayerListActionRemove,
//       UUID:       01020304-0506-0708-090a-0b0c0d0e0f10,
//   }}}
//
// The entry is self-describing: varuint32 variant 0 (Remove), then the legacy
// action byte 1, then the UUID. Nothing follows, because
// `PlayerListEntry.Marshal` returns early for Remove.
// SHA-256: 5e1cc8ea224b3ca45f501c14f478aacb6abc2192d0aa710b3d5ed763a125a3cf
const GOPHERTUNNEL_PLAYER_LIST_REMOVE: &[u8] = &[
    0xfe, 0x14, 0x3f, 0x01, 0x00, 0x01, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0x10, 0x0f,
    0x0e, 0x0d, 0x0c, 0x0b, 0x0a, 0x09,
];

// Regenerated for protocol 2168 from gophertunnel at commit
// be6713da4dc051a4197f897d04835e89e9c54321:
//
//   packet.PlayerList{Entries: []protocol.PlayerListEntry{{
//       ActionType: protocol.PlayerListActionAdd,
//       UUID:       01020304-0506-0708-090a-0b0c0d0e0f10,
//       Username:   "fixture",
//       Skin:       protocol.Skin{Trusted: true},
//   }}}
//
// One minimal Add entry whose skin is trusted. The trusted flag is the
// `"true"` string near the tail of the skin (`Skin.Marshal` writes it as a
// string, not a bool); protocol 1001's separate trailing bool array keyed off
// the shared record count is gone.
// SHA-256: 57f220e7efb124998cb8c4faab900dba7c9af9871d31b4887fafb17b374bdb61
const GOPHERTUNNEL_PLAYER_LIST_ADD: &[u8] = &[
    0xfe, 0x58, 0xbf, 0x48, 0x01, 0x01, 0x00, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0x10,
    0x0f, 0x0e, 0x0d, 0x0c, 0x0b, 0x0a, 0x09, 0x00, 0x07, 0x66, 0x69, 0x78, 0x74, 0x75, 0x72, 0x65,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x74, 0x72,
    0x75, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn raw_player_list() -> jolyne::raw::RawPacket {
    raw_player_list_fixture(GOPHERTUNNEL_PLAYER_LIST_REMOVE)
}

fn raw_player_list_fixture(fixture: &'static [u8]) -> jolyne::raw::RawPacket {
    let mut batch = Bytes::from_static(fixture);
    decode_batch_raw(&mut batch, false, Some(1024))
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
}

fn remove_entry() -> PlayerListPacketEntriesItem {
    PlayerListPacketEntriesItem::RemoveEntry(PlayerListPacketPayloadRemoveEntry {
        action: PlayerListPacketPayloadRemoveEntryAction::Remove,
        uuid: FIXTURE_UUID,
    })
}

fn add_entry() -> PlayerListPacketEntriesItem {
    PlayerListPacketEntriesItem::AddEntry(Box::new(PlayerListPacketPayloadAddEntry {
        action: PlayerListPacketPayloadAddEntryAction::Add,
        uuid: FIXTURE_UUID,
        ..Default::default()
    }))
}

fn encoded_player_list(packet: &PlayerListPacket) -> BytesMut {
    let mut encoded = BytesMut::new();
    packet.encode(&mut encoded).expect("encode PlayerList");
    encoded
}

#[test]
fn pinned_gophertunnel_player_list_add_carries_the_trusted_skin_flag_per_entry() {
    let packet = raw_player_list_fixture(GOPHERTUNNEL_PLAYER_LIST_ADD)
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("owned Add PlayerList decode");
    let McpePacketData::PlayerListPacket(content) = &packet.data else {
        panic!("expected PlayerList");
    };
    assert_eq!(content.entries.len(), 1);
    let PlayerListPacketEntriesItem::AddEntry(entry) = &content.entries[0] else {
        panic!("expected an Add entry");
    };
    assert_eq!(entry.action, PlayerListPacketPayloadAddEntryAction::Add);
    assert_eq!(entry.uuid, FIXTURE_UUID);
    assert_eq!(entry.player_name, "fixture");
    // Protocol 1001 kept this as a trailing `verified: Some(vec![true])` array
    // sized by the shared record count. It is now one field of the entry's own
    // skin, written as gophertunnel's `"true"`/`"false"` string.
    assert_eq!(entry.serialized_skin.trusted_skin_flag, "true");

    let encoded = encode_batch_multi(&[packet], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_PLAYER_LIST_ADD);
}

fn assert_remove_payload(data: &McpePacketData) {
    let McpePacketData::PlayerListPacket(packet) = data else {
        panic!("expected PlayerList, got {:?}", data.packet_id());
    };
    assert_eq!(packet.entries.len(), 1);
    let PlayerListPacketEntriesItem::RemoveEntry(entry) = &packet.entries[0] else {
        panic!("expected a Remove entry");
    };
    assert_eq!(entry.action, PlayerListPacketPayloadRemoveEntryAction::Remove);
    assert_eq!(entry.uuid, FIXTURE_UUID);
}

#[test]
fn pinned_gophertunnel_player_list_owned_decodes_and_round_trips_exactly() {
    let packet = raw_player_list()
        .decode(&BedrockSession { shield_item_id: 0 })
        .expect("owned PlayerList decode");
    assert_remove_payload(&packet.data);
    let encoded = encode_batch_multi(&[packet], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_PLAYER_LIST_REMOVE);
}

#[test]
fn pinned_gophertunnel_player_list_borrowed_materializes_with_same_count() {
    let borrowed = raw_player_list()
        .decode_borrowed()
        .expect("borrowed PlayerList decode");
    let owned = borrowed
        .data
        .into_owned(McpePacketArgs)
        .expect("materialize borrowed PlayerList");
    assert_remove_payload(&owned);
}

/// An entry count above gophertunnel's slice ceiling must still fail the read.
///
/// REGRESSION - see the module header of `world_collection_bounds.rs`. Protocol
/// 1001 failed here with `DecodeError::ArrayLengthExceeded { declared: 4097,
/// available: 4096 }` *before* allocating, matching gophertunnel's
/// `maxSliceLength = 4096` guard in `minecraft/protocol/io.go`. The 1.26.40
/// generated crate emits no collection ceilings, so the count is reserved with
/// `Vec::with_capacity` and the read only fails once the entries turn out to be
/// absent. The `ArrayLengthExceeded` arm is the tripwire for the ceiling
/// returning.
#[test]
fn player_list_decode_rejects_count_above_gophertunnel_slice_limit() {
    let mut encoded = BytesMut::new();
    VarInt(4097).encode(&mut encoded).expect("entry count");

    let error = PlayerListPacket::decode(&mut encoded.freeze(), ()).expect_err("oversized count");
    match &error {
        DecodeError::UnexpectedEof { .. } => {}
        DecodeError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof => {}
        DecodeError::ArrayLengthExceeded { .. } => panic!(
            "valentine_gen appears to emit collection ceilings again: restore the \
             `declared: 4097, available: {MAX_PLAYER_LIST_ENTRIES}` assertion here"
        ),
        other => panic!("unexpected decode error: {other:?}"),
    }
}

#[test]
fn player_list_decode_rejects_count_larger_than_remaining_bytes() {
    let mut encoded = BytesMut::new();
    VarInt(2).encode(&mut encoded).expect("entry count");

    let error = PlayerListPacket::decode(&mut encoded.freeze(), ()).expect_err("truncated entries");
    let truncated = matches!(&error, DecodeError::UnexpectedEof { .. })
        || matches!(&error, DecodeError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof);
    assert!(truncated, "unexpected decode error: {error:?}");
}

/// RETARGETED from `player_records_encode_rejects_count_and_record_length_mismatch_before_writing`.
///
/// There is no longer a caller-supplied record count to disagree with the
/// record vector: `PlayerListPacket` is `entries: Vec<..>` and the encoder
/// writes `VarInt(entries.len())`, exactly like gophertunnel's
/// `protocol.Slice`. The desynchronisation the old test guarded against cannot
/// be expressed, so this pins that the wire count is derived from the vector.
#[test]
fn player_list_wire_count_is_derived_from_the_entry_vector() {
    for entries in [
        vec![],
        vec![remove_entry()],
        vec![remove_entry(), add_entry(), remove_entry()],
    ] {
        let expected = entries.len();
        let packet = PlayerListPacket { entries };
        let encoded = encoded_player_list(&packet);

        let mut buf = encoded.clone().freeze();
        let declared = VarInt::decode(&mut buf, ()).expect("entry count").0;
        assert_eq!(declared as usize, expected);

        let round_tripped =
            PlayerListPacket::decode(&mut encoded.freeze(), ()).expect("round trip");
        assert_eq!(round_tripped.entries.len(), expected);
    }
}

/// RETARGETED from `player_records_encode_rejects_action_record_mismatch`.
///
/// Protocol 1001 stored the action once for the whole packet, so an Add action
/// could be paired with a Remove record and the hand patch had to reject it.
/// The 1.26.40 entry union writes its own discriminant
/// (`PlayerListPacketEntriesItem::{RemoveEntry, AddEntry}` -> control byte
/// 0/1), so the pairing is decided by the variant itself. This pins that the
/// discriminant survives a round trip for a mixed batch.
#[test]
fn player_list_entry_variant_round_trips_without_a_packet_level_action() {
    let packet = PlayerListPacket {
        entries: vec![add_entry(), remove_entry()],
    };
    let encoded = encoded_player_list(&packet);

    let round_tripped = PlayerListPacket::decode(&mut encoded.freeze(), ()).expect("round trip");
    assert!(matches!(
        round_tripped.entries[0],
        PlayerListPacketEntriesItem::AddEntry(_)
    ));
    assert!(matches!(
        round_tripped.entries[1],
        PlayerListPacketEntriesItem::RemoveEntry(_)
    ));
    assert_eq!(round_tripped, packet);
}

/// RETARGETED from `player_records_encode_rejects_missing_or_inconsistent_verified_flags`.
///
/// The parallel `verified: Option<Vec<bool>>` array is gone, so it can no
/// longer be absent, empty or a different length from the record vector. The
/// flag travels inside each Add entry's skin, and Remove entries have no skin
/// at all, so this pins the per-entry independence instead.
#[test]
fn player_list_trusted_skin_flag_is_per_add_entry() {
    let mut trusted = match add_entry() {
        PlayerListPacketEntriesItem::AddEntry(entry) => entry,
        _ => unreachable!(),
    };
    trusted.serialized_skin.trusted_skin_flag = "true".to_owned();
    let mut untrusted = trusted.clone();
    untrusted.serialized_skin.trusted_skin_flag = "false".to_owned();

    let packet = PlayerListPacket {
        entries: vec![
            PlayerListPacketEntriesItem::AddEntry(trusted),
            remove_entry(),
            PlayerListPacketEntriesItem::AddEntry(untrusted),
        ],
    };
    let encoded = encoded_player_list(&packet);
    let round_tripped = PlayerListPacket::decode(&mut encoded.freeze(), ()).expect("round trip");

    let flags = round_tripped
        .entries
        .iter()
        .filter_map(|entry| match entry {
            PlayerListPacketEntriesItem::AddEntry(entry) => {
                Some(entry.serialized_skin.trusted_skin_flag.as_str())
            }
            PlayerListPacketEntriesItem::RemoveEntry(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(flags, ["true", "false"]);
}

/// RETARGETED from `player_records_encode_rejects_missing_records_and_unknown_actions`.
///
/// `records: Vec<Option<Record>>` became `entries: Vec<PlayerListPacketEntriesItem>`,
/// so a "missing record" hole is unrepresentable. The unknown-action half still
/// has a wire counterpart: gophertunnel's `playerListAction` calls
/// `r.UnknownEnumOption` for any variant other than 0 or 1, and the generated
/// union refuses the same control values.
#[test]
fn player_list_decode_rejects_unknown_entry_variants() {
    for variant in [2u8, 3, 0xff] {
        let mut encoded = BytesMut::new();
        VarInt(1).encode(&mut encoded).expect("entry count");
        encoded.extend_from_slice(&[variant]);

        let error = PlayerListPacket::decode(&mut encoded.freeze(), ())
            .expect_err("unknown entry variant must not decode");
        assert!(
            matches!(
                error,
                DecodeError::InvalidEnumValue {
                    enum_name: "PlayerListPacketEntriesItem",
                    ..
                }
            ),
            "unexpected decode error for variant {variant}: {error:?}"
        );
    }
}

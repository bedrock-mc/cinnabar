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
//! `Skin.Trusted` (a `"true"`/`"false"` string). The generated codec matches
//! that exactly, so the hand patch and its error variants were removed from
//! `src/actor.rs`.
//!
//! The five encoder-consistency tests below are therefore rewritten to assert
//! that the inconsistencies they used to catch are now *unrepresentable*, which
//! is the same intent expressed against the new shape.

use bedrock_protocol_1_26_51::packets::{self, PlayerList as PlayerListPacket};
use bedrock_protocol_1_26_51::types::{ActorUniqueID, PlayerListData, SerializedSkinRef};
use bedrock_protocol_1_26_51::wire::{self, Decode, Encode};
use bytes::Bytes;

const FIXTURE_UUID: uuid::Uuid = uuid::Uuid::from_bytes([
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
]);

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

fn raw_player_list() -> crate::raw_packet::RawPacket {
    raw_player_list_fixture(GOPHERTUNNEL_PLAYER_LIST_REMOVE)
}

fn raw_player_list_fixture(fixture: &'static [u8]) -> crate::raw_packet::RawPacket {
    crate::raw_packet::decode_batch(Bytes::from_static(fixture), 1024)
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
}

fn remove_entry() -> PlayerListData {
    PlayerListData::RemoveEntry {
        action: Default::default(),
        uuid: FIXTURE_UUID,
    }
}

fn add_entry_with_skin(serialized_skin: SerializedSkinRef) -> PlayerListData {
    PlayerListData::AddEntry {
        action: Default::default(),
        uuid: FIXTURE_UUID,
        actor_unique_id: ActorUniqueID(0),
        player_name: Default::default(),
        xbl_xuid: Default::default(),
        platform_online_id: Default::default(),
        build_platform: Default::default(),
        serialized_skin: Box::new(serialized_skin),
        is_teacher: false,
        is_host: false,
        is_sub_client: false,
        player_color: Default::default(),
    }
}

fn add_entry() -> PlayerListData {
    add_entry_with_skin(SerializedSkinRef::default())
}

fn encoded_player_list(packet: &PlayerListPacket) -> Vec<u8> {
    packet.encode_to_vec().expect("encode PlayerList")
}

/// A declared count the input cannot hold fails before any entry is allocated.
#[track_caller]
fn assert_unrepresentable_count(error: wire::DecodeError) {
    assert!(
        matches!(
            error,
            wire::DecodeError::LengthNotRepresentable { .. }
                | wire::DecodeError::SchemaConstraint(_)
        ),
        "unexpected decode error: {error:?}"
    );
}

#[test]
fn pinned_gophertunnel_player_list_add_carries_the_trusted_skin_flag_per_entry() {
    let packet = raw_player_list_fixture(GOPHERTUNNEL_PLAYER_LIST_ADD)
        .decode()
        .expect("owned Add PlayerList decode");
    let packets::Packet::PlayerList(content) = &*packet.data else {
        panic!("expected PlayerList");
    };
    assert_eq!(content.entries.len(), 1);
    let PlayerListData::AddEntry {
        uuid,
        player_name,
        serialized_skin,
        ..
    } = &content.entries[0]
    else {
        panic!("expected an Add entry");
    };
    assert_eq!(*uuid, FIXTURE_UUID);
    assert_eq!(*player_name, "fixture");
    // Protocol 1001 kept this as a trailing `verified: Some(vec![true])` array
    // sized by the shared record count. It is now one field of the entry's own
    // skin, written as gophertunnel's `"true"`/`"false"` string.
    assert_eq!(serialized_skin.trusted_skin_flag, "true");

    let encoded = crate::encode(&packet).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_PLAYER_LIST_ADD);
}

fn assert_remove_payload(data: &packets::Packet) {
    let packets::Packet::PlayerList(packet) = data else {
        panic!("expected PlayerList, got {:?}", data.id());
    };
    assert_eq!(packet.entries.len(), 1);
    let PlayerListData::RemoveEntry { uuid, .. } = &packet.entries[0] else {
        panic!("expected a Remove entry");
    };
    assert_eq!(*uuid, FIXTURE_UUID);
}

#[test]
fn pinned_gophertunnel_player_list_owned_decodes_and_round_trips_exactly() {
    let packet = raw_player_list().decode().expect("owned PlayerList decode");
    assert_remove_payload(&packet.data);
    let encoded = crate::encode(&packet).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_PLAYER_LIST_REMOVE);
}

/// A large declared count with no entries fails before allocating any entry.
#[test]
fn player_list_decode_rejects_large_count_without_payload() {
    let encoded = wire::VarUInt(4097).encode_to_vec().expect("entry count");

    let error = PlayerListPacket::decode_exact(&encoded).expect_err("oversized count");
    assert_unrepresentable_count(error);
}

#[test]
fn player_list_decode_rejects_count_larger_than_remaining_bytes() {
    let encoded = wire::VarUInt(2).encode_to_vec().expect("entry count");

    let error = PlayerListPacket::decode_exact(&encoded).expect_err("truncated entries");
    assert_unrepresentable_count(error);
}

/// RETARGETED from `player_records_encode_rejects_count_and_record_length_mismatch_before_writing`.
///
/// There is no longer a caller-supplied record count to disagree with the
/// record vector: `PlayerListPacket` is `entries: Vec<..>` and the encoder
/// writes `VarUInt(entries.len())`, exactly like gophertunnel's
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

        let declared = wire::Reader::new(&encoded)
            .read_var_u32()
            .expect("entry count");
        assert_eq!(declared as usize, expected);

        let round_tripped = PlayerListPacket::decode_exact(&encoded).expect("round trip");
        assert_eq!(round_tripped.entries.len(), expected);
    }
}

/// RETARGETED from `player_records_encode_rejects_action_record_mismatch`.
///
/// Protocol 1001 stored the action once for the whole packet, so an Add action
/// could be paired with a Remove record and the hand patch had to reject it.
/// The entry union writes its own discriminant
/// (`PlayerListData::{RemoveEntry, AddEntry}` -> control byte 0/1), so the
/// pairing is decided by the variant itself. This pins that the
/// discriminant survives a round trip for a mixed batch.
#[test]
fn player_list_entry_variant_round_trips_without_a_packet_level_action() {
    let packet = PlayerListPacket {
        entries: vec![add_entry(), remove_entry()],
    };
    let encoded = encoded_player_list(&packet);

    let round_tripped = PlayerListPacket::decode_exact(&encoded).expect("round trip");
    assert!(matches!(
        round_tripped.entries[0],
        PlayerListData::AddEntry { .. }
    ));
    assert!(matches!(
        round_tripped.entries[1],
        PlayerListData::RemoveEntry { .. }
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
    let skin = |flag: &str| SerializedSkinRef {
        trusted_skin_flag: flag.into(),
        ..Default::default()
    };
    let packet = PlayerListPacket {
        entries: vec![
            add_entry_with_skin(skin("true")),
            remove_entry(),
            add_entry_with_skin(skin("false")),
        ],
    };
    let encoded = encoded_player_list(&packet);
    let round_tripped = PlayerListPacket::decode_exact(&encoded).expect("round trip");

    let flags = round_tripped
        .entries
        .iter()
        .filter_map(|entry| match entry {
            PlayerListData::AddEntry {
                serialized_skin, ..
            } => Some(
                serialized_skin
                    .trusted_skin_flag
                    .to_string_lossy()
                    .into_owned(),
            ),
            PlayerListData::RemoveEntry { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(flags, ["true", "false"]);
}

/// RETARGETED from `player_records_encode_rejects_missing_records_and_unknown_actions`.
///
/// `records: Vec<Option<Record>>` became `entries: Vec<PlayerListData>`,
/// so a "missing record" hole is unrepresentable. The unknown-action half still
/// has a wire counterpart: gophertunnel's `playerListAction` calls
/// `r.UnknownEnumOption` for any variant other than 0 or 1, and the generated
/// union refuses the same control values.
#[test]
fn player_list_decode_rejects_unknown_entry_variants() {
    for variant in [2u8, 3, 0xff] {
        let mut encoded = wire::VarUInt(1).encode_to_vec().expect("entry count");
        wire::encode_into(&wire::VarUInt(u32::from(variant)), &mut encoded).expect("entry variant");
        // Supply enough bytes that the discriminant error, rather than a
        // truncated-payload error, is the observed failure.
        encoded.resize(encoded.len() + 17, 0);

        let error = PlayerListPacket::decode_exact(&encoded)
            .expect_err("unknown entry variant must not decode");
        assert!(
            matches!(
                error,
                wire::DecodeError::UnknownVariant {
                    type_name: "PlayerListData",
                    ..
                }
            ),
            "unexpected decode error for variant {variant}: {error:?}"
        );
    }
}

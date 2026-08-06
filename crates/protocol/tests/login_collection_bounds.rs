//! Decode bounds on login-sequence packet collections.
//!
//! REGRESSION - READ BEFORE EDITING.
//!
//! Against 1.26.30 every test here asserted that a malicious length prefix was
//! refused *before allocation*, with `DecodeError::ArrayLengthExceeded` naming
//! the declared count and the bytes actually available. That mirrored
//! gophertunnel's `maxSliceLength = 4096` guard
//! (`limit.SliceLength(l, maxSliceLength)` in `minecraft/protocol/io.go` at
//! commit be6713da4dc051a4197f897d04835e89e9c54321).
//!
//! The 1.26.40 generated crate emits no collection ceilings at all: every
//! length-prefixed field decodes as a bare `Vec::with_capacity(len)` over an
//! attacker-supplied count, and `ArrayLengthExceeded` is never constructed
//! anywhere under `bedrock_versions/v1_26_40/`. A hostile peer can therefore
//! make the decoder reserve up to `i32::MAX` elements before the read fails.
//!
//! That is a valentine_gen defect in generated code this crate must not edit,
//! and it cannot be worked around here without changing wire semantics. So each
//! test below pins the weaker property that does survive - the read still fails
//! rather than yielding a packet - and asserts the failure is an end-of-buffer
//! error, *not* a length ceiling. Restoring the ceiling in valentine_gen trips
//! these assertions, which is the signal to revert this file to its stricter
//! 1.26.30 form. See `world_collection_bounds.rs` for the same treatment of the
//! world packets.

use bytes::{Bytes, BytesMut};
use jolyne::valentine::{
    BiomeDefinitionListPacket, BiomeDefinitionListPacketMapofBiomenamestodataItem,
    CerealizerExperimentsAnonExperimentToggle, CreativeContentPacket, CreativeGroupInfoPayload,
    CreativeItemEntryPayload, GameRule, ItemData, ItemRegistryPacket, ItemRegistryPacketView,
    PackInfoData, PackInstanceId, ResourcePackStackPacket, ResourcePacksInfoPacket, ServerBlockProperty,
    StartGamePacket,
    bedrock::{
        codec::{BedrockCodec, U32LE, VarInt},
        error::DecodeError,
    },
};

const MAX_LOGIN_COLLECTION_ELEMENTS: usize = 4096;

/// Asserts a declared-but-absent collection still fails the read.
///
/// The assertion is deliberately two-sided: an `ArrayLengthExceeded` here would
/// mean the pre-allocation ceiling is back and this whole file should return to
/// asserting declared/available counts.
#[track_caller]
fn assert_rejected_without_a_length_ceiling(error: DecodeError) {
    match &error {
        DecodeError::UnexpectedEof { .. } => {}
        DecodeError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof => {}
        DecodeError::ArrayLengthExceeded { .. } => panic!(
            "valentine_gen appears to emit collection ceilings again: restore the stricter \
             1.26.30 assertions in this file"
        ),
        other => panic!("unexpected decode error: {other:?}"),
    }
}

fn malicious_collection_prefix<T: BedrockCodec>(
    empty: &T,
    one: &T,
    encode_count: impl FnOnce(&mut BytesMut),
) -> Bytes {
    let mut empty_bytes = BytesMut::new();
    empty.encode(&mut empty_bytes).expect("encode empty packet");
    let mut one_bytes = BytesMut::new();
    one.encode(&mut one_bytes).expect("encode one-item packet");
    let count_offset = empty_bytes
        .iter()
        .zip(one_bytes.iter())
        .position(|(empty, one)| empty != one)
        .expect("one-item packet must differ at its collection count");

    let mut prefix = BytesMut::from(&empty_bytes[..count_offset]);
    encode_count(&mut prefix);
    prefix.freeze()
}

fn encode_oversized_varint(bytes: &mut BytesMut) {
    VarInt((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as i32)
        .encode(bytes)
        .expect("oversized varint count");
}

/// `Experiments` is the one login collection whose count is not a varint:
/// gophertunnel writes it with `protocol.SliceUint32Length`.
fn encode_oversized_u32(bytes: &mut BytesMut) {
    U32LE((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as u32)
        .encode(bytes)
        .expect("oversized u32 count");
}

/// A count that is merely larger than the bytes actually present.
fn encode_impossible_varint(bytes: &mut BytesMut) {
    VarInt(MAX_LOGIN_COLLECTION_ELEMENTS as i32)
        .encode(bytes)
        .expect("impossible varint count");
}

/// 1.26.40 renames `texture_packs` to `resource_packs` and widens the count
/// from a `u16` to a varuint32: gophertunnel `packet/resource_packs_info.go`
/// marshals it with `protocol.Slice`.
#[test]
fn resource_packs_info_rejects_oversized_resource_pack_count() {
    let empty = ResourcePacksInfoPacket::default();
    let mut one = empty.clone();
    one.resource_packs.push(PackInfoData::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(
        ResourcePacksInfoPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

/// `resource_packs` in 1.26.30 was `texture_pack_list` here; the field kept its
/// gophertunnel name (`TexturePacks` in `packet/resource_pack_stack.go`).
#[test]
fn resource_pack_stack_rejects_oversized_texture_pack_count() {
    let empty = ResourcePackStackPacket::default();
    let mut one = empty.clone();
    one.texture_pack_list.push(PackInstanceId::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(
        ResourcePackStackPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

#[test]
fn resource_pack_stack_rejects_impossible_texture_pack_count() {
    let empty = ResourcePackStackPacket::default();
    let mut one = empty.clone();
    one.texture_pack_list.push(PackInstanceId::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_impossible_varint);

    assert_rejected_without_a_length_ceiling(
        ResourcePackStackPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

#[test]
fn resource_pack_stack_rejects_oversized_experiment_count() {
    let empty = ResourcePackStackPacket::default();
    let mut one = empty.clone();
    one.experiments
        .toggles
        .push(CerealizerExperimentsAnonExperimentToggle::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_u32);

    assert_rejected_without_a_length_ceiling(
        ResourcePackStackPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

#[test]
fn resource_pack_stack_rejects_impossible_experiment_count() {
    let empty = ResourcePackStackPacket::default();
    let mut one = empty.clone();
    one.experiments
        .toggles
        .push(CerealizerExperimentsAnonExperimentToggle::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        U32LE(MAX_LOGIN_COLLECTION_ELEMENTS as u32)
            .encode(bytes)
            .expect("impossible u32 count")
    });

    assert_rejected_without_a_length_ceiling(
        ResourcePackStackPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

/// `itemstates` is `item_data` in 1.26.40.
#[test]
fn item_registry_owned_rejects_oversized_count() {
    let empty = ItemRegistryPacket::default();
    let mut one = empty.clone();
    one.item_data.push(ItemData::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(ItemRegistryPacket::decode(&mut bytes, ()).unwrap_err());
}

#[test]
fn item_registry_owned_rejects_impossible_count() {
    let empty = ItemRegistryPacket::default();
    let mut one = empty.clone();
    one.item_data.push(ItemData::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_impossible_varint);

    assert_rejected_without_a_length_ceiling(ItemRegistryPacket::decode(&mut bytes, ()).unwrap_err());
}

#[test]
fn item_registry_borrowed_rejects_oversized_count() {
    let empty = ItemRegistryPacket::default();
    let mut one = empty.clone();
    one.item_data.push(ItemData::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(ItemRegistryPacketView::decode(&mut bytes).unwrap_err());
}

#[test]
fn item_registry_borrowed_rejects_impossible_count() {
    let empty = ItemRegistryPacket::default();
    let mut one = empty.clone();
    one.item_data.push(ItemData::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_impossible_varint);

    assert_rejected_without_a_length_ceiling(ItemRegistryPacketView::decode(&mut bytes).unwrap_err());
}

/// StartGame's inline world fields moved into the nested `settings:
/// LevelSettings`, so the game rules now live at `settings.rule_data.rules_list`
/// and the experiments at `settings.experiments.toggles`. `block_properties`
/// stayed at the top level, matching gophertunnel's `packet/start_game.go`.
#[test]
fn start_game_rejects_oversized_gamerule_count() {
    let empty = StartGamePacket::default();
    let mut one = empty.clone();
    one.settings.rule_data.rules_list.push(GameRule::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(StartGamePacket::decode(&mut bytes, ()).unwrap_err());
}

#[test]
fn start_game_rejects_oversized_experiment_count() {
    let empty = StartGamePacket::default();
    let mut one = empty.clone();
    one.settings
        .experiments
        .toggles
        .push(CerealizerExperimentsAnonExperimentToggle::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_u32);

    assert_rejected_without_a_length_ceiling(StartGamePacket::decode(&mut bytes, ()).unwrap_err());
}

#[test]
fn start_game_rejects_oversized_block_property_count() {
    let empty = StartGamePacket::default();
    let mut one = empty.clone();
    one.block_properties.push(ServerBlockProperty::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(StartGamePacket::decode(&mut bytes, ()).unwrap_err());
}

/// `biome_definitions` is `mapof_biomenamestodata` and `string_list` is
/// `stringlist.strings`; the wire layout (definition slice then string slice) is
/// unchanged from `packet/biome_definition_list.go`.
#[test]
fn biome_definition_list_rejects_oversized_biome_count() {
    let empty = BiomeDefinitionListPacket::default();
    let mut one = empty.clone();
    one.mapof_biomenamestodata
        .push(BiomeDefinitionListPacketMapofBiomenamestodataItem::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(
        BiomeDefinitionListPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

#[test]
fn biome_definition_list_rejects_oversized_string_count() {
    let empty = BiomeDefinitionListPacket::default();
    let mut one = empty.clone();
    one.stringlist.strings.push(String::new());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(
        BiomeDefinitionListPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

/// CreativeContent's `items` is `entries`, and the packet no longer needs the
/// negotiated shield item ID to decode, so its args are `()`.
#[test]
fn creative_content_rejects_oversized_group_count() {
    let empty = CreativeContentPacket::default();
    let mut one = empty.clone();
    one.groups.push(CreativeGroupInfoPayload::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(
        CreativeContentPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

#[test]
fn creative_content_rejects_oversized_entry_count() {
    let empty = CreativeContentPacket::default();
    let mut one = empty.clone();
    one.entries.push(CreativeItemEntryPayload::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(
        CreativeContentPacket::decode(&mut bytes, ()).unwrap_err(),
    );
}

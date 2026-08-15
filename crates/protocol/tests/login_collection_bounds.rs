//! Decode bounds on login-sequence packet collections.
//!
//! Generated decoders never trust collection counts for eager allocation. They
//! grow fallibly while decoding and reject malformed/truncated collections
//! without imposing a global 4,096-element ceiling.

use bytes::{Bytes, BytesMut};
use jolyne::valentine::{
    BiomeDefinitionListPacket, BiomeDefinitionListPacketMapofBiomenamestodataItem,
    CerealizerExperimentsAnonExperimentToggle, CreativeContentPacket, CreativeGroupInfoPayload,
    CreativeItemEntryPayload, GameRule, ItemData, ItemRegistryPacket, ItemRegistryPacketView,
    PackInfoData, PackInstanceId, ResourcePackStackPacket, ResourcePacksInfoPacket,
    ServerBlockProperty, StartGamePacket,
    bedrock::{
        codec::{BedrockCodec, U32LE, VarInt},
        error::DecodeError,
    },
};

const MAX_LOGIN_COLLECTION_ELEMENTS: usize = 4096;

/// Asserts a declared-but-absent collection fails without a global ceiling.
#[track_caller]
fn assert_rejected_without_a_length_ceiling(error: DecodeError) {
    assert!(
        matches!(
            error,
            DecodeError::ArrayLengthExceeded {
                declared,
                available
            } if declared > available
        ) || matches!(error, DecodeError::UnexpectedEof { .. }),
        "unexpected decode error: {error:?}"
    );
}

/// Unknown-width element shapes cannot prove a byte lower bound. They start at
/// zero capacity, grow fallibly per decoded item, and still reject truncation.
#[track_caller]
fn assert_unknown_width_rejected_fallibly(error: DecodeError) {
    assert!(
        matches!(error, DecodeError::UnexpectedEof { .. })
            || matches!(&error, DecodeError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof),
        "unexpected decode error: {error:?}"
    );
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

fn encode_resource_pack_offer_limit_plus_one(bytes: &mut BytesMut) {
    VarInt(33)
        .encode(bytes)
        .expect("resource-pack offer count above the wire cap");
}

fn encode_resource_pack_stack_limit_plus_one(bytes: &mut BytesMut) {
    VarInt(40)
        .encode(bytes)
        .expect("resource-pack stack count above the wire cap");
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
    let mut bytes =
        malicious_collection_prefix(&empty, &one, encode_resource_pack_offer_limit_plus_one);

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
    let mut bytes =
        malicious_collection_prefix(&empty, &one, encode_resource_pack_stack_limit_plus_one);

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

    assert_unknown_width_rejected_fallibly(ItemRegistryPacket::decode(&mut bytes, ()).unwrap_err());
}

#[test]
fn item_registry_owned_rejects_impossible_count() {
    let empty = ItemRegistryPacket::default();
    let mut one = empty.clone();
    one.item_data.push(ItemData::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_impossible_varint);

    assert_unknown_width_rejected_fallibly(ItemRegistryPacket::decode(&mut bytes, ()).unwrap_err());
}

#[test]
fn item_registry_borrowed_rejects_oversized_count() {
    let empty = ItemRegistryPacket::default();
    let mut one = empty.clone();
    one.item_data.push(ItemData::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_unknown_width_rejected_fallibly(ItemRegistryPacketView::decode(&mut bytes).unwrap_err());
}

#[test]
fn item_registry_borrowed_rejects_impossible_count() {
    let empty = ItemRegistryPacket::default();
    let mut one = empty.clone();
    one.item_data.push(ItemData::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_impossible_varint);

    assert_unknown_width_rejected_fallibly(ItemRegistryPacketView::decode(&mut bytes).unwrap_err());
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

    assert_unknown_width_rejected_fallibly(StartGamePacket::decode(&mut bytes, ()).unwrap_err());
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

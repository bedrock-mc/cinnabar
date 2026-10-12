//! Decode bounds on login-sequence packet collections.
//!
//! Generated decoders check every declared count against the remaining input before
//! allocating, and reject malformed/truncated collections without imposing a global
//! 4,096-element ceiling.

use bedrock_protocol_1_26_51::packets::{
    BiomeDefinitionList, CreativeContent, ItemRegistry, ResourcePackStack, ResourcePacksInfo,
    StartGame,
};
use bedrock_protocol_1_26_51::types::{
    BiomeDefinitionData, CreativeGroupInfo, CreativeItemEntry, ExperimentToggle, GameRule,
    ItemData, PackInfoData, PackInstanceId, ServerBlockProperty,
};
use bedrock_protocol_1_26_51::wire::{self, Decode, Encode};

const MAX_LOGIN_COLLECTION_ELEMENTS: usize = 4096;

/// Asserts a declared-but-absent collection fails as truncated input, not a length ceiling.
#[track_caller]
fn assert_rejected_without_a_length_ceiling(error: wire::DecodeError) {
    assert!(
        matches!(
            error,
            wire::DecodeError::LengthNotRepresentable { needed, remaining, .. }
                if needed > remaining
        ) || matches!(error, wire::DecodeError::UnexpectedEof { .. }),
        "unexpected decode error: {error:?}"
    );
}

/// The encoding of `empty` up to its collection count, followed by the count `encode_count`
/// writes in place of the real one.
fn malicious_collection_prefix<T: Encode>(
    empty: &T,
    one: &T,
    encode_count: impl FnOnce(&mut wire::Writer<'_>),
) -> Vec<u8> {
    let empty_bytes = empty.encode_to_vec().expect("encode empty packet");
    let one_bytes = one.encode_to_vec().expect("encode one-item packet");
    let count_offset = empty_bytes
        .iter()
        .zip(one_bytes.iter())
        .position(|(empty, one)| empty != one)
        .expect("one-item packet must differ at its collection count");

    let mut prefix = empty_bytes[..count_offset].to_vec();
    encode_count(&mut wire::Writer::new(&mut prefix));
    prefix
}

fn encode_oversized_varint(writer: &mut wire::Writer<'_>) {
    writer
        .write_var_u32((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as u32)
        .expect("oversized varint count");
}

fn encode_resource_pack_offer_limit_plus_one(writer: &mut wire::Writer<'_>) {
    writer
        .write_var_u32(33)
        .expect("resource-pack offer count above the wire cap");
}

fn encode_resource_pack_stack_limit_plus_one(writer: &mut wire::Writer<'_>) {
    writer
        .write_var_u32(40)
        .expect("resource-pack stack count above the wire cap");
}

/// `Experiments` is the one login collection whose count is not a varint:
/// gophertunnel writes it with `protocol.SliceUint32Length`.
fn encode_oversized_u32(writer: &mut wire::Writer<'_>) {
    writer
        .write_all(&((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as u32).to_le_bytes())
        .expect("oversized u32 count");
}

/// A count that is merely larger than the bytes actually present.
fn encode_impossible_varint(writer: &mut wire::Writer<'_>) {
    writer
        .write_var_u32(MAX_LOGIN_COLLECTION_ELEMENTS as u32)
        .expect("impossible varint count");
}

/// The resource-pack count is a varuint32: gophertunnel `packet/resource_packs_info.go`
/// marshals it with `protocol.Slice`.
#[test]
fn resource_packs_info_rejects_oversized_resource_pack_count() {
    let empty = ResourcePacksInfo::default();
    let mut one = empty.clone();
    one.resource_packs.push(PackInfoData::default());
    let bytes =
        malicious_collection_prefix(&empty, &one, encode_resource_pack_offer_limit_plus_one);

    assert_rejected_without_a_length_ceiling(ResourcePacksInfo::decode_exact(&bytes).unwrap_err());
}

/// The field keeps its gophertunnel name (`TexturePacks` in `packet/resource_pack_stack.go`).
#[test]
fn resource_pack_stack_rejects_oversized_texture_pack_count() {
    let empty = ResourcePackStack::default();
    let mut one = empty.clone();
    one.texture_pack_list.push(PackInstanceId::default());
    let bytes =
        malicious_collection_prefix(&empty, &one, encode_resource_pack_stack_limit_plus_one);

    assert_rejected_without_a_length_ceiling(ResourcePackStack::decode_exact(&bytes).unwrap_err());
}

#[test]
fn resource_pack_stack_rejects_impossible_texture_pack_count() {
    let empty = ResourcePackStack::default();
    let mut one = empty.clone();
    one.texture_pack_list.push(PackInstanceId::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_impossible_varint);

    assert_rejected_without_a_length_ceiling(ResourcePackStack::decode_exact(&bytes).unwrap_err());
}

#[test]
fn resource_pack_stack_rejects_oversized_experiment_count() {
    let empty = ResourcePackStack::default();
    let mut one = empty.clone();
    one.experiments.toggles.push(ExperimentToggle::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_oversized_u32);

    assert_rejected_without_a_length_ceiling(ResourcePackStack::decode_exact(&bytes).unwrap_err());
}

#[test]
fn resource_pack_stack_rejects_impossible_experiment_count() {
    let empty = ResourcePackStack::default();
    let mut one = empty.clone();
    one.experiments.toggles.push(ExperimentToggle::default());
    let bytes = malicious_collection_prefix(&empty, &one, |writer| {
        writer
            .write_all(&(MAX_LOGIN_COLLECTION_ELEMENTS as u32).to_le_bytes())
            .expect("impossible u32 count")
    });

    assert_rejected_without_a_length_ceiling(ResourcePackStack::decode_exact(&bytes).unwrap_err());
}

#[test]
fn item_registry_rejects_oversized_count() {
    let empty = ItemRegistry::default();
    let mut one = empty.clone();
    one.item_data.push(ItemData::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(ItemRegistry::decode_exact(&bytes).unwrap_err());
}

#[test]
fn item_registry_rejects_impossible_count() {
    let empty = ItemRegistry::default();
    let mut one = empty.clone();
    one.item_data.push(ItemData::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_impossible_varint);

    assert_rejected_without_a_length_ceiling(ItemRegistry::decode_exact(&bytes).unwrap_err());
}

/// The game rules live at `settings.rule_data.rules_list` and the experiments at
/// `settings.experiments.toggles`; `block_properties` stays at the top level, matching
/// gophertunnel's `packet/start_game.go`.
#[test]
fn start_game_rejects_oversized_gamerule_count() {
    let empty = StartGame::default();
    let mut one = empty.clone();
    one.settings.rule_data.rules_list.push(GameRule::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(StartGame::decode_exact(&bytes).unwrap_err());
}

#[test]
fn start_game_rejects_oversized_experiment_count() {
    let empty = StartGame::default();
    let mut one = empty.clone();
    one.settings
        .experiments
        .toggles
        .push(ExperimentToggle::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_oversized_u32);

    assert_rejected_without_a_length_ceiling(StartGame::decode_exact(&bytes).unwrap_err());
}

#[test]
fn start_game_rejects_oversized_block_property_count() {
    let empty = StartGame::default();
    let mut one = empty.clone();
    one.block_properties.push(ServerBlockProperty::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(StartGame::decode_exact(&bytes).unwrap_err());
}

/// The wire layout (definition map then string slice) matches
/// `packet/biome_definition_list.go`.
#[test]
fn biome_definition_list_rejects_oversized_biome_count() {
    let empty = BiomeDefinitionList::default();
    let mut one = empty.clone();
    one.map_of_biome_names_to_data
        .push((wire::U16LE(0), BiomeDefinitionData::default()));
    let bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(
        BiomeDefinitionList::decode_exact(&bytes).unwrap_err(),
    );
}

#[test]
fn biome_definition_list_rejects_oversized_string_count() {
    let empty = BiomeDefinitionList::default();
    let mut one = empty.clone();
    one.string_list.strings.push(wire::Str::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(
        BiomeDefinitionList::decode_exact(&bytes).unwrap_err(),
    );
}

#[test]
fn creative_content_rejects_oversized_group_count() {
    let empty = CreativeContent::default();
    let mut one = empty.clone();
    one.groups.push(CreativeGroupInfo::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(CreativeContent::decode_exact(&bytes).unwrap_err());
}

#[test]
fn creative_content_rejects_oversized_entry_count() {
    let empty = CreativeContent::default();
    let mut one = empty.clone();
    one.entries.push(CreativeItemEntry::default());
    let bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    assert_rejected_without_a_length_ceiling(CreativeContent::decode_exact(&bytes).unwrap_err());
}

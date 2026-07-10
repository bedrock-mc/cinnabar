use bytes::{Bytes, BytesMut};
use jolyne::valentine::{
    BiomeDefinition, BiomeDefinitionListPacket, BlockPropertiesItem, CreativeContentPacket,
    CreativeContentPacketArgs, CreativeContentPacketGroupsItem, CreativeContentPacketItemsItem,
    Experiment, GameRuleVarint, ItemRegistryPacket, ItemRegistryPacketView,
    ResourcePackStackPacket, ResourcePacksInfoPacket, StartGamePacket, TexturePackInfosItem,
    bedrock::{
        codec::{BedrockCodec, I16LE, I32LE, VarInt},
        error::DecodeError,
    },
};

const MAX_LOGIN_COLLECTION_ELEMENTS: usize = 4096;

fn assert_limit_error(error: DecodeError, declared: usize, available: usize) {
    assert!(
        matches!(
            error,
            DecodeError::ArrayLengthExceeded {
                declared: actual_declared,
                available: actual_available,
            } if actual_declared == declared && actual_available == available
        ),
        "unexpected decode error: {error:?}",
    );
}

fn resource_pack_stack_prefix(resource_pack_count: i32) -> Bytes {
    let mut bytes = BytesMut::new();
    false.encode(&mut bytes).expect("must-accept flag");
    VarInt(resource_pack_count)
        .encode(&mut bytes)
        .expect("resource-pack count");
    bytes.freeze()
}

fn resource_pack_stack_experiments_prefix(experiment_count: i32) -> Bytes {
    let mut bytes = BytesMut::new();
    false.encode(&mut bytes).expect("must-accept flag");
    VarInt(0)
        .encode(&mut bytes)
        .expect("empty resource-pack stack");
    VarInt(0).encode(&mut bytes).expect("empty game version");
    I32LE(experiment_count)
        .encode(&mut bytes)
        .expect("experiment count");
    bytes.freeze()
}

fn item_registry_prefix(item_count: i32) -> Bytes {
    let mut bytes = BytesMut::new();
    VarInt(item_count)
        .encode(&mut bytes)
        .expect("item-state count");
    bytes.freeze()
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

fn encode_oversized_i32(bytes: &mut BytesMut) {
    I32LE((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as i32)
        .encode(bytes)
        .expect("oversized i32 count");
}

#[test]
fn resource_packs_info_rejects_oversized_texture_pack_count_before_allocation() {
    let empty = ResourcePacksInfoPacket::default();
    let mut one = empty.clone();
    one.texture_packs.push(TexturePackInfosItem::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, |bytes| {
        I16LE((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as i16)
            .encode(bytes)
            .expect("oversized i16 count");
    });

    let error = ResourcePacksInfoPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn resource_pack_stack_rejects_oversized_resource_pack_count_before_allocation() {
    let mut bytes = resource_pack_stack_prefix((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as i32);
    let error = ResourcePackStackPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn resource_pack_stack_rejects_impossible_resource_pack_count_before_allocation() {
    let mut bytes = resource_pack_stack_prefix(MAX_LOGIN_COLLECTION_ELEMENTS as i32);
    let error = ResourcePackStackPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(error, MAX_LOGIN_COLLECTION_ELEMENTS, 0);
}

#[test]
fn resource_pack_stack_rejects_oversized_experiment_count_before_allocation() {
    let mut bytes =
        resource_pack_stack_experiments_prefix((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as i32);
    let error = ResourcePackStackPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn resource_pack_stack_rejects_impossible_experiment_count_before_allocation() {
    let mut bytes = resource_pack_stack_experiments_prefix(MAX_LOGIN_COLLECTION_ELEMENTS as i32);
    let error = ResourcePackStackPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(error, MAX_LOGIN_COLLECTION_ELEMENTS, 0);
}

#[test]
fn item_registry_owned_rejects_oversized_count_before_allocation() {
    let mut bytes = item_registry_prefix((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as i32);
    let error = ItemRegistryPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn item_registry_owned_rejects_impossible_count_before_allocation() {
    let mut bytes = item_registry_prefix(MAX_LOGIN_COLLECTION_ELEMENTS as i32);
    let error = ItemRegistryPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(error, MAX_LOGIN_COLLECTION_ELEMENTS, 0);
}

#[test]
fn item_registry_borrowed_rejects_oversized_count_before_allocation() {
    let mut bytes = item_registry_prefix((MAX_LOGIN_COLLECTION_ELEMENTS + 1) as i32);
    let error = ItemRegistryPacketView::decode(&mut bytes).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn item_registry_borrowed_rejects_impossible_count_before_allocation() {
    let mut bytes = item_registry_prefix(MAX_LOGIN_COLLECTION_ELEMENTS as i32);
    let error = ItemRegistryPacketView::decode(&mut bytes).unwrap_err();
    assert_limit_error(error, MAX_LOGIN_COLLECTION_ELEMENTS, 0);
}

#[test]
fn start_game_rejects_oversized_gamerule_count_before_allocation() {
    let empty = StartGamePacket::default();
    let mut one = empty.clone();
    one.gamerules.push(GameRuleVarint::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    let error = StartGamePacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn start_game_rejects_oversized_experiment_count_before_allocation() {
    let empty = StartGamePacket::default();
    let mut one = empty.clone();
    one.experiments.push(Experiment::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_i32);

    let error = StartGamePacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn start_game_rejects_oversized_block_property_count_before_allocation() {
    let empty = StartGamePacket::default();
    let mut one = empty.clone();
    one.block_properties.push(BlockPropertiesItem::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    let error = StartGamePacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn biome_definition_list_rejects_oversized_biome_count_before_allocation() {
    let empty = BiomeDefinitionListPacket::default();
    let mut one = empty.clone();
    one.biome_definitions.push(BiomeDefinition::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    let error = BiomeDefinitionListPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn biome_definition_list_rejects_oversized_string_count_before_allocation() {
    let empty = BiomeDefinitionListPacket::default();
    let mut one = empty.clone();
    one.string_list.push(String::new());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    let error = BiomeDefinitionListPacket::decode(&mut bytes, ()).unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn creative_content_rejects_oversized_group_count_before_allocation() {
    let empty = CreativeContentPacket::default();
    let mut one = empty.clone();
    one.groups.push(CreativeContentPacketGroupsItem::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    let error =
        CreativeContentPacket::decode(&mut bytes, CreativeContentPacketArgs { shield_item_id: 0 })
            .unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

#[test]
fn creative_content_rejects_oversized_item_count_before_allocation() {
    let empty = CreativeContentPacket::default();
    let mut one = empty.clone();
    one.items.push(CreativeContentPacketItemsItem::default());
    let mut bytes = malicious_collection_prefix(&empty, &one, encode_oversized_varint);

    let error =
        CreativeContentPacket::decode(&mut bytes, CreativeContentPacketArgs { shield_item_id: 0 })
            .unwrap_err();
    assert_limit_error(
        error,
        MAX_LOGIN_COLLECTION_ELEMENTS + 1,
        MAX_LOGIN_COLLECTION_ELEMENTS,
    );
}

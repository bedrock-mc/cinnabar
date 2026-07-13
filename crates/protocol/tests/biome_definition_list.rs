use bytes::{Bytes, BytesMut};
use jolyne::{
    batch::{decode_batch_raw, encode_batch_multi},
    raw::RawPacket,
    valentine::{McpePacketArgs, McpePacketData, McpePacketName},
};
use protocol::BedrockSession;
use valentine::bedrock::{
    codec::{BedrockCodec, VarInt},
    error::DecodeError,
    version::v1_26_30::BiomeChunkGeneration,
};

const GOPHERTUNNEL_BIOME_DEFINITION_LIST: &[u8] =
    include_bytes!("../fixtures/biome_definition_list_chunk_generation.bin");
const MAX_BIOME_COLLECTION_ELEMENTS: usize = 4_096;

fn session() -> BedrockSession {
    BedrockSession { shield_item_id: 0 }
}

fn raw_fixture() -> RawPacket {
    let mut batch = Bytes::from_static(GOPHERTUNNEL_BIOME_DEFINITION_LIST);
    decode_batch_raw(&mut batch, false, Some(1_024))
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
}

fn assert_fixture_fields(data: &McpePacketData) {
    let McpePacketData::PacketBiomeDefinitionList(packet) = data else {
        panic!("expected BiomeDefinitionList, got {:?}", data.packet_id());
    };
    assert_eq!(packet.biome_definitions.len(), 1);
    assert!(packet.string_list.is_empty());
    let chunk = packet.biome_definitions[0]
        .chunk_generation
        .as_ref()
        .expect("fixture has chunk generation");
    assert!(chunk.climate.is_none());
    assert!(chunk.consolidated_features.is_none());
    assert!(chunk.mountain_parameters.is_none());
    assert!(chunk.surface_material_adjustments.is_none());
    assert!(chunk.overworld_rules.is_none());
    assert!(chunk.multi_noise_rules.is_none());
    assert!(chunk.legacy_rules.is_none());
    assert!(chunk.replacements_data.is_none());
    assert!(chunk.village_type.is_none());
    assert!(chunk.surface_builder.is_none());
    assert!(chunk.subsurface_builder.is_none());
}

#[test]
fn pinned_gophertunnel_biome_definition_list_owned_decodes_and_round_trips() {
    let packet = raw_fixture()
        .decode(&session())
        .expect("owned BiomeDefinitionList decode");
    assert_eq!(
        packet.data.packet_id(),
        McpePacketName::PacketBiomeDefinitionList
    );
    assert_fixture_fields(&packet.data);
    let encoded = encode_batch_multi(&[packet], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_BIOME_DEFINITION_LIST);
}

#[test]
fn pinned_gophertunnel_biome_definition_list_borrowed_raw_materializes_and_round_trips() {
    let borrowed = raw_fixture()
        .decode_borrowed()
        .expect("borrowed BiomeDefinitionList decode");
    assert!(matches!(
        &borrowed.data,
        valentine::bedrock::version::v1_26_30::BorrowedMcpePacketData::Raw {
            name: McpePacketName::PacketBiomeDefinitionList,
            ..
        }
    ));
    let owned = borrowed
        .into_owned(McpePacketArgs { shield_item_id: 0 })
        .expect("materialize borrowed BiomeDefinitionList");
    assert_fixture_fields(&owned.data);
    let encoded = encode_batch_multi(&[owned], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_BIOME_DEFINITION_LIST);
}

#[test]
fn biome_chunk_generation_rejects_oversized_nested_collection_before_allocation() {
    let mut bytes = BytesMut::from(&[0, 1][..]);
    VarInt((MAX_BIOME_COLLECTION_ELEMENTS + 1) as i32)
        .encode(&mut bytes)
        .expect("encode oversized collection count");

    let error = BiomeChunkGeneration::decode(&mut bytes.freeze(), ())
        .expect_err("4,097 consolidated features must fail before allocation");
    assert!(matches!(
        error,
        DecodeError::ArrayLengthExceeded {
            declared: 4_097,
            available: MAX_BIOME_COLLECTION_ELEMENTS,
        }
    ));
}

#[test]
fn biome_chunk_generation_rejects_truncated_surface_builder_slots() {
    let mut bytes = Bytes::from_static(&[0; 10]);
    let error = BiomeChunkGeneration::decode(&mut bytes, ())
        .expect_err("ten of eleven option slots must be truncated");
    assert!(matches!(error, DecodeError::UnexpectedEof { .. }));
}

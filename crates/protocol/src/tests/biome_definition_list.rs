use bedrock_protocol_1_26_51::packets::{self, PacketId};
use bedrock_protocol_1_26_51::types::BiomeDefinitionChunkGenData;
use bedrock_protocol_1_26_51::wire::{self, Decode};
use bytes::Bytes;

use crate::raw_packet::RawPacket;

const GOPHERTUNNEL_BIOME_DEFINITION_LIST: &[u8] =
    include_bytes!("../../fixtures/biome_definition_list_chunk_generation.bin");
const MAX_BIOME_COLLECTION_ELEMENTS: usize = 4_096;

fn raw_fixture() -> RawPacket {
    crate::raw_packet::decode_batch(
        Bytes::from_static(GOPHERTUNNEL_BIOME_DEFINITION_LIST),
        1_024,
    )
    .expect("raw batch decode")
    .into_iter()
    .next()
    .expect("one packet")
}

fn assert_fixture_fields(data: &packets::Packet) {
    let packets::Packet::BiomeDefinitionList(packet) = data else {
        panic!("expected BiomeDefinitionList, got {:?}", data.id());
    };
    // gophertunnel packet/biome_definition_list.go writes the definition slice then a
    // FuncSlice of strings.
    assert_eq!(packet.map_of_biome_names_to_data.len(), 1);
    assert!(packet.string_list.strings.is_empty());
    let chunk = packet.map_of_biome_names_to_data[0]
        .1
        .chunk_gen_data
        .as_ref()
        .expect("fixture has chunk generation");
    assert!(chunk.climate.is_none());
    assert!(chunk.consolidated_features.is_none());
    assert!(chunk.mountain_params.is_none());
    assert!(chunk.surface_material_adjustments.is_none());
    assert!(chunk.overworld_gen_rules.is_none());
    assert!(chunk.multinoise_gen_rules.is_none());
    assert!(chunk.legacy_world_gen_rules.is_none());
    assert!(chunk.replacement_biomes.is_none());
    assert!(chunk.village_type.is_none());
    assert!(chunk.surface_builder_data.is_none());
    assert!(chunk.subsurface_builder_data.is_none());
}

#[test]
fn pinned_gophertunnel_biome_definition_list_decodes_and_round_trips() {
    let packet = raw_fixture().decode().expect("BiomeDefinitionList decode");
    assert_eq!(packet.data.id(), PacketId::BiomeDefinitionList);
    assert_fixture_fields(&packet.data);
    let encoded = crate::encode(&packet).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_BIOME_DEFINITION_LIST);
}

/// A hostile nested count remains fatal when its first declared element is
/// absent, without a global element ceiling.
#[test]
fn biome_chunk_generation_rejects_oversized_nested_collection() {
    let mut bytes = vec![0, 1];
    wire::Writer::new(&mut bytes)
        .write_var_u32((MAX_BIOME_COLLECTION_ELEMENTS + 1) as u32)
        .expect("encode oversized collection count");

    let error = BiomeDefinitionChunkGenData::decode_exact(&bytes)
        .expect_err("4,097 consolidated features must not decode");
    assert!(matches!(
        error,
        wire::DecodeError::UnexpectedEof { .. } | wire::DecodeError::LengthNotRepresentable { .. }
    ));
}

#[test]
fn biome_chunk_generation_rejects_truncated_surface_builder_slots() {
    // `BiomeDefinitionChunkGenData` is eleven optional slots; ten present bytes
    // leaves the last one truncated.
    let error = BiomeDefinitionChunkGenData::decode_exact(&[0; 10])
        .expect_err("ten of eleven option slots must be truncated");
    assert!(matches!(error, wire::DecodeError::UnexpectedEof { .. }));
}

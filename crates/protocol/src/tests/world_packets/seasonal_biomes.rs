use super::*;
use bedrock_protocol_1_26_51::types::{BiomeClimateData, BiomeDefinitionChunkGenData};

#[test]
fn biome_snow_accumulation_preserves_optional_wire_climate_and_ignores_nonfinite_optional_data() {
    for (maximum, expected) in [(Some(0.5), Some(0.5)), (None, None), (Some(f32::NAN), None)] {
        let mut definition = biome_definition(0, 7);
        definition.1.chunk_gen_data = maximum.map(|maximum| BiomeDefinitionChunkGenData {
            climate: Some(BiomeClimateData {
                snow_accumulation_max: wire::F32LE(maximum),
                ..Default::default()
            }),
            ..Default::default()
        });
        let WorldEvent::BiomeDefinitions(event) = into_world_event(
            protocol::Packet::new(biome_packet(vec![definition], vec!["custom:snow".into()])),
            0,
        )
        .unwrap()
        .unwrap() else {
            panic!("expected climate");
        };
        assert_eq!(event.definitions[0].max_snow_accumulation, expected);
        assert_eq!(event.definitions[0].snow_foliage, 0.125);
    }
}

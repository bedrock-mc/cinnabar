use super::*;
use valentine::bedrock::version::v1_26_51::{BiomeClimateData, BiomeDefinitionChunkGenData};

#[test]
fn biome_snow_accumulation_preserves_optional_wire_climate_and_ignores_nonfinite_optional_data() {
    for (maximum, expected) in [(Some(0.5), Some(0.5)), (None, None), (Some(f32::NAN), None)] {
        let mut definition = biome_definition(0, 7);
        definition.value.chunkgendata =
            maximum.map(|snowaccumulationmax| BiomeDefinitionChunkGenData {
                climate: Some(BiomeClimateData {
                    snowaccumulationmax,
                    ..Default::default()
                }),
                ..Default::default()
            });
        let WorldEvent::BiomeDefinitions(event) = into_world_event(
            biome_packet(vec![definition], vec!["custom:snow".into()]).into(),
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

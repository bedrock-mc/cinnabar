use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::BiomeDefinitionList;

use super::{
    BiomeDefinitionEvent, BiomeDefinitionsEvent, MAX_BIOME_DEFINITIONS, MAX_BIOME_NAME_BYTES,
    WorldPacketError,
};

/// Keeps custom biome names intact while qualifying known retail names.
fn canonical_biome_name(name: &str) -> Arc<str> {
    if name.contains(':') {
        return Arc::from(name);
    }
    const RETAIL_BIOMES: &str = include_str!("../../data/retail_biomes_1_26_50.txt");
    let known_retail = RETAIL_BIOMES
        .lines()
        .any(|identifier| identifier.strip_prefix("minecraft:") == Some(name));
    if known_retail {
        Arc::from(format!("minecraft:{name}"))
    } else {
        Arc::from(name)
    }
}

/// Validates the string-table indices, name lengths and climate of a BiomeDefinitionList.
pub(super) fn normalize_biome_definitions(
    packet: BiomeDefinitionList,
) -> Result<BiomeDefinitionsEvent, WorldPacketError> {
    let string_list = packet.string_list.strings;
    let biome_definitions = packet.map_of_biome_names_to_data;
    if biome_definitions.len() > MAX_BIOME_DEFINITIONS {
        return Err(WorldPacketError::TooManyBiomeDefinitions {
            count: biome_definitions.len(),
            max: MAX_BIOME_DEFINITIONS,
        });
    }
    let mut definitions = Vec::with_capacity(biome_definitions.len());
    for (definition_index, (key, definition)) in biome_definitions.into_iter().enumerate() {
        // gophertunnel protocol/biome.go declares the key as a signed Int16 NameIndex, so it is
        // reinterpreted here to report out-of-range indices as signed.
        let name_index = key.0 as i16;
        let name = usize::try_from(name_index)
            .ok()
            .and_then(|index| string_list.get(index))
            .ok_or(WorldPacketError::InvalidBiomeNameIndex {
                index: name_index,
                string_count: string_list.len(),
            })?;
        let name = name.to_string_lossy();
        if name.len() > MAX_BIOME_NAME_BYTES {
            return Err(WorldPacketError::BiomeNameTooLong {
                bytes: name.len(),
                max: MAX_BIOME_NAME_BYTES,
            });
        }
        let (temperature, downfall, snow_foliage) = (
            definition.temperature.0,
            definition.downfall.0,
            definition.foliage_snow.0,
        );
        for (field, value) in [
            ("temperature", temperature),
            ("downfall", downfall),
            ("snow_foliage", snow_foliage),
        ] {
            if !value.is_finite() {
                return Err(WorldPacketError::NonFiniteBiomeClimate {
                    definition: definition_index,
                    field,
                });
            }
        }
        // Climate is optional on the wire. Ignore an unusable optional
        // field instead of rejecting a well-framed biome definition.
        let max_snow_accumulation = definition
            .chunk_gen_data
            .as_ref()
            .and_then(|generation| generation.climate.as_ref())
            .map(|climate| climate.snow_accumulation_max.0)
            .filter(|value| value.is_finite());
        definitions.push(BiomeDefinitionEvent {
            biome_id: (definition.id.0 != u16::MAX).then_some(definition.id.0),
            name: canonical_biome_name(&name),
            temperature,
            downfall,
            snow_foliage,
            max_snow_accumulation,
            map_water_color: definition.map_water_color_argb.0 as u32,
        });
    }
    Ok(BiomeDefinitionsEvent {
        definitions: Arc::from(definitions),
    })
}

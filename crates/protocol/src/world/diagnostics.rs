//! Compact dimension definitions and wire facts retained for lighting diagnostics.

use std::sync::Arc;

use bedrock_protocol_1_26_51::enums::GeneratorType;
use bedrock_protocol_1_26_51::packets::DimensionData;
use bedrock_protocol_1_26_51::types::SubChunkData;
use bedrock_protocol_1_26_51::wire;

/// Vertical bounds advertised separately from StartGame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionHeightDiagnostic {
    pub name: Arc<str>,
    pub dimension: i32,
    pub minimum_y: i32,
    pub height_range: i32,
    pub generator: i32,
}

pub(super) fn dimension_generator_id(generator: GeneratorType) -> i32 {
    generator.to_raw()
}

/// A bounded summary of one server heightmap, including an explicitly absent payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeightmapDiagnostic {
    /// Wire type: 0 no data, 1 data, 2 all too high, 3 all too low, 4 render copied.
    pub kind: u8,
    pub payload_present: bool,
    pub sample_count: usize,
    pub min: Option<i8>,
    pub max: Option<i8>,
}

/// Metadata accompanying a sub-chunk result; no light arrays are carried by this packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubChunkDiagnostic {
    /// Payload available to decoding, including data reconstructed from cached blobs.
    pub payload_present: bool,
    pub heightmap: HeightmapDiagnostic,
    pub render_heightmap: HeightmapDiagnostic,
}

impl SubChunkDiagnostic {
    /// Retains wire metadata before block decoding consumes the serialized payload.
    pub(super) fn from_entry(entry: &SubChunkData) -> Self {
        let data = &entry.height_map_data;
        Self {
            payload_present: entry.serialized_sub_chunk.is_some(),
            heightmap: summarize_heightmap(
                data.height_map_type.to_raw(),
                data.subchunk_height_map.as_ref(),
            ),
            render_heightmap: summarize_heightmap(
                data.render_height_map_type.to_raw(),
                data.subchunk_render_height_map.as_ref(),
            ),
        }
    }
}

/// Reduces arbitrary well-formed heightmap rows to fixed-size diagnostic facts.
fn summarize_heightmap(kind: u8, rows: Option<&[Vec<wire::I8>; 16]>) -> HeightmapDiagnostic {
    let mut result = HeightmapDiagnostic {
        kind,
        payload_present: rows.is_some(),
        sample_count: 0,
        min: None,
        max: None,
    };
    for value in rows.into_iter().flatten().flatten().map(|value| value.0) {
        result.sample_count += 1;
        result.min = Some(result.min.map_or(value, |previous| previous.min(value)));
        result.max = Some(result.max.map_or(value, |previous| previous.max(value)));
    }
    result
}

/// The first [`super::MAX_DIMENSION_DEFINITIONS`] advertised dimension bounds.
pub(super) fn normalize_dimension_data(packet: DimensionData) -> Vec<DimensionHeightDiagnostic> {
    packet
        .definitions
        .into_iter()
        .take(super::MAX_DIMENSION_DEFINITIONS)
        .map(|(name, definition)| DimensionHeightDiagnostic {
            name: Arc::from(name.to_string_lossy()),
            dimension: definition.dimension_type.0,
            minimum_y: definition.minimum_y.0,
            height_range: definition.height_range.0,
            generator: dimension_generator_id(definition.generator_type),
        })
        .collect()
}

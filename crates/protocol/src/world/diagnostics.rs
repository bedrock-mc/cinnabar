//! Compact wire facts retained for lighting diagnostics, without changing world behavior.

use valentine::bedrock::version::v1_26_51::{
    EnumsSubChunkPacketPayloadHeightMapDataType as HeightmapType,
    SubChunkPacketPayloadHeightmapDataRenderHeightMapType as RenderHeightmapType,
    SubChunkPacketPayloadSubChunkPacketData,
};

/// Vertical bounds advertised separately from StartGame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimensionHeightDiagnostic {
    pub dimension: i32,
    pub minimum_y: i32,
    pub height_range: i32,
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
    pub(super) fn from_entry(entry: &SubChunkPacketPayloadSubChunkPacketData) -> Self {
        let data = &entry.height_map_data;
        let heightmap_kind = match data.height_map_type {
            HeightmapType::Nodata => 0,
            HeightmapType::Hasdata => 1,
            HeightmapType::Alltoohigh => 2,
            HeightmapType::Alltoolow => 3,
            HeightmapType::Unknown(value) => value,
        };
        let render_kind = match data.render_height_map_type {
            RenderHeightmapType::Nodata => 0,
            RenderHeightmapType::Hasdata => 1,
            RenderHeightmapType::Alltoohigh => 2,
            RenderHeightmapType::Alltoolow => 3,
            RenderHeightmapType::Allcopied => 4,
            RenderHeightmapType::Unknown(value) => value,
        };
        Self {
            payload_present: entry.serialized_sub_chunk.is_some(),
            heightmap: summarize_heightmap(heightmap_kind, data.subchunk_height_map.as_ref()),
            render_heightmap: summarize_heightmap(
                render_kind,
                data.subchunk_render_height_map.as_ref(),
            ),
        }
    }
}

/// Reduces arbitrary well-formed heightmap rows to fixed-size diagnostic facts.
fn summarize_heightmap(kind: u8, rows: Option<&[Vec<i8>; 16]>) -> HeightmapDiagnostic {
    let mut result = HeightmapDiagnostic {
        kind,
        payload_present: rows.is_some(),
        sample_count: 0,
        min: None,
        max: None,
    };
    for value in rows.into_iter().flatten().flatten().copied() {
        result.sample_count += 1;
        result.min = Some(result.min.map_or(value, |previous| previous.min(value)));
        result.max = Some(result.max.map_or(value, |previous| previous.max(value)));
    }
    result
}

//! Immutable registry identities captured before background decoding.

use assets::{NetworkIdMode, ResolvedBiomeTints, RuntimeAssets};
use protocol::DimensionRange;
use std::sync::Arc;
use world::{BiomeIds, BlockIds, DimensionSlots};

/// Session registries that decode workers resolve raw ids against, as the
/// vanilla palettes do: unknown blocks become air and unknown biomes the
/// dimension's fallback biome.
#[derive(Clone)]
pub struct DecodeIds {
    pub assets: Arc<RuntimeAssets>,
    pub custom_blocks: std::ops::Range<u32>,
    pub remap: Arc<assets::SequentialIdRemap>,
    pub(crate) diagnostics: Arc<super::DecodeDiagnostics>,
    pub(crate) session_id: u64,
    pub mode: NetworkIdMode,
    pub air: u32,
    pub biome_tints: Arc<ResolvedBiomeTints>,
    pub default_biome: u32,
}

impl std::fmt::Debug for DecodeIds {
    /// Summarizes registry identity without retaining bulky asset debug output.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DecodeIds")
            .field("mode", &self.mode)
            .field("air", &self.air)
            .field("default_biome", &self.default_biome)
            .finish_non_exhaustive()
    }
}

impl BlockIds for DecodeIds {
    /// Returns the active registry's air value.
    fn air(&self) -> u32 {
        self.air
    }

    /// Maps unknown wire ids to the same fallback used by vanilla decoding.
    fn resolve(&self, network_id: u32) -> u32 {
        let wire_id = network_id;
        let network_id = if self.mode == NetworkIdMode::Sequential {
            self.remap.to_internal(network_id)
        } else {
            network_id
        };
        let known =
            self.assets.is_known(self.mode, network_id) || self.custom_blocks.contains(&network_id);
        self.diagnostics.observe(wire_id, network_id, self, known);
        if known { network_id } else { self.air }
    }
}

impl BiomeIds for DecodeIds {
    /// Returns this dimension's fallback biome.
    fn default_biome(&self) -> u32 {
        self.default_biome
    }

    /// Maps unknown wire ids to the same fallback used by vanilla decoding.
    fn resolve(&self, biome_id: u16) -> u32 {
        let biome_id = u32::from(biome_id);
        if !self.assets.is_diagnostic()
            && self.biome_tints.dense_index(biome_id) == assets::MISSING_BIOME_DENSE_INDEX
        {
            self.default_biome
        } else {
            biome_id
        }
    }
}

/// Converts the admitted dimension range into packed palette slots.
pub fn dimension_slots(range: DimensionRange) -> DimensionSlots {
    DimensionSlots {
        base_sub_chunk_y: range.base_sub_chunk_y,
        count: range.sub_chunk_count,
    }
}

/// Vanilla's fallback biome ids: ocean, hell, and the_end.
pub fn default_biome_id(dimension: i32) -> u32 {
    match dimension {
        1 => 8,
        2 => 9,
        _ => 0,
    }
}

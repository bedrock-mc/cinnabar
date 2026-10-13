use bevy::{prelude::Resource, render::extract_resource::ExtractResource};

/// Maximum section population in the player's current render distance and dimension.
/// The renderer combines this hint with observed payload sizes, within adapter limits.
#[derive(Resource, ExtractResource, Debug, Default, Clone, Copy)]
#[extract_app(bevy::render::RenderApp)]
pub struct ChunkArenaCapacityHint {
    pub sections: usize,
}

impl ChunkArenaCapacityHint {
    /// Counts the square column footprint, including the player's column, at this radius.
    #[must_use]
    pub fn for_view(radius_chunks: usize, sections_per_column: usize) -> Self {
        let side = radius_chunks.saturating_mul(2).saturating_add(1);
        Self {
            sections: side
                .saturating_mul(side)
                .saturating_mul(sections_per_column),
        }
    }
}

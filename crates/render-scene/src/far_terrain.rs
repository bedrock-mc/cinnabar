//! Far-terrain tiles and the near-column mask the far pass draws with.

use bevy::prelude::Resource;
use far_terrain::{NearMask, TileMesh};
use std::sync::Arc;

/// What the far pass draws; replaced whole when anything in it changes.
#[derive(Resource, bevy::render::extract_resource::ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct FarTerrainScene {
    pub tiles: Arc<[Arc<TileMesh>]>,
    /// Columns near terrain draws; far quads over them are hidden.
    pub near: Arc<NearMask>,
}

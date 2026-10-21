//! Binds worker-prepared held models to actor pages and restores startup after disconnect.

use super::*;
use crate::prepared_actor_artwork::PreparedHeldBlockEntities;
use render_model::block_entity::PreparedHeldBlockModels;

/// Builds startup geometry and bindings from the immutable static atlas.
pub(super) fn build(
    atlas: Option<&BlockEntityAtlas>,
    entities: &RuntimeEntityAssets,
    location: Option<ActorArtworkLocation>,
    pages: &mut ActorArtworkPages,
) -> PreparedHeldBlockEntities {
    let source = atlas.map(|atlas| PreparedHeldBlockModels::from_entity_assets(atlas, entities));
    if let Some((source, location)) = source.as_ref().zip(location) {
        pages.register_equipment_artwork(source.identity(), location);
    }
    bindings(source.as_ref(), location)
}

/// Couples cached geometry to an admitted raster location from the same generation.
fn bindings(
    source: Option<&PreparedHeldBlockModels>,
    location: Option<ActorArtworkLocation>,
) -> PreparedHeldBlockEntities {
    let mut prepared = PreparedHeldBlockEntities {
        identity: source.map(PreparedHeldBlockModels::identity),
        ..Default::default()
    };
    if let Some((source, location)) = source.zip(location) {
        for (kind, model) in source.models() {
            prepared.geometries.push(model.geometry.clone());
            prepared
                .models
                .insert(*kind, (model.geometry.id, location, model.glint_span));
        }
    }
    prepared
}

impl EquipmentRuntime {
    /// Packs held static pixels on the worker and shares its already prepared geometry.
    pub(crate) fn prepare_block_entity_artwork(
        pages: &ActorArtworkPages,
        artwork: &render_model::PreparedBlockEntityArtwork,
    ) -> (ActorArtworkPages, PreparedHeldBlockEntities) {
        let source = artwork.held_models();
        if let Some(location) = pages.equipment_artwork_location(source.identity()) {
            return (pages.clone(), bindings(Some(source), Some(location)));
        }
        let (mut pages, location) = source.raster().map_or_else(
            || (pages.clone(), None),
            |raster| {
                let (pages, locations) = pages
                    .clone()
                    .with_equipment_rasters(std::slice::from_ref(raster));
                (pages, locations[0])
            },
        );
        if let Some(location) = location {
            pages.register_equipment_artwork(source.identity(), location);
        }
        (pages, bindings(Some(source), location))
    }

    /// Admits one prepared atlas generation, restoring startup bindings after disconnect.
    pub(crate) fn set_block_entity_artwork(
        &mut self,
        prepared: Option<&PreparedHeldBlockEntities>,
    ) {
        let prepared = prepared.unwrap_or(&self.startup_held_block_entities);
        if self.held_block_entities.identity == prepared.identity
            && self.held_block_entities.models == prepared.models
        {
            return;
        }
        self.held_block_entities = prepared.clone();
        self.pending.extend(prepared.geometries.iter().cloned());
    }
}

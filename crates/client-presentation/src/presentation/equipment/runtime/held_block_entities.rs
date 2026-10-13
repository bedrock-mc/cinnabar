//! Startup admission of cached held block-entity geometry and artwork.

use super::*;
use crate::prepared_actor_artwork::PreparedHeldBlockEntities;

/// Equipment geometry index space reserved for held block-entity item models.
pub(super) const RIG_INDEX_BASE: u32 = 0x00ff_0100;

/// Builds models whose textures are available and binds them to the static atlas raster.
pub(super) fn build(
    atlas: Option<&BlockEntityAtlas>,
    location: Option<ActorArtworkLocation>,
) -> PreparedHeldBlockEntities {
    let mut prepared = PreparedHeldBlockEntities::default();
    let Some((atlas, location)) = atlas.zip(location) else {
        return prepared;
    };
    prepared.identity = Some(atlas.identity());
    for (index, kind) in HeldBlockEntity::all().into_iter().enumerate() {
        let id = equipment_rig_id(RIG_INDEX_BASE + index as u32);
        if let Some(geometry) =
            render_model::block_entity::held_block_entity_geometry(id, atlas, kind)
        {
            prepared.geometries.push(geometry);
            prepared.models.insert(kind, (id, location));
        }
    }
    prepared
}

impl EquipmentRuntime {
    /// Prepares held geometry and shared static pixels on the accepted pack's worker.
    pub(crate) fn prepare_block_entity_artwork(
        pages: &ActorArtworkPages,
        artwork: &render_model::PreparedBlockEntityArtwork,
    ) -> (ActorArtworkPages, PreparedHeldBlockEntities) {
        let atlas = artwork.atlas();
        let raster = EquipmentRaster {
            width: atlas.size()[0] as u16,
            height: atlas.static_height() as u16,
            rgba8: Arc::clone(atlas.static_rgba8()),
        };
        let (pages, locations) = pages.clone().with_equipment_rasters(&[raster]);
        let prepared = build(Some(atlas), locations[0]);
        (pages, prepared)
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

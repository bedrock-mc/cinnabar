//! Startup admission of cached held block-entity geometry and artwork.

use super::*;

/// Equipment geometry index space reserved for held block-entity item models.
pub(super) const RIG_INDEX_BASE: u32 = 0x00ff_0100;

/// Queues models whose textures are available and binds them to the static atlas raster.
pub(super) fn build(
    atlas: Option<&BlockEntityAtlas>,
    location: Option<ActorArtworkLocation>,
    pending: &mut Vec<ActorRigGeometry>,
) -> BTreeMap<HeldBlockEntity, (EntityRigId, ActorArtworkLocation)> {
    let mut models = BTreeMap::new();
    let Some((atlas, location)) = atlas.zip(location) else {
        return models;
    };
    for (index, kind) in HeldBlockEntity::all().into_iter().enumerate() {
        let id = equipment_rig_id(RIG_INDEX_BASE + index as u32);
        if let Some(geometry) =
            render_model::block_entity::held_block_entity_geometry(id, atlas, kind)
        {
            pending.push(geometry);
            models.insert(kind, (id, location));
        }
    }
    models
}

//! Binds worker-prepared held models to current actor pages and restores startup on disconnect.

use super::*;
use render_model::block_entity::PreparedHeldBlockModels;

/// Startup fallback and the currently accepted held-artwork bindings.
pub(super) struct HeldModels {
    startup: Option<PreparedHeldBlockModels>,
    startup_location: Option<ActorArtworkLocation>,
    current: Option<[u8; 32]>,
    pub(super) models: BTreeMap<HeldBlockEntity, (EntityRigId, ActorArtworkLocation, [f32; 2])>,
}

/// Queues startup models and retains them for restoring after a session.
pub(super) fn build(
    atlas: Option<&BlockEntityAtlas>,
    location: Option<ActorArtworkLocation>,
    pending: &mut Vec<ActorRigGeometry>,
) -> HeldModels {
    let startup = atlas.map(PreparedHeldBlockModels::new);
    let current = startup.as_ref().map(PreparedHeldBlockModels::identity);
    let models = startup
        .as_ref()
        .zip(location)
        .map_or_else(BTreeMap::new, |(source, location)| {
            source
                .models()
                .iter()
                .map(|(kind, model)| {
                    pending.push(model.geometry.clone());
                    (*kind, (model.geometry.id, location, model.glint_span))
                })
                .collect()
        });
    HeldModels {
        startup,
        startup_location: location,
        current,
        models,
    }
}

impl EquipmentRuntime {
    /// Detects accepted artwork changes without rebuilding meshes on frames or dynamic text changes.
    pub fn held_artwork_changed(&self, source: Option<&PreparedHeldBlockModels>) -> bool {
        self.held_block_entities.current
            != source
                .or(self.held_block_entities.startup.as_ref())
                .map(PreparedHeldBlockModels::identity)
    }

    /// Binds the accepted static raster into fresh session pages; disconnect restores startup.
    /// Geometry is queued only when the source identity changes, while page locations always rebind.
    pub fn bind_held_artwork(
        &mut self,
        source: Option<&PreparedHeldBlockModels>,
        pages: &ActorArtworkPages,
    ) -> Option<ActorArtworkPages> {
        let held = &mut self.held_block_entities;
        let source = source.or(held.startup.as_ref());
        let identity = source.map(PreparedHeldBlockModels::identity);
        let startup = identity == held.startup.as_ref().map(PreparedHeldBlockModels::identity);
        let (extended, location) = if startup {
            (None, held.startup_location)
        } else if let Some(raster) = source.and_then(PreparedHeldBlockModels::raster) {
            let (extended, locations) = pages.with_equipment_rasters(std::slice::from_ref(raster));
            (Some(extended), locations[0])
        } else {
            (None, None)
        };
        if identity != held.current {
            self.pending.extend(
                source
                    .into_iter()
                    .flat_map(|source| source.models())
                    .map(|(_, model)| model.geometry.clone()),
            );
            held.current = identity;
        }
        held.models = source
            .zip(location)
            .map_or_else(BTreeMap::new, |(source, location)| {
                source
                    .models()
                    .iter()
                    .map(|(kind, model)| (*kind, (model.geometry.id, location, model.glint_span)))
                    .collect()
            });
        extended
    }
}

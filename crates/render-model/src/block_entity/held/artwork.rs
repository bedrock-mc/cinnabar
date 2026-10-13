//! Worker-built static models shared by held items and accepted block-entity artwork.

use super::*;
use crate::EquipmentRaster;

/// Immutable held geometry and the static raster its UVs address.
#[derive(Clone, Debug)]
pub struct PreparedHeldBlockModels {
    identity: [u8; 32],
    raster: Option<EquipmentRaster>,
    models: Arc<[(HeldBlockEntity, HeldBlockEntityModel)]>,
}

impl PreparedHeldBlockModels {
    /// Builds all admitted models once, before their artwork is published to a frame.
    pub fn new(atlas: &BlockEntityAtlas) -> Self {
        let raster = u16::try_from(atlas.size()[0])
            .ok()
            .zip(u16::try_from(atlas.static_height()).ok())
            .filter(|(width, height)| *width > 0 && *height > 0)
            .map(|(width, height)| EquipmentRaster {
                width,
                height,
                rgba8: Arc::clone(atlas.static_rgba8()),
            });
        let models = if raster.is_some() {
            HeldBlockEntity::all()
                .into_iter()
                .enumerate()
                .filter_map(|(index, kind)| {
                    // This range is disjoint from catalog and skull equipment geometry.
                    let id = crate::equipment_rig_id(0x00ff_0100 + index as u32);
                    held_block_entity_model(id, atlas, kind).map(|geometry| (kind, geometry))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        Self {
            identity: atlas.identity(),
            raster,
            models: models.into(),
        }
    }

    /// The atlas identity includes both static placements and their pixels.
    pub fn identity(&self) -> [u8; 32] {
        self.identity
    }

    /// Shares the static pixels addressed by every prepared model.
    pub fn raster(&self) -> Option<&EquipmentRaster> {
        self.raster.as_ref()
    }

    /// Shares models with stable IDs, so an artwork reload replaces existing rig geometry.
    pub fn models(&self) -> &[(HeldBlockEntity, HeldBlockEntityModel)] {
        &self.models
    }
}

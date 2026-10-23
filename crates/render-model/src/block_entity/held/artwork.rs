//! Worker-built static models shared by held items and accepted block-entity artwork.

use super::*;
use crate::EquipmentRaster;
use sha2::{Digest, Sha256};

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
        Self::with_heads(atlas, &HeadModels::default())
    }

    /// Includes the entity catalog's dragon and piglin head geometry in the held cache.
    pub fn from_entity_assets(
        atlas: &BlockEntityAtlas,
        entities: &assets::RuntimeEntityAssets,
    ) -> Self {
        Self::with_heads(atlas, &HeadModels::from_assets(entities))
    }

    /// Retains the exact head geometry selected by the static artwork preparation owner.
    pub(in crate::block_entity) fn with_heads(
        atlas: &BlockEntityAtlas,
        heads: &HeadModels,
    ) -> Self {
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
                    model_with_heads(id, atlas, kind, heads).map(|geometry| (kind, geometry))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut identity = Sha256::new();
        identity.update(atlas.identity());
        for (_, model) in &models {
            identity.update(model.geometry.id.0.to_le_bytes());
            identity.update(bytemuck::cast_slice(&model.geometry.vertices));
        }
        Self {
            identity: identity.finalize().into(),
            raster,
            models: models.into(),
        }
    }

    /// The identity includes static artwork and the prepared geometry using it.
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

//! Shared admission for item geometry and the physical texture array.

use super::{
    DroppedItemModel, ITEM_MESH_VERTEX_BYTES, MAX_ITEM_LAYERS, MIN_ITEM_ATLAS_SIDE,
    atlas_layer_limit,
};
use assets::texture::policy::{MAX_SUBSCRIBER_BYTES, TexturePolicy};

/// Device limits and the space already charged to this frame's distinct models.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemModelBudget {
    policy: TexturePolicy,
    max_array_layers: usize,
    max_vertices: usize,
    side: u32,
    layers: usize,
    vertices: usize,
}

impl Default for ItemModelBudget {
    /// Uses the shared storage policy when no render device is present.
    fn default() -> Self {
        Self::new(
            assets::texture::policy::device_policy(),
            MAX_ITEM_LAYERS,
            u64::MAX,
        )
    }
}

impl ItemModelBudget {
    /// Starts with the reserved white layer and no geometry.
    pub fn new(policy: TexturePolicy, max_array_layers: usize, max_buffer_size: u64) -> Self {
        Self {
            policy,
            max_array_layers,
            max_vertices: (max_buffer_size.min(MAX_SUBSCRIBER_BYTES as u64)
                / ITEM_MESH_VERTEX_BYTES as u64) as usize,
            side: MIN_ITEM_ATLAS_SIDE,
            layers: 1,
            vertices: 0,
        }
    }

    /// Clears usage while preserving the device limits.
    pub fn reset(&mut self) {
        self.side = MIN_ITEM_ATLAS_SIDE;
        self.layers = 1;
        self.vertices = 0;
    }

    /// Returns the maximum complete vertices allowed in the upload buffer.
    pub fn max_vertices(self) -> usize {
        self.max_vertices
    }

    /// Returns the texture policy used by both selection and upload.
    pub fn policy(self) -> TexturePolicy {
        self.policy
    }

    /// Charges one validated mesh atomically, returning its current atlas extent.
    pub fn admit(&mut self, model: &DroppedItemModel, vertices: usize) -> Option<u32> {
        self.admit_dimensions(
            model.atlas_side(self.policy)?,
            model.texture_layers(),
            vertices,
        )
    }

    /// Charges validated cached dimensions with the same limits used for model uploads.
    pub fn admit_dimensions(&mut self, side: u32, layers: usize, vertices: usize) -> Option<u32> {
        let side = self.side.max(side);
        let layers = self.layers.checked_add(layers)?;
        let total = self.vertices.checked_add(vertices)?;
        if vertices == 0
            || total > self.max_vertices
            || layers > atlas_layer_limit(side, self.policy, self.max_array_layers)
        {
            return None;
        }
        self.side = side;
        self.layers = layers;
        self.vertices = total;
        Some(side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DroppedItemSprite;

    /// Makes a solid sprite with original pixels at the requested extent.
    fn sprite(side: u32) -> DroppedItemModel {
        DroppedItemModel::Sprite(DroppedItemSprite {
            width: side,
            height: side,
            rgba8: vec![255; (side * side * 4) as usize].into(),
        })
    }

    #[test]
    fn atlas_growth_rejection_leaves_room_for_a_smaller_fallback() {
        let policy = assets::texture::policy::device_policy();
        let mut budget = ItemModelBudget::new(policy, MAX_ITEM_LAYERS, u64::MAX);
        let small = sprite(16);
        let mesh = small.mesh(MIN_ITEM_ATLAS_SIDE, 1).unwrap();
        for _ in 0..4 {
            assert!(budget.admit(&small, mesh.len()).is_some());
        }
        let hd = sprite(2048);
        let mesh = hd.mesh(2048, 1).unwrap();
        assert!(budget.admit(&hd, mesh.len()).is_none());
        assert!(
            budget
                .admit(&small, small.mesh(MIN_ITEM_ATLAS_SIDE, 1).unwrap().len())
                .is_some()
        );
    }

    #[test]
    fn mesh_buffer_rejection_does_not_charge_texture_layers() {
        let model = sprite(1);
        let mesh = model.mesh(MIN_ITEM_ATLAS_SIDE, 1).unwrap();
        let mut budget = ItemModelBudget::new(
            assets::texture::policy::device_policy(),
            3,
            (mesh.len() * ITEM_MESH_VERTEX_BYTES) as u64,
        );
        assert!(budget.admit(&model, mesh.len() + 1).is_none());
        assert!(budget.admit(&model, mesh.len()).is_some());
        assert!(budget.admit(&model, mesh.len()).is_none());
        budget.reset();
        assert!(budget.admit(&model, mesh.len()).is_some());
    }
}

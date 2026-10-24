//! Validated source costs measured before pixels or geometry are copied.
use super::{DroppedItemModel, MIN_ITEM_ATLAS_SIDE, block::quad_normal};

/// Geometry and retained pixels for one distinct item artwork source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemModelCost {
    pub side: u32,
    pub layers: usize,
    pub pixels: usize,
    pub vertices: usize,
}

impl ItemModelCost {
    /// Normalizes a validated source extent without applying changeable device limits.
    pub fn new(side: u32, layers: usize, pixels: usize, vertices: usize) -> Option<Self> {
        if side == 0 || layers == 0 || vertices == 0 {
            return None;
        }
        Some(Self {
            side: side.checked_next_power_of_two()?.max(MIN_ITEM_ATLAS_SIDE),
            layers,
            pixels,
            vertices,
        })
    }

    /// Measures an existing source without allocating pixel buffers or mesh vertices.
    pub fn from_model(model: &DroppedItemModel) -> Option<Self> {
        let (side, layers, pixels) = match model {
            DroppedItemModel::Vacant => return None,
            DroppedItemModel::Sprite(sprite) | DroppedItemModel::NativeSprite(sprite) => {
                (sprite.width.max(sprite.height), 1, sprite.rgba8.len())
            }
            DroppedItemModel::Cube(cube) => (
                cube.tile,
                cube.faces.len(),
                cube.faces.iter().map(|face| face.len()).sum(),
            ),
            DroppedItemModel::Block(block) => (
                block
                    .materials
                    .iter()
                    .map(|(sprite, _)| sprite.width.max(sprite.height))
                    .max()?,
                block.materials.len(),
                block
                    .materials
                    .iter()
                    .map(|(sprite, _)| sprite.rgba8.len())
                    .sum(),
            ),
        };
        let side = side.checked_next_power_of_two()?.max(MIN_ITEM_ATLAS_SIDE);
        Self::new(side, layers, pixels, model.mesh_vertex_count(side)?)
    }
}

/// Counts a validated block face using the same degenerate-face rule as mesh construction.
pub fn block_quad_vertex_count(quad: &assets::ModelQuad) -> usize {
    if quad_normal(quad).is_some() { 6 } else { 0 }
}

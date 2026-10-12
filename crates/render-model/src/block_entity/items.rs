//! Items that block entities hold in the world (frames, campfires), handed to the dropped-item
//! renderer instead of a second item mesher.

use std::sync::Arc;

use glam::Mat4;

/// One item to draw at a fixed pose.
#[derive(Clone, Debug, PartialEq)]
pub struct StaticItemPlacement {
    pub identifier: Arc<str>,
    pub metadata: u32,
    /// A retained sequential block identity whose dedicated pot-content model is drawn.
    /// `None` selects the item sprite used by frames and campfires.
    pub potted_block: Option<u32>,
    /// Maps the unit item model into the world.
    pub world_from_item: [[f32; 4]; 3],
    /// Fixed light levels; `None` samples the world at the item.
    pub light: Option<(u8, u8)>,
}

/// The frame's placements, replaced every update by the block-entity presentation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StaticItemPlacements(pub Vec<StaticItemPlacement>);

/// The top three rows of `matrix`, the layout the dropped-item renderer reads.
#[must_use]
pub fn matrix_rows(matrix: Mat4) -> [[f32; 4]; 3] {
    let columns = matrix.to_cols_array_2d();
    std::array::from_fn(|row| std::array::from_fn(|column| columns[column][row]))
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    #[test]
    fn rows_reproduce_the_affine_transform() {
        let matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
            * Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0));
        let rows = matrix_rows(matrix);
        assert_eq!(rows[0], [2.0, 0.0, 0.0, 1.0]);
        assert_eq!(rows[1], [0.0, 3.0, 0.0, 2.0]);
        assert_eq!(rows[2], [0.0, 0.0, 4.0, 3.0]);
    }
}

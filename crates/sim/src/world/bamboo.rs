use super::{BlockPhysics, CollisionRegistry, Vec3};
use std::sync::Arc;

impl CollisionRegistry {
    /// Resolves origin-sampled carrier bounds against each bamboo column.
    pub fn set_bamboo_column_offset(&mut self, runtime_id: u32) -> bool {
        let Some(block) = Arc::make_mut(&mut self.blocks).get_mut(&runtime_id) else {
            return false;
        };
        block.bamboo_offset = true;
        true
    }

    /// Translation of a registered block's shapes into its position-dependent world bounds.
    pub fn block_shape_offset(&self, runtime_id: u32, position: [i32; 3]) -> Option<Vec3> {
        self.physics(runtime_id)
            .map(|physics| physics.shape_offset(position))
    }
}

impl BlockPhysics {
    pub(super) fn shape_offset(&self, position: [i32; 3]) -> Vec3 {
        let mut offset = Vec3::new(position[0] as f64, position[1] as f64, position[2] as f64);
        if self.bamboo_offset {
            let column = world::bamboo::column_offset(position);
            for axis in [0, 2] {
                offset[axis] += f64::from(column[axis] - world::bamboo::ORIGIN_OFFSET[axis]);
            }
        }
        offset
    }
}

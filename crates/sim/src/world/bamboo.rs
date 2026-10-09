use super::{BlockPhysics, CollisionRegistry, Vec3};
use std::sync::Arc;

impl CollisionRegistry {
    /// Resolves origin-sampled carrier bounds against each bamboo column.
    pub fn set_bamboo_column_offset(&mut self, runtime_id: u32) -> bool {
        let Some(block) = Arc::make_mut(&mut self.blocks).get_mut(&runtime_id) else {
            return false;
        };
        block.random_offset = Some((world::random_offset::BAMBOO, world::bamboo::ORIGIN_OFFSET));
        true
    }

    /// Installs an admitted component for shapes authored at their undisplaced local origin.
    pub fn set_random_offset(
        &mut self,
        runtime_id: u32,
        component: world::random_offset::RandomOffsetComponent,
    ) -> bool {
        if !component.is_valid() {
            return false;
        }
        let Some(block) = Arc::make_mut(&mut self.blocks).get_mut(&runtime_id) else {
            return false;
        };
        block.random_offset = Some((component, [0.0; 3]));
        for shape in block
            .shapes
            .iter()
            .chain(block.pick_shapes.iter().flatten())
        {
            for (axis, halo) in self.collision_halo.iter_mut().enumerate() {
                if shape.max[axis] + f64::from(component.axes[axis].range[1]) > 1.0 {
                    halo.0 = -1;
                }
                if shape.min[axis] + f64::from(component.axes[axis].range[0]) < 0.0 {
                    halo.1 = 1;
                }
            }
        }
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
        if let Some((component, origin)) = self.random_offset {
            let column = component.offset(position);
            for axis in 0..3 {
                offset[axis] += f64::from(column[axis] - origin[axis]);
            }
        }
        offset
    }
}

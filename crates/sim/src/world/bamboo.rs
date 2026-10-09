use super::{BlockPhysics, CollisionRegistry, Vec3};
use std::sync::Arc;

impl CollisionRegistry {
    /// Resolves origin-sampled carrier bounds against each bamboo column.
    pub fn set_bamboo_column_offset(&mut self, runtime_id: u32) -> bool {
        let Some(block) = Arc::make_mut(&mut self.blocks).get_mut(&runtime_id) else {
            return false;
        };
        block.random_offset = Some((
            block_transform::random_offset::BAMBOO,
            block_transform::bamboo::ORIGIN_OFFSET,
        ));
        true
    }

    /// Installs an admitted component for shapes authored at their undisplaced local origin.
    pub fn set_random_offset(
        &mut self,
        runtime_id: u32,
        component: block_transform::random_offset::RandomOffsetComponent,
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
                let min = shape.min[axis] + f64::from(component.axes[axis].range[0]);
                let max = shape.max[axis] + f64::from(component.axes[axis].range[1]);
                halo.0 = halo.0.min(1 - max.ceil() as i32);
                halo.1 = halo.1.max(-(min.floor() as i32));
            }
        }
        true
    }

    /// Translation of a registered block's shapes into its position-dependent world bounds.
    pub fn block_shape_offset(&self, runtime_id: u32, position: [i32; 3]) -> Option<Vec3> {
        self.physics(runtime_id)
            .map(|physics| physics.shape_offset(position))
    }

    /// Includes every owner cell whose admitted translated shapes can intersect a query.
    pub(super) fn query_bounds(
        &self,
        query: super::Aabb,
    ) -> Result<[[i32; 3]; 2], super::WorldQueryError> {
        let min: [f64; 3] = std::array::from_fn(|axis| self.collision_halo[axis].0.min(-1) as f64);
        let max: [f64; 3] = std::array::from_fn(|axis| self.collision_halo[axis].1.max(1) as f64);
        Ok([
            super::block_floor(query.min + Vec3::new(min[0], min[1], min[2]))?,
            super::block_ceil(query.max + Vec3::new(max[0], max[1], max[2]))?,
        ])
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

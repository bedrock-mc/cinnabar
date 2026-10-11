//! Owner-cell reach for compiled interaction geometry, independent of movement.

use super::super::CollisionRegistry;
use assets::block_shapes::{CompiledBlockShapes, PISTON_DIRECTIONS, SelectionContext};

/// Accumulates admitted selection bounds and possible carried-actor translations.
#[derive(Debug, Clone, Default)]
pub(in crate::world) struct SelectionHalo {
    selection: [(i32, i32); 3],
    actor_visual: [(i32, i32); 3],
    moving: bool,
}

impl SelectionHalo {
    /// Includes compiled base/alternate bounds and the full validated piston progress range.
    pub(super) fn include(&mut self, shapes: &CompiledBlockShapes) {
        let direction = if shapes.selection_context == SelectionContext::PistonArm {
            shapes
                .actor_facing
                .map(|facing| PISTON_DIRECTIONS[usize::from(facing)])
        } else {
            None
        };
        for shape in shapes
            .selection
            .iter()
            .chain(shapes.contextual_selection.iter())
        {
            let min = std::array::from_fn(|axis| {
                shape.min[axis] + direction.map_or(0.0, |d| f64::from(-d[axis]).min(0.0))
            });
            let max = std::array::from_fn(|axis| {
                shape.max[axis] + direction.map_or(0.0, |d| f64::from(-d[axis]).max(0.0))
            });
            include_bounds(&mut self.selection, min, max);
        }
        if let Some(shape) = shapes.actor_visual {
            include_bounds(
                &mut self.actor_visual,
                shape.min.map(|v| v - 1.0),
                shape.max.map(|v| v + 1.0),
            );
        }
        self.moving |= shapes.selection_context == SelectionContext::MovingBlock;
    }

    /// Unions selection owner reach with movement and any registered moving-actor reach.
    pub(in crate::world) fn combined(&self, collision: [(i32, i32); 3]) -> [(i32, i32); 3] {
        std::array::from_fn(|axis| {
            let mut range = (
                collision[axis].0.min(self.selection[axis].0),
                collision[axis].1.max(self.selection[axis].1),
            );
            if self.moving {
                range.0 = range.0.min(self.actor_visual[axis].0);
                range.1 = range.1.max(self.actor_visual[axis].1);
            }
            range
        })
    }
}

/// Rounds local extents outward to all owner cells that can overlap a traversed cell.
fn include_bounds(halo: &mut [(i32, i32); 3], min: [f64; 3], max: [f64; 3]) {
    for axis in 0..3 {
        halo[axis].0 = halo[axis].0.min(1 - max[axis].ceil() as i32);
        halo[axis].1 = halo[axis].1.max(-(min[axis].floor() as i32));
    }
}

impl CollisionRegistry {
    /// Covers every owner whose movement or admitted selection geometry can meet a ray cell.
    pub(in crate::world) fn selection_ray_halo(&self) -> [(i32, i32); 3] {
        self.selection_halo.combined(self.collision_halo)
    }
}

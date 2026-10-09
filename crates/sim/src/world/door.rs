use std::{borrow::Cow, sync::Arc};

use super::{Aabb, BlockPhysics, CollisionRegistry, PaletteWorld, Vec3, WorldQueryError};

/// Registry cardinal direction, in the native door direction order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorFacing {
    South,
    West,
    North,
    East,
}

/// Door properties retained from the block registry for paired-half collision queries.
#[derive(Debug, Clone)]
pub struct DoorState {
    pub family: Arc<str>,
    pub facing: DoorFacing,
    pub upper: bool,
    pub open: bool,
    pub hinge_right: bool,
}

impl CollisionRegistry {
    /// Attaches paired-door behavior to an existing registered runtime ID.
    pub fn set_door_state(&mut self, runtime_id: u32, state: DoorState) -> bool {
        let Some(block) = Arc::make_mut(&mut self.blocks).get_mut(&runtime_id) else {
            return false;
        };
        block.door = Some(state);
        true
    }
}

impl PaletteWorld<'_> {
    /// Resolves block-local collision shapes and position offsets without mutating the world.
    /// Proposed primary cells are visible to paired lookups.
    pub fn collision_shapes_with_updates(
        &self,
        position: [i32; 3],
        runtime_id: u32,
        updates: &[([i32; 3], u32)],
    ) -> Result<Cow<'_, [Aabb]>, WorldQueryError> {
        let physics =
            self.registry
                .physics(runtime_id)
                .ok_or(WorldQueryError::UnknownRuntimeId {
                    runtime_id,
                    block: position,
                })?;
        let shapes: Cow<'_, [Aabb]> =
            match self.resolved_door_shape_with_updates(position, physics, updates)? {
                Some(shape) => Cow::Owned(vec![shape]),
                None => Cow::Borrowed(&physics.shapes),
            };
        let cell_offset = Vec3::new(position[0] as f64, position[1] as f64, position[2] as f64);
        let offset = physics.shape_offset(position) - cell_offset;
        if offset == Vec3::ZERO {
            Ok(shapes)
        } else {
            Ok(Cow::Owned(
                shapes
                    .iter()
                    .map(|shape| shape.translated(offset))
                    .collect(),
            ))
        }
    }

    /// Resolves a door's lower facing/open state and upper hinge at query time.
    pub(super) fn block_collision_shapes<'a>(
        &self,
        position: [i32; 3],
        physics: &'a BlockPhysics,
        query: Aabb,
    ) -> Result<Cow<'a, [Aabb]>, WorldQueryError> {
        let Some(_) = &physics.door else {
            return Ok(Cow::Borrowed(&physics.shapes));
        };
        let offset = Vec3::new(position[0] as f64, position[1] as f64, position[2] as f64);
        if !Aabb::new(Vec3::ZERO, Vec3::ONE)
            .translated(offset)
            .intersects(query)
        {
            return Ok(Cow::Borrowed(&[]));
        }
        Ok(Cow::Owned(vec![
            self.resolved_door_shape(position, physics)?
                .expect("door physics has one resolved shape"),
        ]))
    }

    /// Resolves the paired door plane on the stack for frequent visibility queries.
    pub(super) fn resolved_door_shape(
        &self,
        position: [i32; 3],
        physics: &BlockPhysics,
    ) -> Result<Option<Aabb>, WorldQueryError> {
        self.resolved_door_shape_with_updates(position, physics, &[])
    }

    /// Uses proposed paired halves when present, otherwise reads the committed world.
    fn resolved_door_shape_with_updates(
        &self,
        position: [i32; 3],
        physics: &BlockPhysics,
        updates: &[([i32; 3], u32)],
    ) -> Result<Option<Aabb>, WorldQueryError> {
        let Some(door) = &physics.door else {
            return Ok(None);
        };
        let mut neighbor = position;
        neighbor[1] = neighbor[1]
            .checked_add(if door.upper { -1 } else { 1 })
            .ok_or(WorldQueryError::CoordinateOutOfRange)?;
        let runtime_id = match updates.iter().find(|(cell, _)| *cell == neighbor) {
            Some((_, runtime_id)) => *runtime_id,
            None => self.primary_runtime_id(neighbor)?,
        };
        let paired = self
            .registry
            .physics(runtime_id)
            .and_then(|block| block.door.as_ref())
            .filter(|other| other.family == door.family);
        let (facing, open, hinge) = paired.map_or((DoorFacing::South, false, false), |other| {
            let (lower, upper) = if door.upper {
                (other, door)
            } else {
                (door, other)
            };
            (lower.facing, lower.open, upper.hinge_right)
        });
        Ok(Some(door_box(facing, open, hinge)))
    }
}

/// Builds the current native door plane after resolving its paired state.
fn door_box(facing: DoorFacing, open: bool, hinge_right: bool) -> Aabb {
    // Vanilla door planes use thickness 0.1825f.
    let direction = facing as usize;
    let blocked = if open {
        (direction + usize::from(hinge_right) * 2) & 3
    } else {
        (direction + 3) & 3
    };
    let thickness = f64::from(0.1825_f32);
    let far = f64::from(1.0_f32 - thickness as f32);
    let mut min = Vec3::ZERO;
    let mut max = Vec3::ONE;
    match blocked {
        0 => max.z = thickness,
        1 => min.x = far,
        2 => min.z = far,
        _ => max.x = thickness,
    }
    Aabb::new(min, max)
}

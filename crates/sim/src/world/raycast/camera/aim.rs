use super::{CameraBlockHit, TraversalState, ray_box, validate_ray};
use crate::{Aabb, BlockPhysicsFlags, PaletteWorld, Vec3, WorldQueryError};
use world::SubChunkKey;

impl PaletteWorld<'_> {
    /// Aim sampling clips cell cubes; visibility clips outlines. Both retain the actual outline.
    pub fn camera_aim_ray(
        &self,
        origin: Vec3,
        end: Vec3,
        max_steps: u32,
        outlines: bool,
        liquids: bool,
    ) -> Result<Option<CameraBlockHit>, WorldQueryError> {
        let delta = end - origin;
        let distance = delta.length_squared().sqrt();
        let direction = validate_ray(origin, delta, distance)?;
        let mut state = TraversalState::new(origin, direction)?;
        if let Some(hit) = self.camera_aim_cell(
            state.cell, origin, direction, distance, true, false, liquids,
        )? {
            return Ok(Some(hit));
        }
        let end_cell = [
            end.x.floor() as i32,
            end.y.floor() as i32,
            end.z.floor() as i32,
        ];
        for _ in 0..max_steps {
            if state.cell == end_cell {
                break;
            }
            let axis = if state.next[0] < state.next[1] && state.next[0] < state.next[2] {
                0
            } else if state.next[1] < state.next[2] {
                1
            } else {
                2
            };
            if state.next[axis] > distance {
                break;
            }
            state.cell[axis] = state.cell[axis]
                .checked_add(state.step[axis])
                .ok_or(WorldQueryError::CoordinateOutOfRange)?;
            state.next[axis] += state.delta[axis];
            if let Some(hit) = self.camera_aim_cell(
                state.cell, origin, direction, distance, false, outlines, liquids,
            )? {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }

    /// Liquid targeting is disabled when the cached eye occupies a primary liquid block.
    pub fn camera_eye_in_liquid(&self, eye: Vec3) -> Result<bool, WorldQueryError> {
        validate_ray(eye, Vec3::ONE, 1.0)?;
        let block = [
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        ];
        let id = self.camera_aim_id(block, 0)?;
        let physics = self
            .registry
            .physics(id)
            .ok_or(WorldQueryError::UnknownRuntimeId {
                runtime_id: id,
                block,
            })?;
        Ok(is_liquid(physics.flags))
    }

    /// A borrowed palette lookup keeps candidate and visibility scans allocation-free.
    fn camera_aim_id(&self, block: [i32; 3], layer: usize) -> Result<u32, WorldQueryError> {
        let key = SubChunkKey::new(self.dimension, block[0] >> 4, block[1] >> 4, block[2] >> 4);
        if !self.store.is_sub_chunk_loaded(key) {
            return Err(WorldQueryError::UnloadedChunk(key.chunk()));
        }
        Ok(self
            .store
            .sub_chunk(key)
            .and_then(|chunk| {
                chunk.runtime_id(
                    layer,
                    block[0].rem_euclid(16) as u8,
                    block[1].rem_euclid(16) as u8,
                    block[2].rem_euclid(16) as u8,
                )
            })
            .unwrap_or(self.registry.air_runtime_id))
    }

    /// Extra liquid layers win after the origin cell; the initial cell only tests primary outline.
    #[allow(clippy::too_many_arguments)]
    fn camera_aim_cell(
        &self,
        block: [i32; 3],
        origin: Vec3,
        direction: Vec3,
        distance: f64,
        initial: bool,
        outlines: bool,
        liquids: bool,
    ) -> Result<Option<CameraBlockHit>, WorldQueryError> {
        let extra = if liquids && !initial {
            self.camera_aim_id(block, 1)?
        } else {
            self.registry.air_runtime_id
        };
        let use_extra = extra != self.registry.air_runtime_id;
        let id = if use_extra {
            extra
        } else {
            self.camera_aim_id(block, 0)?
        };
        if id == self.registry.air_runtime_id {
            return Ok(None);
        }
        let physics = self
            .registry
            .physics(id)
            .ok_or(WorldQueryError::UnknownRuntimeId {
                runtime_id: id,
                block,
            })?;
        let liquid = is_liquid(physics.flags);
        if liquid && !liquids {
            return Ok(None);
        }
        let offset = Vec3::new(
            f64::from(block[0]),
            f64::from(block[1]),
            f64::from(block[2]),
        );
        let cube = Aabb::new(offset, offset + Vec3::ONE);
        let door = if physics.pick_shapes.is_none() {
            self.resolved_door_shape(block, physics)?
        } else {
            None
        };
        let shapes = physics.pick_shapes.as_deref().unwrap_or_else(|| {
            if door.is_some() {
                door.as_slice()
            } else {
                &physics.shapes
            }
        });
        let outline = if liquid {
            cube
        } else {
            let Some((&first, rest)) = shapes.split_first() else {
                return Ok(None);
            };
            let mut outline = first.translated(offset);
            for shape in rest {
                let shape = shape.translated(offset);
                for axis in 0..3 {
                    outline.min[axis] = outline.min[axis].min(shape.min[axis]);
                    outline.max[axis] = outline.max[axis].max(shape.max[axis]);
                }
            }
            outline
        };
        if initial
            && (0..3)
                .all(|axis| origin[axis] >= outline.min[axis] && origin[axis] <= outline.max[axis])
        {
            return Ok(None);
        }
        let Some((distance, face, point)) = ray_box(
            origin,
            direction,
            distance,
            if initial || outlines { outline } else { cube },
        ) else {
            return Ok(None);
        };
        Ok(Some(CameraBlockHit {
            block_pos: block,
            face,
            hit_local: point - offset,
            runtime_id: id,
            distance,
            selection_bounds: outline,
            targetable: initial
                || use_extra
                || !liquid
                || physics
                    .flow
                    .is_some_and(|flow| flow.liquid_depth == Some(0)),
        }))
    }
}

/// Both liquid materials use a full-block outline independently of flow height.
fn is_liquid(flags: BlockPhysicsFlags) -> bool {
    flags.contains(BlockPhysicsFlags::WATER) || flags.contains(BlockPhysicsFlags::LAVA)
}

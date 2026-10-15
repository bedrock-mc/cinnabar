use super::super::{super::section::SectionCache, crossing_order};
use super::{CameraBlockHit, TraversalState, checked_offset, ray_box, validate_ray};
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
        let origin = origin.rounded();
        let end = end.rounded();
        let direction = end - origin;
        let length = f64::from((direction.length_squared() as f32).sqrt());
        validate_ray(origin, direction, length)?;
        // Preserve the supplied endpoint instead of rebuilding it from a normalized ray.
        let mut state = TraversalState::new(origin, direction, 1.0)?;
        let mut section = SectionCache::default();
        if let Some(hit) =
            self.camera_aim_halo(&state, origin, direction, outlines, liquids, &mut section)?
        {
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
            if state.next[axis] > 1.0 {
                break;
            }
            state.advance_axis(axis)?;
            if let Some(hit) =
                self.camera_aim_halo(&state, origin, direction, outlines, liquids, &mut section)?
            {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }

    /// Outline visibility includes displaced owners; cube sampling keeps its cell budget.
    fn camera_aim_halo(
        &self,
        state: &TraversalState,
        origin: Vec3,
        direction: Vec3,
        outlines: bool,
        liquids: bool,
        section: &mut SectionCache,
    ) -> Result<Option<CameraBlockHit>, WorldQueryError> {
        if !outlines {
            return self
                .camera_aim_cell(state.cell, origin, direction, 1.0, false, liquids, section);
        }
        let halo = self.registry.selection_ray_halo();
        // Only intercepts in this traversed cell can finish the step-bounded scan.
        let max_fraction = state.next_crossing().min(1.0);
        let mut best: Option<CameraBlockHit> = None;
        for x in halo[0].0..=halo[0].1 {
            for y in halo[1].0..=halo[1].1 {
                for z in halo[2].0..=halo[2].1 {
                    let block = checked_offset(state.cell, [x, y, z])?;
                    let Some(hit) = self.camera_aim_cell(
                        block,
                        origin,
                        direction,
                        max_fraction,
                        true,
                        liquids,
                        section,
                    )?
                    else {
                        continue;
                    };
                    if best.as_ref().is_none_or(|old| {
                        crossing_order(hit.distance, old.distance)
                            .then_with(|| hit.block_pos.cmp(&old.block_pos))
                            .then_with(|| hit.runtime_id.cmp(&old.runtime_id))
                            .then_with(|| hit.face.cmp(&old.face))
                            .is_lt()
                    }) {
                        best = Some(hit);
                    }
                }
            }
        }
        Ok(best)
    }

    /// Liquid targeting is disabled when the cached eye occupies a primary liquid block.
    pub fn camera_eye_in_liquid(&self, eye: Vec3) -> Result<bool, WorldQueryError> {
        let eye = eye.rounded();
        validate_ray(eye, Vec3::ONE, 1.0)?;
        let block = [
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        ];
        let id = self.camera_aim_id(block, 0, &mut SectionCache::default())?;
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
    fn camera_aim_id(
        &self,
        block: [i32; 3],
        layer: usize,
        section: &mut SectionCache,
    ) -> Result<u32, WorldQueryError> {
        let key = SubChunkKey::new(self.dimension, block[0] >> 4, block[1] >> 4, block[2] >> 4);
        let Some(loaded) = section.get(self, key) else {
            return Err(WorldQueryError::UnloadedChunk(key.chunk()));
        };
        Ok(loaded
            .as_ref()
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
        max_fraction: f32,
        outlines: bool,
        liquids: bool,
        section: &mut SectionCache,
    ) -> Result<Option<CameraBlockHit>, WorldQueryError> {
        let origin_cell = [
            origin.x.floor() as i32,
            origin.y.floor() as i32,
            origin.z.floor() as i32,
        ];
        let initial = block == origin_cell;
        let extra = if liquids && !initial {
            self.camera_aim_id(block, 1, section)?
        } else {
            self.registry.air_runtime_id
        };
        let use_extra = extra != self.registry.air_runtime_id;
        let id = if use_extra {
            extra
        } else {
            self.camera_aim_id(block, 0, section)?
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
        let context = assets::block_shapes::PickContext {
            liquids,
            ..self.pick_context
        };
        let admitted = match physics.pick_rule {
            assets::block_shapes::PickRule::Liquid
            | assets::block_shapes::PickRule::LiquidSource => liquids,
            rule => rule.allows(context, physics.flow.and_then(|flow| flow.liquid_depth)),
        };
        if !admitted {
            return Ok(None);
        }
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
        let outline = if liquid {
            cube
        } else {
            let Some(outline) = self.block_outline_from_physics(block, physics)? else {
                return Ok(None);
            };
            outline
        };
        if (initial || outlines)
            && (0..3)
                .all(|axis| origin[axis] >= outline.min[axis] && origin[axis] <= outline.max[axis])
        {
            return Ok(None);
        }
        let Some((fraction, distance, face, point)) = ray_box(
            origin,
            direction,
            1.0,
            if initial || outlines { outline } else { cube },
        ) else {
            return Ok(None);
        };
        if crossing_order(fraction, max_fraction).is_gt() {
            return Ok(None);
        }
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

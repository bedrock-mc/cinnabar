//! Borrowed visibility queries share interaction geometry without allocating hit provenance.

use super::{
    super::section::SectionCache, Candidate, TraversalState, candidate_precedes, checked_offset,
    crossing_order, ray_box, ray_union_box, validate_ray,
};
use crate::{Aabb, PaletteWorld, Vec3, WorldQueryError};
use std::borrow::Cow;
use world::SubChunkKey;

mod aim;

/// A rendered-camera intercept; interaction authority must still use a revision-bearing query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraBlockHit {
    pub block_pos: [i32; 3],
    pub face: u8,
    pub hit_local: Vec3,
    pub runtime_id: u32,
    pub distance: f64,
    /// Absolute bounds of the intercepted selection shape.
    pub selection_bounds: Aabb,
    /// Flowing primary liquids obstruct aim rays but cannot become targets.
    pub targetable: bool,
}

impl PaletteWorld<'_> {
    /// Finds a selection-shape intercept with no heap allocation or retained chunk revisions.
    pub fn camera_visibility_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
    ) -> Result<Option<CameraBlockHit>, WorldQueryError> {
        let origin = origin.rounded();
        let max_distance = f64::from(max_distance as f32);
        let direction = validate_ray(origin, direction, max_distance)?;
        let mut state = TraversalState::new(origin, direction, max_distance)?;
        let mut best = None;
        let mut bounds = Aabb::new(Vec3::ZERO, Vec3::ZERO);
        let mut section = SectionCache::default();
        loop {
            self.inspect_camera_halo(
                state.cell,
                origin,
                direction,
                max_distance,
                &mut best,
                &mut bounds,
                &mut section,
            )?;
            let next = state.next_crossing();
            if best
                .as_ref()
                .is_some_and(|hit: &Candidate| crossing_order(hit.fraction, next).is_lt())
                || next > 1.0
            {
                break;
            }
            for cell in state.advance(next)? {
                self.inspect_camera_halo(
                    cell,
                    origin,
                    direction,
                    max_distance,
                    &mut best,
                    &mut bounds,
                    &mut section,
                )?;
            }
        }
        Ok(best.map(|hit| CameraBlockHit {
            block_pos: hit.block_pos,
            face: hit.face,
            hit_local: hit.hit_local,
            runtime_id: hit.runtime_id,
            distance: hit.distance,
            selection_bounds: bounds,
            targetable: true,
        }))
    }

    /// Inspects borrowed palette layers, including selection boxes extending into adjacent cells.
    #[allow(clippy::too_many_arguments)]
    fn inspect_camera_halo(
        &self,
        cell: [i32; 3],
        origin: Vec3,
        direction: Vec3,
        distance: f64,
        best: &mut Option<Candidate>,
        bounds: &mut Aabb,
        section: &mut SectionCache,
    ) -> Result<(), WorldQueryError> {
        let halo = self.registry.selection_ray_halo();
        for x in halo[0].0..=halo[0].1 {
            for y in halo[1].0..=halo[1].1 {
                for z in halo[2].0..=halo[2].1 {
                    let block = checked_offset(cell, [x, y, z])?;
                    let key = SubChunkKey::new(
                        self.dimension,
                        block[0] >> 4,
                        block[1] >> 4,
                        block[2] >> 4,
                    );
                    let Some(loaded) = section.get(self, key) else {
                        return Err(WorldQueryError::UnloadedChunk(key.chunk()));
                    };
                    let Some(chunk) = loaded else {
                        continue;
                    };
                    for layer in 0..chunk.storages().len() {
                        let id = chunk
                            .runtime_id(
                                layer,
                                block[0].rem_euclid(16) as u8,
                                block[1].rem_euclid(16) as u8,
                                block[2].rem_euclid(16) as u8,
                            )
                            .expect("validated palette storage resolves every local coordinate");
                        self.inspect_camera_block(
                            block, id, origin, direction, distance, best, bounds,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Tests registry shapes and the current paired-door plane using stack storage.
    #[allow(clippy::too_many_arguments)]
    fn inspect_camera_block(
        &self,
        block: [i32; 3],
        runtime_id: u32,
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
        best: &mut Option<Candidate>,
        bounds: &mut Aabb,
    ) -> Result<(), WorldQueryError> {
        let physics = self
            .registry
            .physics(runtime_id)
            .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })?;
        if !self.may_pick(physics) {
            return Ok(());
        }
        let door = if physics.pick_shapes.is_none() {
            self.resolved_door_shape(block, physics)?
        } else {
            None
        };
        let shapes = if physics.pick_shapes.is_some() {
            self.selection_shapes_at(block, physics)?
        } else {
            Cow::Borrowed(
                door.as_slice()
                    .first()
                    .map_or(&*physics.shapes, |_| door.as_slice()),
            )
        };
        let offset = Vec3::new(
            f64::from(block[0]),
            f64::from(block[1]),
            f64::from(block[2]),
        );
        for (shape_index, &shape) in shapes.iter().enumerate() {
            if shape.min.x == shape.max.x
                || shape.min.y == shape.max.y
                || shape.min.z == shape.max.z
            {
                continue;
            }
            let shape = shape.translated(physics.shape_offset(block));
            let Some((fraction, distance, face, point)) = ray_union_box(
                origin,
                direction,
                max_distance,
                shape,
                &shapes,
                physics.shape_offset(block),
            ) else {
                continue;
            };
            let local = point - offset;
            let candidate = Candidate {
                block_pos: block,
                runtime_id,
                shape_index,
                distance,
                fraction,
                face,
                hit_local: Vec3::new(
                    local.x.clamp(0.0, 1.0),
                    local.y.clamp(0.0, 1.0),
                    local.z.clamp(0.0, 1.0),
                ),
                hit_world: point,
            };
            if best
                .as_ref()
                .is_none_or(|previous| candidate_precedes(&candidate, previous))
            {
                *best = Some(candidate);
                *bounds = shape;
            }
        }
        Ok(())
    }
}

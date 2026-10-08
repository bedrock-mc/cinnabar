//! Borrowed visibility queries share interaction geometry without allocating hit provenance.

use super::{
    Candidate, TraversalState, candidate_precedes, checked_offset, ray_box, strictly_precedes,
    validate_ray,
};
use crate::{Aabb, PaletteWorld, Vec3, WorldQueryError};
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
        let direction = validate_ray(origin, direction, max_distance)?;
        let mut state = TraversalState::new(origin, direction)?;
        let mut best = None;
        let mut bounds = Aabb::new(Vec3::ZERO, Vec3::ZERO);
        loop {
            self.inspect_camera_halo(
                state.cell,
                origin,
                direction,
                max_distance,
                &mut best,
                &mut bounds,
            )?;
            let next = state.next_crossing();
            if best
                .as_ref()
                .is_some_and(|hit: &Candidate| strictly_precedes(hit.distance, next))
                || next > max_distance
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
    fn inspect_camera_halo(
        &self,
        cell: [i32; 3],
        origin: Vec3,
        direction: Vec3,
        distance: f64,
        best: &mut Option<Candidate>,
        bounds: &mut Aabb,
    ) -> Result<(), WorldQueryError> {
        let halo = self.registry.collision_halo;
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
                    if !self.store.is_sub_chunk_loaded(key) {
                        return Err(WorldQueryError::UnloadedChunk(key.chunk()));
                    }
                    let Some(chunk) = self.store.sub_chunk(key) else {
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
        let door = if physics.pick_shapes.is_none() {
            self.resolved_door_shape(block, physics)?
        } else {
            None
        };
        let shapes = physics.pick_shapes.as_deref().unwrap_or_else(|| {
            door.as_slice()
                .first()
                .map_or(&*physics.shapes, |_| door.as_slice())
        });
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
            let shape = shape.translated(offset);
            let Some((distance, face, point)) = ray_box(origin, direction, max_distance, shape)
            else {
                continue;
            };
            let candidate = Candidate {
                block_pos: block,
                runtime_id,
                shape_index,
                distance,
                face,
                hit_local: Vec3::new(
                    (point.x - offset.x).clamp(0.0, 1.0),
                    (point.y - offset.y).clamp(0.0, 1.0),
                    (point.z - offset.z).clamp(0.0, 1.0),
                ),
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

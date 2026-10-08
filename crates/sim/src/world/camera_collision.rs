//! Camera collision scans borrow palette layers and keep lenient skip counts.

use super::{
    Aabb, BlockPhysics, LenientSkipCounts, PaletteWorld, SubChunkKey, Vec3, WorldQueryError,
    block_ceil, block_floor,
    raycast::{TraversalState, checked_offset, strictly_precedes},
    validate_collision_query,
};

impl PaletteWorld<'_> {
    /// Emits known solids in cell order without constructing identities or shape vectors.
    pub(super) fn visit_camera_colliders(
        &self,
        query: Aabb,
        visitor: &mut dyn FnMut(Aabb),
    ) -> Result<LenientSkipCounts, WorldQueryError> {
        validate_collision_query(query)?;
        let mut skipped = LenientSkipCounts::default();
        if query.min == query.max {
            return Ok(skipped);
        }
        let grown = query.grown(1.0);
        let min = block_floor(grown.min)?;
        let max = block_ceil(grown.max)?;
        for x in min[0]..=max[0] {
            for z in min[2]..=max[2] {
                for y in min[1]..=max[1] {
                    self.visit_camera_block(
                        [x, y, z],
                        &mut skipped,
                        &|shape| shape.intersects(query),
                        &mut |shape| visitor(shape),
                    )?;
                }
            }
        }
        Ok(skipped)
    }

    /// Walks the cells along one segment (plus the registry halo) and stops past its first hit.
    pub(super) fn camera_segment_entry_lenient(
        &self,
        origin: Vec3,
        delta: Vec3,
    ) -> Result<(Option<f64>, LenientSkipCounts), WorldQueryError> {
        let end = origin + delta;
        validate_collision_query(Aabb::new(
            origin.component_min(end),
            origin.component_max(end),
        ))?;
        let mut state = TraversalState::new(origin, delta)?;
        let mut skipped = LenientSkipCounts::default();
        let mut best: Option<f64> = None;
        loop {
            self.inspect_camera_segment_halo(state.cell, origin, delta, &mut best, &mut skipped)?;
            let next = state.next_crossing();
            if next > 1.0 || best.is_some_and(|hit| strictly_precedes(hit, next)) {
                return Ok((best, skipped));
            }
            for cell in state.advance(next)? {
                self.inspect_camera_segment_halo(cell, origin, delta, &mut best, &mut skipped)?;
            }
        }
    }

    fn inspect_camera_segment_halo(
        &self,
        cell: [i32; 3],
        origin: Vec3,
        delta: Vec3,
        best: &mut Option<f64>,
        skipped: &mut LenientSkipCounts,
    ) -> Result<(), WorldQueryError> {
        let halo = self.registry.collision_halo;
        for x in halo[0].0..=halo[0].1 {
            for y in halo[1].0..=halo[1].1 {
                for z in halo[2].0..=halo[2].1 {
                    self.visit_camera_block(
                        checked_offset(cell, [x, y, z])?,
                        skipped,
                        &|shape| shape.segment_entry(origin, delta).is_some(),
                        &mut |shape| {
                            if let Some(hit) = shape.segment_entry(origin, delta) {
                                *best = Some(best.map_or(hit, |previous| previous.min(hit)));
                            }
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Emits one cell's camera shapes that `touches` accepts, tallying unreadable cells.
    fn visit_camera_block(
        &self,
        block: [i32; 3],
        skipped: &mut LenientSkipCounts,
        touches: &impl Fn(Aabb) -> bool,
        visitor: &mut impl FnMut(Aabb),
    ) -> Result<(), WorldQueryError> {
        let [x, y, z] = block;
        let key = SubChunkKey::new(self.dimension, x >> 4, y >> 4, z >> 4);
        if !self.store.is_sub_chunk_loaded(key) {
            skipped.unloaded_chunk = skipped.unloaded_chunk.saturating_add(1);
            return Ok(());
        }
        let chunk = self.store.sub_chunk(key);
        let layers = chunk
            .as_ref()
            .map_or(1, |chunk| chunk.storages().len().max(1));
        for layer in 0..layers {
            let id = chunk
                .as_ref()
                .and_then(|chunk| {
                    chunk.runtime_id(
                        layer,
                        x.rem_euclid(16) as u8,
                        y.rem_euclid(16) as u8,
                        z.rem_euclid(16) as u8,
                    )
                })
                .unwrap_or(self.registry.air_runtime_id);
            let Some(physics) = self.registry.physics(id) else {
                skipped.unknown_runtime_id = skipped.unknown_runtime_id.saturating_add(1);
                continue;
            };
            match self.visit_camera_cell(block, physics, touches, visitor) {
                Ok(()) => {}
                Err(WorldQueryError::UnloadedChunk(_)) => {
                    skipped.unloaded_chunk = skipped.unloaded_chunk.saturating_add(1);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Paired doors use one stack shape; other blocks borrow the registered shape slice.
    fn visit_camera_cell(
        &self,
        block: [i32; 3],
        physics: &BlockPhysics,
        touches: &impl Fn(Aabb) -> bool,
        visitor: &mut impl FnMut(Aabb),
    ) -> Result<(), WorldQueryError> {
        let offset = physics.shape_offset(block);
        if physics.door.is_some() && !touches(Aabb::new(Vec3::ZERO, Vec3::ONE).translated(offset)) {
            return Ok(());
        }
        let door = self.resolved_door_shape(block, physics)?;
        let shapes = door.as_slice();
        for shape in if door.is_some() {
            shapes
        } else {
            &physics.shapes
        } {
            let shape = shape.translated(offset);
            if touches(shape) {
                visitor(shape);
            }
        }
        Ok(())
    }
}

//! Camera collision scans borrow palette layers and keep lenient skip counts.

use std::{
    collections::HashMap,
    hash::{BuildHasherDefault, Hash, Hasher},
    sync::Arc,
};

use world::SubChunk;

use super::{
    Aabb, BlockPhysics, LenientSkipCounts, PaletteWorld, SubChunkKey, Vec3, WorldQueryError,
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
        let [min, max] = self.registry.query_bounds(query)?;
        let mut section = SectionCache::default();
        for x in min[0]..=max[0] {
            for z in min[2]..=max[2] {
                for y in min[1]..=max[1] {
                    self.visit_camera_block(
                        [x, y, z],
                        &mut section,
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
        let mut section = SectionCache::default();
        loop {
            self.inspect_camera_segment_halo(
                state.cell,
                origin,
                delta,
                &mut best,
                &mut section,
                &mut skipped,
            )?;
            let next = state.next_crossing();
            if next > 1.0 || best.is_some_and(|hit| strictly_precedes(hit, next)) {
                return Ok((best, skipped));
            }
            for cell in state.advance(next)? {
                self.inspect_camera_segment_halo(
                    cell,
                    origin,
                    delta,
                    &mut best,
                    &mut section,
                    &mut skipped,
                )?;
            }
        }
    }

    fn inspect_camera_segment_halo(
        &self,
        cell: [i32; 3],
        origin: Vec3,
        delta: Vec3,
        best: &mut Option<f64>,
        section: &mut SectionCache,
        skipped: &mut LenientSkipCounts,
    ) -> Result<(), WorldQueryError> {
        let halo = self.registry.collision_halo;
        for x in halo[0].0..=halo[0].1 {
            for y in halo[1].0..=halo[1].1 {
                for z in halo[2].0..=halo[2].1 {
                    self.visit_camera_block(
                        checked_offset(cell, [x, y, z])?,
                        section,
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
        section: &mut SectionCache,
        skipped: &mut LenientSkipCounts,
        touches: &impl Fn(Aabb) -> bool,
        visitor: &mut impl FnMut(Aabb),
    ) -> Result<(), WorldQueryError> {
        let [x, y, z] = block;
        let key = SubChunkKey::new(self.dimension, x >> 4, y >> 4, z >> 4);
        let Some(chunk) = section.get(self, key) else {
            skipped.unloaded_chunk = skipped.unloaded_chunk.saturating_add(1);
            return Ok(());
        };
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

/// The sub-chunk a scan read last, so its cells look the section up once instead of once each.
#[derive(Default)]
pub(super) struct SectionCache {
    key: Option<SubChunkKey>,
    /// `None` when the section is unloaded; `Some(None)` when loaded with no palette storage.
    section: Option<Option<Arc<SubChunk>>>,
}

impl SectionCache {
    /// The loaded section at `key` (`Some(None)` for an all-air one), or `None` when unloaded.
    fn get(
        &mut self,
        world: &PaletteWorld<'_>,
        key: SubChunkKey,
    ) -> Option<&Option<Arc<SubChunk>>> {
        if self.key != Some(key) {
            self.key = Some(key);
            self.section = world
                .store
                .is_sub_chunk_loaded(key)
                .then(|| world.store.sub_chunk(key));
        }
        self.section.as_ref()
    }
}

/// A block cell that hashes each coordinate through `Hasher::write_i32`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellKey([i32; 3]);

impl Hash for CellKey {
    /// Writes the three coordinates in order without a slice length prefix.
    fn hash<H: Hasher>(&self, state: &mut H) {
        for coordinate in self.0 {
            state.write_i32(coordinate);
        }
    }
}

/// Each block cell's camera shapes, read once and reused by later scans while the world is
/// unchanged, as it is across one frame's particle collisions.
#[derive(Default)]
pub struct CameraShapeCache {
    cells: HashMap<CellKey, (u32, u32), BuildHasherDefault<CellHasher>>,
    shapes: Vec<Aabb>,
    section: SectionCache,
}

impl PaletteWorld<'_> {
    /// Visits the camera shapes `query` touches, as [`Self::visit_camera_colliders`] does, reading
    /// each cell through `cache`; unreadable cells contribute nothing.
    pub fn visit_camera_colliders_cached(
        &self,
        query: Aabb,
        cache: &mut CameraShapeCache,
        visitor: &mut dyn FnMut(Aabb),
    ) -> Result<(), WorldQueryError> {
        validate_collision_query(query)?;
        if query.min == query.max {
            return Ok(());
        }
        let [min, max] = self.registry.query_bounds(query)?;
        let CameraShapeCache {
            cells,
            shapes,
            section,
        } = cache;
        for x in min[0]..=max[0] {
            for z in min[2]..=max[2] {
                for y in min[1]..=max[1] {
                    let cell = [x, y, z];
                    let key = CellKey(cell);
                    let (start, end) = match cells.get(&key) {
                        Some(range) => *range,
                        None => {
                            let start = shapes.len() as u32;
                            let mut skipped = LenientSkipCounts::default();
                            self.visit_camera_block(
                                cell,
                                section,
                                &mut skipped,
                                &|_| true,
                                &mut |shape| {
                                    shapes.push(shape);
                                },
                            )?;
                            let range = (start, shapes.len() as u32);
                            cells.insert(key, range);
                            range
                        }
                    };
                    for shape in &shapes[start as usize..end as usize] {
                        if shape.intersects(query) {
                            visitor(*shape);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Hashes block cells with one multiply per coordinate. The keys are local coordinates the client
/// reads itself, so the default hasher's resistance to chosen collisions buys nothing here.
#[derive(Default)]
struct CellHasher(u64);

impl Hasher for CellHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u8(byte);
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write_u64(u64::from(value));
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }

    fn write_i32(&mut self, value: i32) {
        self.write_u64(u64::from(value as u32));
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records coordinate writes and rejects byte or slice-length hashing.
    #[derive(Default)]
    struct CountingHasher {
        coordinates: Vec<i32>,
    }

    impl Hasher for CountingHasher {
        /// Rejects any hashing path that writes bytes instead of whole coordinates.
        fn write(&mut self, _: &[u8]) {
            panic!("cell keys must use write_i32 for each coordinate");
        }

        /// Records each coordinate passed to the typed hashing path.
        fn write_i32(&mut self, coordinate: i32) {
            self.coordinates.push(coordinate);
        }

        /// Returns a placeholder digest; this hasher only counts writes.
        fn finish(&self) -> u64 {
            0
        }
    }

    #[test]
    fn cell_key_hashes_once_per_coordinate() {
        for coordinates in [[0; 3], [1, -2, 3], [i32::MIN, 0, i32::MAX]] {
            let mut hasher = CountingHasher::default();
            CellKey(coordinates).hash(&mut hasher);
            assert_eq!(hasher.coordinates.len(), 3);
            assert_eq!(hasher.coordinates, coordinates);
        }
    }
}

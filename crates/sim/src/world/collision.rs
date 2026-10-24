use super::{section::SectionCache, *};

// Collision revision allocation starts at one; zero identifies an absent column.
const UNLOADED_BOUNDARY_REVISION: u64 = 0;
// Extend beyond valid i32 coordinates so f32-rounded queries overlap at both limits.
const UNLOADED_BOUNDARY_VERTICAL_LIMIT: f64 = -(i32::MIN as f64) * 2.0;

impl<'a> PaletteWorld<'a> {
    /// Keeps anchors in absent terrain blocked until their own feet region arrives.
    pub(super) fn require_movement_origin(&self, feet: Vec3) -> Result<(), WorldQueryError> {
        validate_collision_query(Aabb::new(feet, feet))?;
        let [x, y, z] = block_floor(feet)?;
        let section = SubChunkKey::new(self.dimension, x >> 4, y >> 4, z >> 4);
        if self.unloaded_collision_barriers && !self.store.is_sub_chunk_loaded(section) {
            return Err(WorldQueryError::UnloadedChunk(section.chunk()));
        }
        Ok(())
    }

    /// Borrows movement terrain with solid unloaded-column collision barriers.
    /// Prediction still waits for the column containing the player's feet.
    /// Partially received columns still require authoritative data for every queried section.
    #[must_use]
    pub fn for_movement(
        store: &'a ChunkStore,
        registry: &'a CollisionRegistry,
        dimension: i32,
    ) -> Self {
        let mut world = Self::new(store, registry, dimension, Default::default());
        world.unloaded_collision_barriers = true;
        world
    }

    /// Identifies an absent column without assigning it a loaded collision revision.
    pub(super) fn unloaded_boundary_revision(
        &self,
        chunk: ChunkKey,
    ) -> Option<ChunkCollisionRevision> {
        (self.unloaded_collision_barriers && self.store.collision_revision(chunk).is_none())
            .then_some(ChunkCollisionRevision {
                chunk,
                revision: UNLOADED_BOUNDARY_REVISION,
            })
    }

    /// Emits boundary geometry without attributing it to an invented palette block.
    fn emit_unloaded_boundary_instances(
        &self,
        query: Aabb,
        chunks: &[ChunkKey],
        emit: &mut impl FnMut(CollisionInstance),
    ) {
        let side = world::SUB_CHUNK_SIDE as f64;
        for &chunk in chunks {
            if self.unloaded_boundary_revision(chunk).is_none() {
                continue;
            }
            let x = f64::from(chunk.x) * side;
            let z = f64::from(chunk.z) * side;
            let shape = Aabb::new(
                Vec3::new(x, -UNLOADED_BOUNDARY_VERTICAL_LIMIT, z),
                Vec3::new(x + side, UNLOADED_BOUNDARY_VERTICAL_LIMIT, z + side),
            );
            if shape.intersects(query) {
                emit(CollisionInstance {
                    shape,
                    block: None,
                    runtime_id: None,
                    cell_has_scaffolding: None,
                });
            }
        }
    }

    /// Shared emission core behind both public collision surfaces so their
    /// outputs structurally cannot diverge: one translated registry shape
    /// instance per (block, layer, shape) triple in block-scan order, each
    /// kept only when it intersects `query`. Returns the scanned columns' identity;
    /// on error the instances already emitted must be discarded.
    pub(super) fn visit_collision_instances(
        &self,
        query: Aabb,
        emit: &mut impl FnMut(CollisionInstance),
    ) -> Result<WorldCollisionIdentity, WorldQueryError> {
        validate_collision_query(query)?;
        if query.min == query.max {
            return WorldCollisionIdentity::new(self.registry.identity(), []);
        }
        let [min, max] = self.registry.query_bounds(query)?;
        let chunks = (min[0] >> 4..=max[0] >> 4)
            .flat_map(|x| {
                (min[2] >> 4..=max[2] >> 4).map(move |z| ChunkKey::new(self.dimension, x, z))
            })
            .collect::<Vec<_>>();
        let identity = self.identity_for_chunks(chunks.iter().copied())?;

        self.emit_unloaded_boundary_instances(query, &chunks, &mut *emit);
        let mut section = SectionCache::default();
        // Consecutive scan columns share a chunk column; check its boundary state once.
        let mut column: Option<(ChunkKey, bool)> = None;
        for x in min[0]..=max[0] {
            for z in min[2]..=max[2] {
                let chunk = ChunkKey::new(self.dimension, x >> 4, z >> 4);
                let boundary = match column {
                    Some((cached, boundary)) if cached == chunk => boundary,
                    _ => {
                        let boundary = self.unloaded_boundary_revision(chunk).is_some();
                        column = Some((chunk, boundary));
                        boundary
                    }
                };
                if boundary {
                    continue;
                }
                for y in min[1]..=max[1] {
                    let block = [x, y, z];
                    let layers = self.layers_at(block, &mut section)?;
                    // Every shape of the cell carries its any-layer scaffolding fact. An unknown
                    // layer fails below in scan order, discarding the whole query.
                    let cell_has_scaffolding = layers
                        .iter()
                        .filter_map(|runtime_id| self.registry.physics(runtime_id))
                        .any(|physics| physics.flags.contains(BlockPhysicsFlags::SCAFFOLDING));
                    for runtime_id in layers.iter() {
                        let physics = self
                            .registry
                            .physics(runtime_id)
                            .ok_or(WorldQueryError::UnknownRuntimeId { runtime_id, block })?;
                        let shapes = self.block_collision_shapes(block, physics, query)?;
                        if shapes.is_empty() {
                            continue;
                        }
                        let offset = physics.shape_offset(block);
                        for shape in shapes.iter() {
                            let shape = shape.translated(offset);
                            if shape.intersects(query) {
                                emit(CollisionInstance {
                                    shape,
                                    block: Some(block),
                                    runtime_id: Some(runtime_id),
                                    cell_has_scaffolding: Some(cell_has_scaffolding),
                                });
                            }
                        }
                    }
                }
            }
        }
        if let Some(colliders) = &self.actor_colliders {
            for &shape in colliders.iter() {
                if shape.intersects(query) {
                    emit(CollisionInstance {
                        shape,
                        block: None,
                        runtime_id: None,
                        cell_has_scaffolding: None,
                    });
                }
            }
        }
        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MovementInput, MovementMode, PlayerState, SimulationError, Simulator, SneakProfile,
    };

    /// Builds an air registry and one authoritative column beside absent terrain.
    fn terrain() -> (ChunkStore, CollisionRegistry) {
        let mut registry = CollisionRegistry::new();
        registry.register(0, []).unwrap();
        let mut store = ChunkStore::new();
        store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
        (store, registry)
    }

    #[test]
    fn unloaded_boundaries_block_travel_past_the_previous_vertical_limits() {
        let (store, registry) = terrain();
        let world = PaletteWorld::for_movement(&store, &registry, 0);
        for y in [-100_002.0, 100_001.0] {
            let query = Aabb::new(Vec3::new(15.5, y, 7.5), Vec3::new(16.5, y + 1.0, 8.5));
            let boxes = world.collision_boxes(query).unwrap();
            assert_eq!(boxes.value.len(), 1);
            assert!(boxes.value[0].intersects(query));
            assert!(world.block_physics([16, y as i32, 8]).is_ok());
        }
    }

    #[test]
    fn flight_boundary_has_absence_identity_and_no_invented_block_provenance() {
        let (store, registry) = terrain();
        let query = Aabb::new(Vec3::new(15.5, 80.0, 7.5), Vec3::new(16.5, 82.0, 8.5));
        let flight = PaletteWorld::for_movement(&store, &registry, 0);
        let boxes = flight.collision_boxes_with_provenance(query).unwrap();
        assert_eq!(boxes.value.len(), 1);
        assert_eq!(boxes.value[0].block, None);
        assert_eq!(boxes.value[0].runtime_id, None);
        assert_eq!(boxes.value[0].aabb.min.x, world::SUB_CHUNK_SIDE as f64);
        assert!(boxes.identity.chunks.iter().any(
            |revision| revision.chunk.x == 1 && revision.revision == UNLOADED_BOUNDARY_REVISION
        ));
        assert!(!store.is_chunk_loaded(ChunkKey::new(0, 1, 0)));
        assert!(
            PaletteWorld::new(&store, &registry, 0, Default::default())
                .collision_boxes(query)
                .is_err()
        );
    }

    #[test]
    fn absent_occupied_columns_pause_prediction_and_retained_snapshots() {
        for position in [
            Vec3::new(24.0, 80.0, 8.0),
            Vec3::new(16.125, 80.0, 8.0),
            Vec3::new(-0.125, 80.0, 8.0),
            Vec3::new(8.0, 80.0, 16.125),
        ] {
            for mode in [MovementMode::Walking, MovementMode::Flying] {
                let (mut store, registry) = terrain();
                let world = PaletteWorld::for_movement(&store, &registry, 0);
                let snapshot = world.snapshot().unwrap();
                let chunk = ChunkKey::new(
                    0,
                    position.x.floor() as i32 >> 4,
                    position.z.floor() as i32 >> 4,
                );
                let mut state = PlayerState::new(position);
                state.velocity = Vec3::new(0.25, -0.5, 0.0);
                let anchor = state.clone();
                let input = MovementInput {
                    mode,
                    forward: 1.0,
                    ..Default::default()
                };
                let simulator = Simulator::default();
                assert!(matches!(
                    simulator.tick(&mut state, input, &world),
                    Err(SimulationError::World(WorldQueryError::UnloadedChunk(key))) if key == chunk
                ));
                assert_eq!(state, anchor);
                assert_eq!(
                    simulator.movement_environment(
                        position,
                        mode,
                        false,
                        SneakProfile::Current,
                        &world
                    ),
                    Err(WorldQueryError::UnloadedChunk(chunk))
                );
                store.mark_chunk_loaded(chunk).unwrap();
                assert!(matches!(
                    simulator.tick(&mut state, input, &snapshot),
                    Err(SimulationError::World(WorldQueryError::UnloadedChunk(key))) if key == chunk
                ));
                assert_eq!(state, anchor);
                simulator
                    .tick(
                        &mut state,
                        input,
                        &PaletteWorld::for_movement(&store, &registry, 0),
                    )
                    .unwrap();
                assert_eq!(state.tick, anchor.tick + 1);
            }
        }
    }

    #[test]
    fn unloaded_boundaries_cover_the_supported_vertical_query_range() {
        let (store, registry) = terrain();
        let world = PaletteWorld::for_movement(&store, &registry, 0);
        let snapshot = world.snapshot().unwrap();
        let [min_coordinate, max_coordinate] = COLLISION_QUERY_COORDINATE_RANGE;
        // Owner-cell scans use f32 arithmetic, so the endpoint faces must stay
        // inside the validated range after conversion to that precision.
        let min_y = f64::from((min_coordinate as f32).next_up());
        let max_y = f64::from((max_coordinate as f32).next_down());
        let height = 2.0;
        for y in [-1_000_000.0, 1_000_000.0, min_y, max_y - height] {
            let query = Aabb::new(Vec3::new(15.5, y, 7.5), Vec3::new(16.5, y + height, 8.5));
            validate_collision_query(query).unwrap();
            let boxes = world.collision_boxes(query).unwrap();
            assert_eq!(boxes.value.len(), 1, "query at Y={y}");
            assert!(boxes.value[0].intersects(query), "query at Y={y}");
            assert!(boxes.value[0].min.y <= min_coordinate);
            assert!(boxes.value[0].max.y >= max_coordinate);
            let provenance = world.collision_boxes_with_provenance(query).unwrap();
            assert_eq!(provenance.value.len(), 1);
            assert_eq!(provenance.value[0].aabb, boxes.value[0]);
            assert_eq!(provenance.value[0].block, None);
            assert_eq!(provenance.value[0].runtime_id, None);
            assert_eq!(provenance.identity, boxes.identity);
            assert_eq!(snapshot.collision_boxes(query).unwrap(), boxes);
        }
        for y in [
            f64::from((min_y as f32).next_down()),
            f64::from((max_y as f32).next_up()),
        ] {
            let query = Aabb::new(Vec3::new(15.5, y, 7.5), Vec3::new(16.5, y + height, 8.5));
            assert_eq!(
                world.collision_boxes(query),
                Err(WorldQueryError::CoordinateOutOfRange)
            );
            assert_eq!(
                snapshot.collision_boxes(query),
                Err(WorldQueryError::CoordinateOutOfRange)
            );
        }
    }

    #[test]
    fn movement_cannot_enter_absent_columns_at_high_or_low_altitudes() {
        let (store, registry) = terrain();
        let world = PaletteWorld::for_movement(&store, &registry, 0);
        for y in [-1_000_000.0, 1_000_000.0] {
            let mut state = PlayerState::new(Vec3::new(15.5, y, 8.0));
            state.velocity.x = 1.0;
            let result = Simulator::default()
                .tick(
                    &mut state,
                    MovementInput {
                        mode: MovementMode::Flying,
                        ..Default::default()
                    },
                    &world,
                )
                .unwrap();
            assert!(result.collisions.x, "movement at Y={y}");
            assert!(
                Aabb::player_at(state.position).max.x <= world::SUB_CHUNK_SIDE as f64 + 0.00001,
                "movement at Y={y} entered absent terrain"
            );
        }
    }

    #[test]
    fn terrain_arrival_replaces_boundary_while_snapshot_preserves_it() {
        let (mut store, registry) = terrain();
        let query = Aabb::new(Vec3::new(15.5, 80.0, 7.5), Vec3::new(16.5, 82.0, 8.5));
        let flight = PaletteWorld::for_movement(&store, &registry, 0);
        let before = flight.collision_boxes(query).unwrap();
        let snapshot = flight.snapshot().unwrap();
        store.mark_chunk_loaded(ChunkKey::new(0, 1, 0)).unwrap();
        let arrived = PaletteWorld::for_movement(&store, &registry, 0)
            .collision_boxes(query)
            .unwrap();
        assert!(arrived.value.is_empty());
        assert!(
            arrived
                .identity
                .chunks
                .iter()
                .all(|revision| revision.revision != UNLOADED_BOUNDARY_REVISION)
        );
        assert_ne!(arrived.identity, before.identity);
        assert_eq!(snapshot.collision_boxes(query).unwrap(), before);
    }

    #[test]
    fn flight_boundaries_preserve_partial_section_and_unknown_block_errors() {
        let (mut store, registry) = terrain();
        let key = SubChunkKey::new(0, 1, 0, 0);
        store.mark_sub_chunk_loaded(key).unwrap();
        let flight = PaletteWorld::for_movement(&store, &registry, 0);
        assert!(matches!(
            flight.block_physics([16, 80, 8]),
            Err(WorldQueryError::UnloadedChunk(_))
        ));
        store.mark_chunk_loaded(key.chunk()).unwrap();
        let high = SubChunkKey::new(0, 1, 5, 0);
        store
            .update_block(high, world::BlockUpdate::new(0, 0, 8, 0, 99), 0)
            .unwrap();
        let flight = PaletteWorld::for_movement(&store, &registry, 0);
        assert!(matches!(
            flight.block_physics([16, 80, 8]),
            Err(WorldQueryError::UnknownRuntimeId { runtime_id: 99, .. })
        ));
    }
}

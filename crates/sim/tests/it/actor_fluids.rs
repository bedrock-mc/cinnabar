use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    SurfaceResponse, Vec3, WorldQueryError, sample_actor_liquid_submersion, sample_actor_liquids,
    sample_liquid_submersion,
};

#[path = "../fixtures/actor_fluids.rs"]
mod fixture;

#[test]
fn actor_body_material_sampling_allocates_nothing() {
    let (store, registry) = fixture::world();
    let world = sim::PaletteWorld::new(&store, &registry, 0, Default::default());
    let (sample, allocations) =
        test_alloc::count(|| sample_actor_liquids(&world, fixture::body()).unwrap());
    assert_eq!(sample, (true, false));
    assert_eq!(allocations, 0);
}

#[test]
fn actor_breathing_material_sampling_allocates_nothing() {
    let (store, registry) = fixture::world();
    let world = sim::PaletteWorld::new(&store, &registry, 0, Default::default());
    let (submerged, allocations) = test_alloc::count(|| {
        sample_actor_liquid_submersion(&world, Vec3::new(0.5, 0.61, 0.5)).unwrap()
    });
    assert!(submerged);
    assert_eq!(allocations, 0);
}

#[test]
fn retained_material_probes_allocate_nothing_after_live_chunk_changes() {
    use world::{BlockUpdate, SubChunkKey};

    let (mut store, registry) = fixture::world();
    let snapshot = sim::PaletteWorld::new(&store, &registry, 0, Default::default())
        .snapshot()
        .unwrap();
    for layer in 0..2 {
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(0, 0, 0, layer, 0),
                0,
            )
            .unwrap();
    }
    let ((contact, submerged), allocations) = test_alloc::count(|| {
        (
            sample_actor_liquids(&snapshot, fixture::body()).unwrap(),
            sample_actor_liquid_submersion(&snapshot, Vec3::new(0.5, 0.61, 0.5)).unwrap(),
        )
    });
    assert_eq!((contact, submerged), ((true, false), true));
    assert_eq!(allocations, 0);
    let live = sim::PaletteWorld::new(&store, &registry, 0, Default::default());
    assert_eq!(
        sample_actor_liquids(&live, fixture::body()).unwrap(),
        (false, false)
    );
}

/// Uses the owned physics fallback so comparisons exercise both query paths.
struct PhysicsOnly<'a>(&'a dyn CollisionWorld);

impl CollisionWorld for PhysicsOnly<'_> {
    /// Forwards shapes unchanged; fluid queries only use the physics method.
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.0.collision_boxes(query)
    }

    /// Keeps the original material allocation and provenance path for comparison.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.0.block_physics(block)
    }
}

#[test]
fn material_probes_match_owned_samples_at_surface_edges_and_negative_cells() {
    use world::{BlockUpdate, ChunkKey, SubChunkKey};

    let (mut store, mut registry) = fixture::world();
    for flags in [BlockPhysicsFlags::WATER, BlockPhysicsFlags::LAVA] {
        for height in [0.000_001, 1.0 / 9.0, 0.5, 0.875, 1.0] {
            registry
                .register_physics(2, [], 0.6, 1.0, 1.0, height, flags, SurfaceResponse::None)
                .unwrap();
            for block in [[0; 3], [-1; 3]] {
                let [x, y, z] = block;
                let key = SubChunkKey::new(0, x >> 4, y >> 4, z >> 4);
                store
                    .mark_chunk_loaded(ChunkKey::new(0, key.x, key.z))
                    .unwrap();
                for (primary, secondary) in [(0, 2), (2, 0), (1, 2), (2, 99), (99, 2)] {
                    for (layer, id) in [(0, primary), (1, secondary)] {
                        store
                            .update_block(
                                key,
                                BlockUpdate::new(
                                    x.rem_euclid(16) as u8,
                                    y.rem_euclid(16) as u8,
                                    z.rem_euclid(16) as u8,
                                    layer,
                                    id,
                                ),
                                0,
                            )
                            .unwrap();
                    }
                    let world = sim::PaletteWorld::new(&store, &registry, 0, Default::default());
                    let origin = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
                    let body = fixture::body().translated(origin);
                    assert_eq!(
                        sample_actor_liquids(&world, body),
                        sample_actor_liquids(&PhysicsOnly(&world), body),
                    );
                    for offset in [0.0, 0.25, 0.5, 0.875, 0.999_999_9, 1.0] {
                        let point = origin + Vec3::new(0.5, offset, 0.5);
                        assert_eq!(
                            sample_actor_liquid_submersion(&world, point),
                            sample_liquid_submersion(&world, point).map(|sample| sample.value),
                            "block {block:?}, layers {primary}/{secondary}, height {height}, offset {offset}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn material_probes_preserve_sparse_air_and_unavailable_column_errors() {
    use world::{ChunkKey, SubChunkKey};

    let (mut store, registry) = fixture::world();
    // A partially loaded column permits only its known section.
    store
        .mark_sub_chunk_loaded(SubChunkKey::new(0, 1, 0, 0))
        .unwrap();
    store.mark_chunk_loaded(ChunkKey::new(0, -1, 0)).unwrap();
    let world = sim::PaletteWorld::new(&store, &registry, 0, Default::default());
    for x in [-0.5, 0.5, 16.5, 32.5] {
        for y in [0.5, 100.5] {
            let point = Vec3::new(x, y, 0.5);
            let body = fixture::body().translated(Vec3::new(x - 0.5, y - 0.5, 0.0));
            assert_eq!(
                sample_actor_liquids(&world, body),
                sample_actor_liquids(&PhysicsOnly(&world), body)
            );
            assert_eq!(
                sample_actor_liquid_submersion(&world, point),
                sample_liquid_submersion(&world, point).map(|sample| sample.value)
            );
        }
    }
    for point in [Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(f64::MAX, 0.0, 0.0)] {
        assert_eq!(
            sample_actor_liquid_submersion(&world, point),
            sample_liquid_submersion(&world, point).map(|sample| sample.value)
        );
    }
}

#[test]
fn material_contact_keeps_scanning_after_liquid_is_found() {
    use world::{BlockUpdate, SubChunkKey};

    let (mut store, registry) = fixture::world();
    store
        .update_block(
            SubChunkKey::new(0, 0, 0, 0),
            BlockUpdate::new(2, 0, 0, 0, 99),
            0,
        )
        .unwrap();
    let world = sim::PaletteWorld::new(&store, &registry, 0, Default::default());
    let body = Aabb::new(Vec3::new(0.3, 0.25, 0.3), Vec3::new(2.7, 0.65, 0.7));
    assert_eq!(
        sample_actor_liquids(&world, body),
        Err(WorldQueryError::UnknownRuntimeId {
            runtime_id: 99,
            block: [2, 0, 0]
        })
    );
}

struct FluidWorld {
    layers: Box<[BlockPhysicsFacts]>,
    available: bool,
}

impl CollisionWorld for FluidWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(vec![]))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        if !self.available {
            return Err(WorldQueryError::UnknownRuntimeId {
                runtime_id: 1,
                block,
            });
        }
        Ok(BlockPhysicsSample {
            layers: if block == [0, 0, 0] {
                self.layers.clone()
            } else {
                Box::new([facts(BlockPhysicsFlags::default(), 0.0)])
            },
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn facts(flags: BlockPhysicsFlags, fluid_height_blocks: f64) -> BlockPhysicsFacts {
    BlockPhysicsFacts {
        friction: 0.6,
        horizontal_speed_factor: 1.0,
        vertical_speed_factor: 1.0,
        fluid_height_blocks,
        flags,
        surface_response: SurfaceResponse::None,
    }
}

fn water() -> FluidWorld {
    FluidWorld {
        layers: Box::new([facts(BlockPhysicsFlags::WATER, 0.875)]),
        available: true,
    }
}

#[test]
fn fish_liquid_contact_ignores_the_rendered_water_surface() {
    let body = Aabb::new(Vec3::new(0.3, 0.79, 0.3), Vec3::new(0.7, 1.19, 0.7));
    // The former feet + 0.1 probe exceeds 0.875. The native small-body probe's
    // center remains in the water material cell and must keep the swimming state.
    assert_eq!(sample_actor_liquids(&water(), body).unwrap(), (true, false));
}

#[test]
fn fish_liquid_contact_reaches_water_beside_its_origin_cell() {
    let body = Aabb::new(Vec3::new(0.86, 0.4, 0.3), Vec3::new(1.26, 0.8, 0.7));
    assert_eq!(sample_actor_liquids(&water(), body).unwrap(), (true, false));
}

#[test]
fn fish_above_the_water_block_is_dry() {
    let body = Aabb::new(Vec3::new(0.3, 1.01, 0.3), Vec3::new(0.7, 1.41, 0.7));
    assert_eq!(
        sample_actor_liquids(&water(), body).unwrap(),
        (false, false)
    );
}

#[test]
fn actor_liquid_contact_reads_waterlogged_secondary_layers() {
    let world = FluidWorld {
        layers: Box::new([
            facts(BlockPhysicsFlags::default(), 0.0),
            facts(BlockPhysicsFlags::WATER, 0.875),
        ]),
        available: true,
    };
    let body = Aabb::new(Vec3::new(0.3, 0.4, 0.3), Vec3::new(0.7, 0.8, 0.7));
    assert_eq!(sample_actor_liquids(&world, body).unwrap(), (true, false));
}

#[test]
fn unavailable_actor_fluid_data_does_not_report_dry_land() {
    let world = FluidWorld {
        available: false,
        ..water()
    };
    let body = Aabb::new(Vec3::new(0.3, 0.4, 0.3), Vec3::new(0.7, 0.8, 0.7));
    assert!(matches!(
        sample_actor_liquids(&world, body),
        Err(WorldQueryError::UnknownRuntimeId { .. })
    ));
}

#[test]
fn actor_water_and_lava_probes_keep_their_distinct_native_margins() {
    let body = Aabb::player_at(Vec3::new(1.25, 0.0, 0.5));
    assert_eq!(sample_actor_liquids(&water(), body).unwrap(), (true, false));
    let world = FluidWorld {
        layers: Box::new([facts(BlockPhysicsFlags::LAVA, 0.875)]),
        available: true,
    };
    assert_eq!(sample_actor_liquids(&world, body).unwrap(), (false, false));
}

#[test]
fn simultaneous_liquid_contact_selects_native_lava_state() {
    struct AdjacentLiquids(FluidWorld);
    impl CollisionWorld for AdjacentLiquids {
        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            self.0.collision_boxes(query)
        }
        fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
            if block == [1, 0, 0] {
                Ok(BlockPhysicsSample {
                    layers: Box::new([facts(BlockPhysicsFlags::LAVA, 0.875)]),
                    identity: CollisionQuery::synthetic(()).identity,
                })
            } else {
                self.0.block_physics(block)
            }
        }
    }
    let body = Aabb::new(Vec3::new(0.7, 0.4, 0.3), Vec3::new(1.3, 0.8, 0.7));
    assert_eq!(
        sample_actor_liquids(&AdjacentLiquids(water()), body).unwrap(),
        (false, true)
    );
}

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    SurfaceResponse, Vec3, WorldQueryError, sample_actor_liquids,
};

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

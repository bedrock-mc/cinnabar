use std::{cell::Cell, collections::BTreeMap};

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery,
    CollisionRegistryIdentity, CollisionWorld, MovementInput, PlayerState, Simulator,
    SurfaceResponse, Vec3, WorldCollisionIdentity, WorldQueryError,
};

#[derive(Default)]
struct TerrainWorld {
    boxes: Vec<Aabb>,
    facts: BTreeMap<[i32; 3], BlockPhysicsFacts>,
    queries: Cell<usize>,
    fail_after: Option<usize>,
}

impl TerrainWorld {
    fn floor(min: Vec3, max: Vec3) -> Self {
        Self {
            boxes: vec![Aabb::new(min, max)],
            ..Self::default()
        }
    }

    fn identity(chunk_x: i32) -> WorldCollisionIdentity {
        WorldCollisionIdentity::new(
            CollisionRegistryIdentity {
                protocol: 1001,
                id_space: sim::CollisionIdSpace::Sequential,
                preg_sha256: [0x31; 32],
            },
            [world::ChunkCollisionRevision {
                chunk: world::ChunkKey::new(0, chunk_x, 0),
                revision: u64::try_from(chunk_x + 2).unwrap(),
            }],
        )
        .unwrap()
    }

    fn poll(&self) -> Result<(), WorldQueryError> {
        let next = self.queries.get() + 1;
        self.queries.set(next);
        if self.fail_after.is_some_and(|limit| next > limit) {
            return Err(WorldQueryError::UnloadedChunk(world::ChunkKey::new(
                0, 2, 3,
            )));
        }
        Ok(())
    }
}

impl CollisionWorld for TerrainWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.poll()?;
        Ok(CollisionQuery {
            value: self
                .boxes
                .iter()
                .copied()
                .filter(|shape| shape.intersects(query))
                .collect(),
            identity: Self::identity(1),
        })
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.poll()?;
        let facts = self
            .facts
            .get(&block)
            .copied()
            .unwrap_or(BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: BlockPhysicsFlags::default(),
                surface_response: SurfaceResponse::None,
            });
        Ok(BlockPhysicsSample {
            layers: Box::new([facts]),
            identity: Self::identity(0),
        })
    }
}

fn grounded(position: Vec3) -> PlayerState {
    let mut state = PlayerState::new(position);
    state.on_ground = true;
    state
}

#[test]
fn flat_and_diagonal_motion_are_normalized_and_bind_world_identity() {
    let world = TerrainWorld::floor(Vec3::new(-16.0, 0.0, -16.0), Vec3::new(16.0, 1.0, 16.0));
    let mut straight = grounded(Vec3::new(0.0, 1.0, 0.0));
    let straight_tick = Simulator::default()
        .tick(
            &mut straight,
            MovementInput {
                forward: 1.0,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    let mut diagonal = grounded(Vec3::new(0.0, 1.0, 0.0));
    let diagonal_tick = Simulator::default()
        .tick(
            &mut diagonal,
            MovementInput {
                strafe: 1.0,
                forward: 1.0,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();

    let straight_distance = straight_tick.movement.horizontal_length_squared();
    let diagonal_distance = diagonal_tick.movement.horizontal_length_squared();
    assert!(diagonal_distance <= 0.010_000_01);
    assert!(diagonal_distance >= straight_distance);
    assert_eq!(straight_tick.world_identity.chunks.len(), 2);
    assert_eq!(
        straight_tick.world_identity.registry,
        TerrainWorld::identity(0).registry
    );
}

#[test]
fn sneaking_clips_motion_at_each_exposed_ledge_orientation() {
    for velocity in [
        Vec3::new(0.8, 0.0, 0.0),
        Vec3::new(-0.8, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 0.8),
        Vec3::new(0.0, 0.0, -0.8),
    ] {
        let world = TerrainWorld::floor(Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.0, 0.5));
        let mut state = grounded(Vec3::new(0.0, 1.0, 0.0));
        state.velocity = velocity;
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    sneaking: true,
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        assert!(tick.movement.x.abs() <= 0.76, "{tick:?}");
        assert!(tick.movement.z.abs() <= 0.76, "{tick:?}");
    }
}

#[test]
fn compound_slab_step_and_head_collision_use_exact_shapes() {
    let mut world = TerrainWorld::floor(Vec3::new(-4.0, 0.0, -4.0), Vec3::new(4.0, 1.0, 4.0));
    world.boxes.extend([
        Aabb::new(Vec3::new(-0.5, 1.0, 0.7), Vec3::new(0.5, 1.5, 1.7)),
        Aabb::new(Vec3::new(-0.2, 1.5, 1.1), Vec3::new(0.2, 2.0, 1.5)),
    ]);
    let mut state = grounded(Vec3::new(0.0, 1.0, 0.4));
    state.velocity.z = 0.5;
    let stepped = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert_eq!(stepped.movement.y, 0.5);
    assert!((stepped.movement.z - 0.4001).abs() <= 1.0e-12);

    let mut jumping = grounded(Vec3::new(0.0, 1.0, -0.5));
    jumping.velocity.y = 0.8;
    world.boxes.push(Aabb::new(
        Vec3::new(-1.0, 3.0, -1.0),
        Vec3::new(1.0, 3.2, 1.0),
    ));
    let hit = Simulator::default()
        .tick(&mut jumping, MovementInput::default(), &world)
        .unwrap();
    assert!(hit.collisions.y);
    assert!(hit.movement.y < 0.8);
}

#[test]
fn query_failure_is_transactional_and_sampling_is_bounded() {
    let world = TerrainWorld {
        fail_after: Some(8),
        ..TerrainWorld::floor(Vec3::new(-16.0, 0.0, -16.0), Vec3::new(16.0, 1.0, 16.0))
    };
    let mut state = grounded(Vec3::new(0.0, 1.0, 0.0));
    state.velocity = Vec3::new(0.5, 0.0, 0.5);
    let before = state.clone();
    let result = Simulator::default().tick(&mut state, MovementInput::default(), &world);
    assert!(matches!(result, Err(sim::SimulationError::World(_))));
    assert_eq!(state, before);
    assert!(world.queries.get() <= sim::MAX_BLOCK_SAMPLES_PER_TICK + 3);
}

#[test]
fn adversarial_finite_inputs_fail_without_mutation_and_large_sweeps_stop_before_sampling() {
    let simulator = Simulator::default();
    for (position, velocity, input) in [
        (
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.25, -0.25, 0.25),
            MovementInput {
                strafe: -1.0,
                forward: 1.0,
                yaw_degrees: 359.0,
                ..MovementInput::default()
            },
        ),
        (
            Vec3::new(-15.5, 2.0, 15.5),
            Vec3::new(-0.5, 0.5, -0.5),
            MovementInput {
                strafe: 1.0,
                forward: -1.0,
                yaw_degrees: -720.0,
                ..MovementInput::default()
            },
        ),
    ] {
        let world = TerrainWorld {
            fail_after: Some(0),
            ..TerrainWorld::default()
        };
        let mut state = PlayerState::new(position);
        state.velocity = velocity;
        let before = state.clone();
        assert!(matches!(
            simulator.tick(&mut state, input, &world),
            Err(sim::SimulationError::World(_))
        ));
        assert_eq!(state, before);
    }

    let world = TerrainWorld::default();
    let mut state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    state.velocity.x = 70.0;
    let before = state.clone();
    assert_eq!(
        simulator.tick(&mut state, MovementInput::default(), &world),
        Err(sim::SimulationError::World(
            WorldQueryError::QueryExtentExceeded
        ))
    );
    assert_eq!(state, before);
    assert_eq!(world.queries.get(), 0);
}

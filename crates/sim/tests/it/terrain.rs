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
fn soul_speed_removes_native_acceleration_friction_without_multiplying_the_attribute() {
    let mut world = TerrainWorld::floor(Vec3::new(-4.0, 0.0, -4.0), Vec3::new(4.0, 1.0, 4.0));
    world.facts.insert(
        [0, 0, 0],
        BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 0.543,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 0.0,
            flags: BlockPhysicsFlags::default(),
            surface_response: SurfaceResponse::SoulSand,
        },
    );
    for (level, expected) in [(0, 0x3d5a_5cc8), (1, 0x3dc8_b43a), (3, 0x3dc8_b43a)] {
        let mut state = grounded(Vec3::new(0.0, 1.0, 0.0));
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    forward: 1.0,
                    soul_speed: level,
                    movement_speed: Some(0.1),
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        assert_eq!(
            (tick.movement.z as f32).to_bits(),
            expected,
            "level {level}"
        );
    }
}

#[test]
fn consumable_use_scales_ground_and_air_input_before_sneak_and_impulse() {
    let world = TerrainWorld::floor(Vec3::new(-16.0, 0.0, -16.0), Vec3::new(16.0, 1.0, 16.0));
    for (on_ground, sneaking, expected_factor) in [
        (true, false, 0.1225),
        (true, true, 0.03675),
        (false, false, 0.1225),
        (false, true, 0.03675),
    ] {
        let mut baseline = PlayerState::new(Vec3::new(0.0, if on_ground { 1.0 } else { 4.0 }, 0.0));
        baseline.on_ground = on_ground;
        let baseline_tick = Simulator::default()
            .tick(
                &mut baseline,
                MovementInput {
                    forward: 1.0,
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        let mut state = PlayerState::new(Vec3::new(0.0, if on_ground { 1.0 } else { 4.0 }, 0.0));
        state.on_ground = on_ground;
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    forward: 1.0,
                    sneaking,
                    using_consumable: true,
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        assert!(
            (tick.movement.z - baseline_tick.movement.z * expected_factor).abs() <= 1.0e-7,
            "{tick:?}"
        );
    }
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
fn grounded_movement_uses_snapshotted_authority_and_surface_formula() {
    let mut world = TerrainWorld::floor(Vec3::new(-16.0, 0.0, -16.0), Vec3::new(16.0, 1.0, 16.0));
    world.facts.insert(
        [0, 0, 0],
        BlockPhysicsFacts {
            friction: 0.8,
            horizontal_speed_factor: 0.4,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 0.0,
            flags: BlockPhysicsFlags::default(),
            surface_response: SurfaceResponse::None,
        },
    );
    let mut state = grounded(Vec3::new(0.0, 1.0, 0.0));
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                forward: 1.0,
                sprinting: true,
                movement_speed: Some(0.25),
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();

    // The support block's speed factor never scales ground acceleration.
    let friction: f64 = 0.91 * 0.8;
    let expected = 0.98 * 0.25 * 1.3 * 0.162_771_36 / friction.powi(3);
    assert!((tick.movement.z - expected).abs() <= 1.0e-7, "{tick:?}");
}

#[test]
fn zero_authority_is_valid_and_absent_authority_uses_vanilla_default() {
    let world = TerrainWorld::floor(Vec3::new(-16.0, 0.0, -16.0), Vec3::new(16.0, 1.0, 16.0));
    let mut zero = grounded(Vec3::new(0.0, 1.0, 0.0));
    let zero_tick = Simulator::default()
        .tick(
            &mut zero,
            MovementInput {
                forward: 1.0,
                movement_speed: Some(0.0),
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert_eq!(zero_tick.movement.z, 0.0);

    let mut absent = grounded(Vec3::new(0.0, 1.0, 0.0));
    let default_tick = Simulator::default()
        .tick(
            &mut absent,
            MovementInput {
                forward: 1.0,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    let friction: f64 = 0.91 * 0.6;
    let expected = 0.98 * 0.1 * 0.162_771_36 / friction.powi(3);
    assert!((default_tick.movement.z - expected).abs() <= 1.0e-7);
}

#[test]
fn air_speed_ignores_ground_movement_authority() {
    let world = TerrainWorld::default();
    for (sprinting, expected) in [(false, 0.0196), (true, 0.025_48)] {
        let mut state = PlayerState::new(Vec3::new(0.0, 4.0, 0.0));
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    forward: 1.0,
                    sprinting,
                    movement_speed: Some(10.0),
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        assert!((tick.movement.z - expected).abs() <= 1.0e-7, "{tick:?}");
    }
}

#[test]
fn sneaking_clips_motion_at_each_exposed_ledge_orientation() {
    for velocity in [
        Vec3::new(0.8, 0.0, 0.0),
        Vec3::new(3.0, 0.0, 0.0),
        Vec3::new(-3.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 3.0),
        Vec3::new(0.0, 0.0, -3.0),
        Vec3::new(3.0, 0.0, 3.0),
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
    assert!((stepped.movement.z - 0.4).abs() <= 1.0e-7);
    assert!(stepped.on_ground);
    assert!(state.on_ground);
    assert!((stepped.velocity.y + 0.0784).abs() <= 1.0e-7);
    assert!((state.velocity.y + 0.0784).abs() <= 1.0e-7);

    let settled = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(settled.on_ground);
    assert_eq!(settled.movement.y, 0.0);
    assert!((settled.velocity.y + 0.0784).abs() <= 1.0e-7);

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

/// Vanilla's maximum step height starts at 0.5625, below this obstacle's top.
#[test]
fn a_point_five_eight_step_is_too_high() {
    let mut world = TerrainWorld::floor(Vec3::new(-8.0, -1.0, -8.0), Vec3::new(8.0, 0.0, 8.0));
    world.boxes.push(Aabb::new(
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 0.58, 4.0),
    ));
    let mut state = PlayerState::new(Vec3::new(0.5, 0.0, 0.5));
    state.on_ground = true;
    state.velocity.z = 0.4;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert_eq!(state.position.y, 0.0);
    assert!(tick.collisions.z);
}

/// Current sneak height is f32 1.49, so a 1.495-high passage admits it.
#[test]
fn sneaking_fits_below_a_one_point_four_nine_five_ceiling() {
    let mut world = TerrainWorld::floor(Vec3::new(-8.0, -1.0, -8.0), Vec3::new(8.0, 0.0, 8.0));
    world.boxes.push(Aabb::new(
        Vec3::new(0.0, 1.495, 1.0),
        Vec3::new(1.0, 3.0, 4.0),
    ));
    let mut state = PlayerState::new(Vec3::new(0.5, 0.0, 0.5));
    state.on_ground = true;
    state.velocity.z = 0.4;
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                sneaking: true,
                ..Default::default()
            },
            &world,
        )
        .unwrap();
    assert!(state.position.z > 0.8);
    assert!(!tick.collisions.z);
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
    for position in [
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-15.5, 2.0, 15.5),
        Vec3::new(31.25, -3.0, -31.25),
    ] {
        for velocity in [
            Vec3::ZERO,
            Vec3::new(0.25, -0.25, 0.25),
            Vec3::new(-0.5, 0.5, -0.5),
        ] {
            for (strafe, forward) in [(-1.0, 1.0), (0.0, 0.0), (1.0, -1.0)] {
                for yaw_degrees in [-720.0, 0.0, 359.0] {
                    let world = TerrainWorld {
                        fail_after: Some(0),
                        ..TerrainWorld::default()
                    };
                    let mut state = PlayerState::new(position);
                    state.velocity = velocity;
                    let before = state.clone();
                    let before_bytes = serde_json::to_vec(&before).unwrap();
                    let input = MovementInput {
                        strafe,
                        forward,
                        yaw_degrees,
                        jumping: true,
                        jump_pressed: true,
                        sprinting: true,
                        sneaking: true,
                        move_vector_is_raw: false,
                        using_consumable: true,
                        item_use_movement_modifier: None,
                        movement_speed: None,
                        effects: sim::MovementEffects::default(),
                        ..MovementInput::default()
                    };
                    assert!(matches!(
                        simulator.tick(&mut state, input, &world),
                        Err(sim::SimulationError::World(_))
                    ));
                    assert_eq!(serde_json::to_vec(&state).unwrap(), before_bytes);
                    assert_eq!(state, before);
                    assert_eq!(world.queries.get(), 1);
                }
            }
        }
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

/// An obstruction met only on the raised step path must block the step, not be tunnelled.
#[test]
fn a_step_cannot_tunnel_through_an_obstruction_on_its_raised_path() {
    let mut world = TerrainWorld::floor(Vec3::new(-4.0, 0.0, -4.0), Vec3::new(4.0, 1.0, 4.0));
    world.boxes.extend([
        Aabb::new(Vec3::new(-1.0, 1.0, 0.7), Vec3::new(1.0, 1.5, 3.0)),
        Aabb::new(Vec3::new(-1.0, 3.0, 0.7), Vec3::new(1.0, 3.2, 0.8)),
    ]);
    let mut state = grounded(Vec3::new(0.0, 1.0, 0.4));
    state.velocity.z = 1.0;
    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert_eq!(result.movement.y, 0.0, "{:?}", result.movement);
}

/// Climbing reads only the feet cell; a vine the box merely overlaps must not hold a sneaking faller.
#[test]
fn sneaking_beside_a_climbable_cell_keeps_falling() {
    let mut world = TerrainWorld::default();
    world.facts.insert(
        [1, 4, 0],
        BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 0.0,
            flags: BlockPhysicsFlags::CLIMBABLE,
            surface_response: SurfaceResponse::None,
        },
    );
    // Feet cell is [0, 4, 0]; the 0.6-wide box reaches x = 1.05, into the vine cell.
    let mut state = PlayerState::new(Vec3::new(0.75, 4.2, 0.5));
    state.velocity.y = -0.3;
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
    assert!(!tick.environment.on_climbable);
    assert_eq!(tick.movement.y as f32, -0.3_f32);
}

fn climbable_at(world: &mut TerrainWorld, block: [i32; 3]) {
    world.facts.insert(
        block,
        BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 0.0,
            flags: BlockPhysicsFlags::CLIMBABLE,
            surface_response: SurfaceResponse::None,
        },
    );
}

/// A ladder the box only overlaps leaves an ordinary ground jump.
#[test]
fn ladder_beside_the_feet_cell_does_not_replace_the_ground_jump() {
    let mut world = TerrainWorld::floor(Vec3::new(-8.0, 0.0, -8.0), Vec3::new(8.0, 1.0, 8.0));
    climbable_at(&mut world, [1, 1, 0]);
    let mut state = grounded(Vec3::new(0.75, 1.0, 0.5));
    let output = Simulator::default()
        .tick_with_controls(
            &mut state,
            MovementInput {
                jumping: true,
                jump_pressed: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(output.jump_initiated);
    assert!(!output.tick_result.environment.on_climbable);
    assert_eq!(output.tick_result.movement.y as f32, 0.42_f32);
}

/// Jumping at a ladder base climbs: no ground jump, sprint impulse or jump delay.
#[test]
fn jump_at_a_ladder_base_climbs_without_a_ground_jump() {
    let mut world = TerrainWorld::floor(Vec3::new(-8.0, 0.0, -8.0), Vec3::new(8.0, 1.0, 8.0));
    climbable_at(&mut world, [0, 1, 0]);
    let input = MovementInput {
        forward: 1.0,
        sprinting: true,
        jumping: true,
        jump_pressed: true,
        ..MovementInput::default()
    };
    let mut state = grounded(Vec3::new(0.5, 1.0, 0.5));
    let output = Simulator::default()
        .tick_with_controls(&mut state, input, &world)
        .unwrap();
    assert!(!output.jump_initiated);
    assert!(output.tick_result.environment.on_climbable);
    assert_eq!(output.tick_result.movement.y as f32, 0.2_f32);
    assert_eq!(state.jump_delay, 0);

    let mut walking = grounded(Vec3::new(0.5, 1.0, 0.5));
    let walked = Simulator::default()
        .tick(
            &mut walking,
            MovementInput {
                jumping: false,
                jump_pressed: false,
                ..input
            },
            &world,
        )
        .unwrap();
    assert_eq!(output.tick_result.movement.z, walked.movement.z);
}

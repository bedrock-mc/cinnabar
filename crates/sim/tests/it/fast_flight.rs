use std::cell::Cell;

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, MovementMode, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

/// Air with an optional wall, inside-block effect and counter for body-effect reads.
#[derive(Default)]
struct FastFlightWorld {
    block_reads: Cell<usize>,
    wall: Option<Aabb>,
    effect: Option<([i32; 3], BlockPhysicsFacts)>,
}

impl CollisionWorld for FastFlightWorld {
    /// Supplies the wall only when the full collision sweep intersects it.
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.wall
                .filter(|wall| wall.intersects(query))
                .into_iter()
                .collect(),
        ))
    }

    /// Counts reads and supplies an optional inside-block effect at one cell.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.block_reads.set(self.block_reads.get() + 1);
        let air = BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 0.0,
            flags: BlockPhysicsFlags::PASSABLE,
            surface_response: SurfaceResponse::None,
        };
        Ok(BlockPhysicsSample {
            layers: Box::new([self
                .effect
                .filter(|(cell, _)| *cell == block)
                .map_or(air, |(_, facts)| facts)]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

#[test]
fn cancelling_fast_diagonal_flight_completes_the_first_walking_tick() {
    let world = FastFlightWorld::default();
    let mut state = PlayerState::new(Vec3::new(0.5, 100.0, 0.5));
    // 500 blocks/s at 20 Hz, split across two horizontal axes.
    let diagonal = 25.0 / std::f64::consts::SQRT_2;
    state.velocity = Vec3::new(diagonal, 0.0, diagonal);
    let flying = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Flying,
                strafe: 1.0,
                forward: 1.0,
                fly_speed: Some(0.0),
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(flying.velocity.x > 16.0 && flying.velocity.z > 16.0);

    world.block_reads.set(0);
    let walking = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert_eq!(walking.tick, flying.tick + 1);
    assert!(walking.position.x > flying.position.x + 16.0);
    assert!(walking.position.z > flying.position.z + 16.0);
    assert!(walking.velocity.y < 0.0);
    assert!(world.block_reads.get() <= sim::MAX_BLOCK_SAMPLES_PER_TICK);
}

#[test]
fn every_movement_mode_keeps_body_sampling_independent_of_speed() {
    for mode in [
        MovementMode::Walking,
        MovementMode::Swimming,
        MovementMode::Crawling,
        MovementMode::Gliding,
        MovementMode::Flying,
        MovementMode::Riding,
    ] {
        let world = FastFlightWorld::default();
        let mut state = PlayerState::new(Vec3::new(0.5, 100.0, 0.5));
        state.velocity = Vec3::new(17.0, 0.0, 17.0);
        let result = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    mode,
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        assert_eq!(result.tick, 1, "{mode:?}");
        assert!(world.block_reads.get() <= sim::MAX_BLOCK_SAMPLES_PER_TICK);
        if mode == MovementMode::Riding {
            assert_eq!(result.movement, Vec3::ZERO);
        } else {
            assert!(result.movement.x > 10.0 && result.movement.z > 10.0);
        }
    }
}

#[test]
fn inside_block_effects_ignore_cells_crossed_between_body_positions() {
    for mode in [
        MovementMode::Walking,
        MovementMode::Swimming,
        MovementMode::Crawling,
        MovementMode::Gliding,
        MovementMode::Flying,
    ] {
        for (flags, surface_response) in [
            (BlockPhysicsFlags::COBWEB, SurfaceResponse::None),
            (BlockPhysicsFlags::POWDER_SNOW, SurfaceResponse::None),
            (BlockPhysicsFlags::PASSABLE, SurfaceResponse::Honey),
        ] {
            let world = FastFlightWorld {
                effect: Some((
                    [8, 100, 8],
                    BlockPhysicsFacts {
                        friction: 0.6,
                        horizontal_speed_factor: 0.25,
                        vertical_speed_factor: 0.05,
                        fluid_height_blocks: 0.0,
                        flags,
                        surface_response,
                    },
                )),
                ..FastFlightWorld::default()
            };
            let mut state = PlayerState::new(Vec3::new(0.5, 100.0, 0.5));
            state.velocity = Vec3::new(17.0, 0.0, 17.0);
            let mut baseline = state.clone();
            let input = MovementInput {
                mode,
                ..MovementInput::default()
            };
            let expected = Simulator::default()
                .tick(&mut baseline, input, &FastFlightWorld::default())
                .unwrap();
            let result = Simulator::default()
                .tick(&mut state, input, &world)
                .unwrap();
            assert_eq!(result, expected);
            assert!(world.block_reads.get() <= sim::MAX_BLOCK_SAMPLES_PER_TICK);
        }
    }
}

#[test]
fn fast_motion_applies_honey_at_the_destination_body() {
    for mode in [
        MovementMode::Walking,
        MovementMode::Swimming,
        MovementMode::Crawling,
        MovementMode::Gliding,
        MovementMode::Flying,
    ] {
        let mut initial = PlayerState::new(Vec3::new(0.5, 100.0, 0.5));
        initial.velocity = Vec3::new(17.0, 0.0, 17.0);
        let input = MovementInput {
            mode,
            ..MovementInput::default()
        };
        let mut baseline = initial.clone();
        let expected = Simulator::default()
            .tick(&mut baseline, input, &FastFlightWorld::default())
            .unwrap();
        let destination = [
            expected.position.x,
            expected.position.y,
            expected.position.z,
        ]
        .map(|axis| axis.floor() as i32);
        let world = FastFlightWorld {
            effect: Some((
                destination,
                BlockPhysicsFacts {
                    friction: 0.6,
                    horizontal_speed_factor: 1.0,
                    vertical_speed_factor: 1.0,
                    fluid_height_blocks: 0.0,
                    flags: BlockPhysicsFlags::PASSABLE,
                    surface_response: SurfaceResponse::Honey,
                },
            )),
            ..FastFlightWorld::default()
        };
        let result = Simulator::default()
            .tick(&mut initial, input, &world)
            .unwrap();
        assert_eq!(result.movement, expected.movement, "{mode:?}");
        assert_eq!(
            result.velocity.x,
            f64::from(expected.velocity.x as f32 * 0.4),
            "{mode:?}"
        );
        assert_eq!(
            result.velocity.z,
            f64::from(expected.velocity.z as f32 * 0.4),
            "{mode:?}"
        );
        assert!(world.block_reads.get() <= sim::MAX_BLOCK_SAMPLES_PER_TICK);
    }
}

#[test]
fn high_ability_flight_keeps_advancing_without_exhausting_body_samples() {
    let world = FastFlightWorld::default();
    let mut state = PlayerState::new(Vec3::new(0.5, 100.0, 0.5));
    let input = MovementInput {
        mode: MovementMode::Flying,
        creative_flight: true,
        strafe: 1.0,
        forward: 1.0,
        sprinting: true,
        jumping: true,
        fly_speed: Some(5.0),
        vertical_fly_speed: Some(100.0),
        ..MovementInput::default()
    };
    for _ in 0..120 {
        world.block_reads.set(0);
        let before = state.position;
        let result = Simulator::default()
            .tick(&mut state, input, &world)
            .unwrap();
        assert!(result.position.x > before.x);
        assert!(result.position.y > before.y);
        assert!(result.position.z > before.z);
        assert!(world.block_reads.get() <= sim::MAX_BLOCK_SAMPLES_PER_TICK);
    }
    assert!(state.velocity.horizontal_length_squared() > 90.0 * 90.0);
}

#[test]
fn fast_server_motion_checks_the_full_solid_collision_path() {
    for wall in [
        None,
        Some(Aabb::new(
            Vec3::new(-10.0, 0.0, 10.0),
            Vec3::new(10.0, 200.0, 11.0),
        )),
    ] {
        let world = FastFlightWorld {
            wall,
            ..FastFlightWorld::default()
        };
        let mut state = PlayerState::new(Vec3::new(0.5, 100.0, 0.5));
        // Actor motion is expressed in blocks per tick: 500 blocks/s at 20 Hz.
        state.velocity.y = 25.0;
        state.velocity.z = 25.0;
        let result = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    mode: MovementMode::Flying,
                    forward: 1.0,
                    jumping: true,
                    fly_speed: Some(0.0),
                    vertical_fly_speed: Some(0.0),
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        assert_eq!(result.movement.y, 25.0);
        if wall.is_some() {
            assert!(result.collisions.z);
            assert!(result.position.z < 10.0);
            assert_eq!(result.velocity.z, 0.0);
        } else {
            assert_eq!(result.movement.z, 25.0);
            assert_eq!(result.position.z, 25.5);
        }
        assert!(world.block_reads.get() <= sim::MAX_BLOCK_SAMPLES_PER_TICK);
    }
}

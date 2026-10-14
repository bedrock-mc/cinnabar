use std::cell::Cell;

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, MovementMode, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

/// Empty air with an optional wall and a counter for body-effect reads.
#[derive(Default)]
struct FastFlightWorld {
    block_reads: Cell<usize>,
    wall: Option<Aabb>,
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

    /// Counts reads while keeping every sampled cell free of block effects.
    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.block_reads.set(self.block_reads.get() + 1);
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: BlockPhysicsFlags::PASSABLE,
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
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

use super::*;
use crate::{
    BlockPhysicsFacts, BlockPhysicsSample, CollisionQuery, ProvenancedCollider, SurfaceResponse,
};

/// A test world: still water at `water_y..=surface_y`, a floor below, optional ground
/// friction and walls.
struct Pool {
    floor_y: i32,
    surface_y: i32,
    ground_friction: f64,
    wall_x: Option<i32>,
    /// The boat's origin column has not streamed in.
    unloaded_origin: bool,
}

impl Pool {
    fn water() -> Self {
        Self {
            floor_y: 58,
            surface_y: 62,
            ground_friction: 0.6,
            wall_x: None,
            unloaded_origin: false,
        }
    }

    fn land(friction: f64) -> Self {
        Self {
            floor_y: 62,
            surface_y: 61,
            ground_friction: friction,
            wall_x: None,
            unloaded_origin: false,
        }
    }

    fn solid(&self, block: [i32; 3]) -> bool {
        block[1] <= self.floor_y || self.wall_x.is_some_and(|x| block[0] >= x)
    }

    fn water_at(&self, block: [i32; 3]) -> bool {
        !self.solid(block) && block[1] <= self.surface_y
    }
}

impl CollisionWorld for Pool {
    fn ensure_movement_origin(&self, feet: Vec3) -> Result<(), WorldQueryError> {
        if self.unloaded_origin {
            return Err(WorldQueryError::UnloadedChunk(world::ChunkKey {
                dimension: 0,
                x: (feet.x.floor() as i32) >> 4,
                z: (feet.z.floor() as i32) >> 4,
            }));
        }
        Ok(())
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.collision_boxes_with_provenance(query)?
                .value
                .into_iter()
                .map(|shape| shape.aabb)
                .collect(),
        ))
    }

    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        let mut shapes = Vec::new();
        for x in query.min.x.floor() as i32..=query.max.x.floor() as i32 {
            for y in query.min.y.floor() as i32..=query.max.y.floor() as i32 {
                for z in query.min.z.floor() as i32..=query.max.z.floor() as i32 {
                    if self.solid([x, y, z]) {
                        let min = Vec3::new(f64::from(x), f64::from(y), f64::from(z));
                        shapes.push(ProvenancedCollider {
                            aabb: Aabb::new(min, min + Vec3::ONE),
                            block: Some([x, y, z]),
                            runtime_id: None,
                            cell_has_scaffolding: None,
                        });
                    }
                }
            }
        }
        Ok(CollisionQuery::synthetic(shapes))
    }

    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(Some(CollisionQuery::synthetic(
            !self.solid(block) && !self.water_at(block),
        )))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let water = self.water_at(block);
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: if self.solid(block) {
                    self.ground_friction
                } else {
                    0.6
                },
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if water { 8.0 / 9.0 } else { 0.0 },
                flags: if water {
                    BlockPhysicsFlags::WATER
                } else {
                    BlockPhysicsFlags::default()
                },
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

/// An oak boat's collision box and origin, as its entity facts give them.
const OAK: BoatBody = BoatBody {
    width: 1.4,
    height: 0.455,
    height_offset: 0.375,
};
const FORWARD: [f32; 2] = [0.0, 1.0];
const CALM: BoatBuoyancy = BoatBuoyancy::Live { wave_draw: 0.5 };

fn run(world: &Pool, mut state: BoatState, move_vector: [f32; 2], ticks: usize) -> BoatState {
    for _ in 0..ticks {
        state = tick_boat(world, &state, boat_paddle_power(move_vector), CALM)
            .unwrap()
            .state;
    }
    state
}

fn horizontal_speed(state: &BoatState) -> f32 {
    state.velocity[0].hypot(state.velocity[2])
}

#[test]
fn steering_input_drives_the_paddle_on_the_outside_of_the_turn() {
    assert_eq!(boat_paddle_power([0.0, 1.0]), [1.0, 1.0]);
    // A move vector's x is positive to the left: turning right rows only the left paddle.
    assert_eq!(boat_paddle_power([-1.0, 0.0]), [1.0, 0.0]);
    assert_eq!(boat_paddle_power([1.0, 0.0]), [0.0, 1.0]);
    assert_eq!(boat_paddle_power([0.0, -1.0]), [-0.15, -0.15]);
    assert_eq!(boat_paddle_power([0.0, 0.0]), [0.0, 0.0]);
    // Reversing mirrors the steering.
    let [left, right] = boat_paddle_power([-1.0, -1.0]);
    assert_eq!(left, 0.0);
    assert!((right + 2.0_f32.sqrt() * 0.15).abs() < 1.0e-6);
}

#[test]
fn a_stroke_starts_at_full_force_and_weakens_until_the_next() {
    // A resting paddle after a long time afloat.
    let mut paddles = BoatPaddles {
        clock: 100,
        stroke_start: [-1; 2],
        force: [0.0; 2],
    };
    let mut forces = Vec::new();
    for _ in 0..21 {
        drive_paddles(&mut paddles, [1.0, 0.5]);
        forces.push(paddles.force);
    }
    assert_eq!(forces[0], [3.0, 1.5]);
    for force in &forces[1..10] {
        assert_eq!(*force, [2.9, 1.4]);
    }
    assert_eq!(forces[10], [3.0, 1.5]);
    assert_eq!(forces[20], [3.0, 1.5]);
    drive_paddles(&mut paddles, [-1.0, 0.0]);
    assert_eq!(paddles.force, [-2.9, 0.0]);
}

#[test]
fn rowing_forward_on_still_water_reaches_cruising_speed_along_the_boat_axis() {
    let world = Pool::water();
    let state = run(
        &world,
        BoatState::at_rest(OAK, [0.5, 62.6, 0.5], 0.0),
        FORWARD,
        200,
    );
    // Water momentum applies before and after each push: v = (0.9 v + push) 0.9.
    let speed = horizontal_speed(&state);
    assert!((0.37..0.4).contains(&speed), "cruising speed {speed}");
    assert!(state.position[0] > 50.0);
    assert!((state.position[2] - 0.5).abs() < 1.0e-3);
    assert_eq!(state.yaw, 0.0);

    let state = run(
        &world,
        BoatState::at_rest(OAK, [0.5, 62.6, 0.5], 90.0),
        FORWARD,
        50,
    );
    assert!(state.position[2] > 5.0);
    assert!((state.position[0] - 0.5).abs() < 1.0e-3);
}

#[test]
fn a_floating_boat_settles_in_the_surface_block() {
    let world = Pool::water();
    let mut state = BoatState::at_rest(OAK, [0.5, 62.0, 0.5], 0.0);
    let mut float = BoatFloat::default();
    for _ in 0..400 {
        let tick = tick_boat(&world, &state, [0.0; 2], CALM).unwrap();
        state = tick.state;
        float = tick.float;
    }
    assert!(float.can_float);
    assert!((62.0..63.0).contains(&state.position[1]), "{state:?}");
    assert!(state.velocity[1].abs() < 0.02);
}

#[test]
fn a_submerged_boat_resurfaces() {
    let world = Pool::water();
    let state = run(
        &world,
        BoatState::at_rest(OAK, [0.5, 60.5, 0.5], 0.0),
        [0.0; 2],
        60,
    );
    assert!(state.position[1] > 62.0, "{state:?}");
}

#[test]
fn turning_right_rotates_the_boat_clockwise() {
    let world = Pool::water();
    let state = run(
        &world,
        BoatState::at_rest(OAK, [0.5, 62.6, 0.5], 0.0),
        [-1.0, 0.0],
        40,
    );
    assert!(state.yaw > 30.0, "{state:?}");
    let state = run(
        &world,
        BoatState::at_rest(OAK, [0.5, 62.6, 0.5], 0.0),
        [1.0, 0.0],
        40,
    );
    assert!(state.yaw < -30.0, "{state:?}");
}

#[test]
fn a_dropped_boat_falls_and_lands() {
    let world = Pool::land(0.6);
    let start = BoatState::at_rest(OAK, [0.5, 66.375, 0.5], 0.0);
    let first = tick_boat(&world, &start, [0.0; 2], CALM).unwrap().state;
    assert!((first.velocity[1] + 0.0392).abs() < 1.0e-6);
    assert!(!first.on_ground);
    let landed = run(&world, first, [0.0; 2], 60);
    assert!(landed.on_ground);
    assert!((landed.position[1] - (63.0 + OAK.height_offset)).abs() < 1.0e-4);
}

#[test]
fn ground_friction_sets_the_momentum_on_land() {
    let slide = |friction| {
        let world = Pool::land(friction);
        let mut state = run(
            &world,
            BoatState::at_rest(OAK, [0.5, 63.375, 0.5], 0.0),
            [0.0; 2],
            2,
        );
        state.velocity = [0.5, state.velocity[1], 0.0];
        run(&world, state, [0.0; 2], 5).position[0]
    };
    // Ice keeps the boat sliding far further than dirt.
    assert!(slide(0.98) > slide(0.6) + 1.0);
}

#[test]
fn a_wall_stops_the_boat_and_clears_its_speed_on_that_axis() {
    let world = Pool {
        wall_x: Some(4),
        ..Pool::water()
    };
    let mut state = BoatState::at_rest(OAK, [0.5, 62.6, 0.5], 0.0);
    let mut collided = false;
    for _ in 0..60 {
        let tick = tick_boat(&world, &state, boat_paddle_power(FORWARD), CALM).unwrap();
        collided |= tick.horizontal_collision;
        state = tick.state;
    }
    assert!(collided);
    assert!(state.position[0] <= 4.0 - OAK.width * 0.5 + 1.0e-4);
}

#[test]
fn a_replayed_tick_keeps_the_recorded_float_and_wave() {
    let world = Pool::water();
    let state = run(
        &world,
        BoatState::at_rest(OAK, [0.5, 62.6, 0.5], 0.0),
        FORWARD,
        20,
    );
    let live = tick_boat(
        &world,
        &state,
        boat_paddle_power(FORWARD),
        BoatBuoyancy::Live { wave_draw: 0.01 },
    )
    .unwrap();
    assert!(live.state.wave_timer > state.wave_timer);
    let replayed = tick_boat(
        &world,
        &state,
        boat_paddle_power(FORWARD),
        BoatBuoyancy::Replay(live.float),
    )
    .unwrap();
    assert_eq!(replayed.float, live.float);
    assert_eq!(replayed.state.wave_timer, state.wave_timer);
    assert_eq!(replayed.state.position, live.state.position);
}

#[test]
fn an_unloaded_origin_pauses_the_boat_instead_of_moving_it() {
    let world = Pool {
        unloaded_origin: true,
        ..Pool::water()
    };
    let state = BoatState::at_rest(OAK, [0.5, 62.6, 0.5], 0.0);
    assert!(matches!(
        tick_boat(&world, &state, boat_paddle_power(FORWARD), CALM),
        Err(WorldQueryError::UnloadedChunk(_))
    ));
}

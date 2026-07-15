use sim::{
    Aabb, CollisionWorld, MovementInput, PlayerState, SimulationError, Simulator, TICKS_PER_SECOND,
    Vec3, WorldQueryError,
};

#[derive(Default)]
struct StaticWorld {
    boxes: Vec<Aabb>,
    fail: Option<WorldQueryError>,
}

impl StaticWorld {
    fn floor() -> Self {
        Self {
            boxes: vec![Aabb::new(
                Vec3::new(-16.0, 0.0, -16.0),
                Vec3::new(16.0, 1.0, 16.0),
            )],
            fail: None,
        }
    }
}

impl CollisionWorld for StaticWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<Vec<Aabb>, WorldQueryError> {
        if let Some(error) = &self.fail {
            return Err(error.clone());
        }
        Ok(self
            .boxes
            .iter()
            .copied()
            .filter(|aabb| aabb.intersects(query))
            .collect())
    }
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1.0e-10,
        "{actual} != {expected}"
    );
}

fn grounded_state(position: Vec3) -> PlayerState {
    let mut state = PlayerState::new(position);
    state.on_ground = true;
    state
}

#[test]
fn one_call_is_exactly_one_bedrock_tick_and_ground_gravity_is_post_move() {
    let world = StaticWorld::floor();
    let mut state = grounded_state(Vec3::new(0.0, 1.0, 0.0));

    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();

    assert_eq!(TICKS_PER_SECOND, 20);
    assert_eq!(state.tick, 1);
    assert_eq!(result.movement, Vec3::ZERO);
    assert_eq!(state.position, Vec3::new(0.0, 1.0, 0.0));
    assert_close(state.velocity.y, -0.0784);
    assert!(state.on_ground);
}

#[test]
fn ground_acceleration_and_drag_match_bedsim_constants() {
    let world = StaticWorld::floor();
    let mut state = grounded_state(Vec3::new(0.0, 1.0, 0.0));
    let input = MovementInput {
        forward: 1.0,
        ..MovementInput::default()
    };

    let result = Simulator::default()
        .tick(&mut state, input, &world)
        .unwrap();

    assert_close(result.movement.z, 0.098_000_014_449_718_6);
    assert_close(state.position.z, 0.098_000_014_449_718_6);
    assert_close(state.velocity.z, 0.053_508_007_889_546_3);
    assert_close(state.velocity.y, -0.0784);
}

#[test]
fn sprint_jump_applies_vertical_and_yaw_forward_impulses_before_collision() {
    let world = StaticWorld::floor();
    let mut state = grounded_state(Vec3::new(0.0, 1.0, 0.0));
    let input = MovementInput {
        jumping: true,
        jump_pressed: true,
        sprinting: true,
        ..MovementInput::default()
    };

    let result = Simulator::default()
        .tick(&mut state, input, &world)
        .unwrap();

    assert_close(result.movement.y, 0.42);
    assert_close(result.movement.z, 0.2);
    assert_close(state.position.y, 1.42);
    assert_close(state.velocity.y, 0.3332);
    assert_close(state.velocity.z, 0.1092);
    assert!(!state.on_ground);
    assert_eq!(state.jump_delay, 9);
}

#[test]
fn collision_resolves_y_then_x_then_z_and_reports_each_clipped_axis() {
    let mut world = StaticWorld::floor();
    world.boxes.push(Aabb::new(
        Vec3::new(1.0, 0.0, -16.0),
        Vec3::new(2.0, 3.0, 16.0),
    ));
    let mut state = grounded_state(Vec3::new(0.5, 1.0, 0.0));
    state.velocity = Vec3::new(1.0, -0.2, 0.25);

    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();

    assert_close(result.movement.x, 0.2001);
    assert_close(result.movement.y, 0.0);
    assert_close(result.movement.z, 0.25);
    assert!(result.collisions.x);
    assert!(result.collisions.y);
    assert!(!result.collisions.z);
    assert_close(state.velocity.x, 0.0);
}

#[test]
fn grounded_horizontal_collision_steps_only_when_the_point_six_path_is_farther() {
    let mut world = StaticWorld::floor();
    world.boxes.push(Aabb::new(
        Vec3::new(-1.0, 1.0, 1.0),
        Vec3::new(1.0, 1.5, 2.0),
    ));
    let mut state = grounded_state(Vec3::new(0.0, 1.0, 0.5));
    state.velocity = Vec3::new(0.0, 0.0, 0.4);

    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();

    assert_close(result.movement.y, 0.5);
    assert_close(result.movement.z, 0.4);
    assert_close(state.position.y, 1.5);
    assert_close(state.position.z, 0.9);
}

#[test]
fn world_query_failure_is_transactional_and_does_not_advance_tick() {
    let world = StaticWorld {
        boxes: Vec::new(),
        fail: Some(WorldQueryError::UnloadedChunk(world::ChunkKey::new(
            0, 2, 3,
        ))),
    };
    let mut state = grounded_state(Vec3::new(32.0, 1.0, 48.0));
    let before = state.clone();

    assert_eq!(
        Simulator::default().tick(&mut state, MovementInput::default(), &world),
        Err(SimulationError::World(WorldQueryError::UnloadedChunk(
            world::ChunkKey::new(0, 2, 3)
        )))
    );
    assert_eq!(state, before);
}

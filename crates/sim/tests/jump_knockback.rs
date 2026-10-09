use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementInput, PlayerState, Simulator, Vec3,
    WorldQueryError,
};

struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-16.0, 0.0, -16.0), Vec3::new(16.0, 1.0, 16.0));
        Ok(CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

/// Starts a player at the requested ground state with an incoming velocity.
fn state(grounded: bool, height: f64, velocity: Vec3) -> PlayerState {
    let mut state = PlayerState::new(Vec3::new(0.0, height, 0.0));
    state.on_ground = grounded;
    state.velocity = velocity;
    state
}

/// Requests one ordinary jump without other movement controls.
fn jump() -> MovementInput {
    MovementInput {
        jumping: true,
        jump_pressed: true,
        ..MovementInput::default()
    }
}

#[test]
fn grounded_jump_raises_low_vertical_motion_without_canceling_horizontal_motion() {
    let mut jumping = state(true, 1.0, Vec3::new(0.45, 0.1, -0.35));
    let mut ordinary = jumping.clone();
    let simulator = Simulator::default();
    let jumped = simulator
        .tick_with_controls(&mut jumping, jump(), &Floor)
        .unwrap();
    let plain = simulator
        .tick(&mut ordinary, MovementInput::default(), &Floor)
        .unwrap();

    assert!(jumped.jump_initiated);
    assert!(jumped.tick_result.movement.y > plain.movement.y);
    assert_eq!(jumped.tick_result.movement.x, plain.movement.x);
    assert_eq!(jumped.tick_result.movement.z, plain.movement.z);
    assert_eq!(jumping.velocity.x, ordinary.velocity.x);
    assert_eq!(jumping.velocity.z, ordinary.velocity.z);
    assert_ne!(jumping.velocity.x, 0.0);
    assert_ne!(jumping.velocity.z, 0.0);
}

#[test]
fn grounded_jump_preserves_vertical_motion_above_the_jump_impulse() {
    let mut jumping = state(true, 1.0, Vec3::new(0.45, 0.6, -0.35));
    let mut ordinary = jumping.clone();
    let simulator = Simulator::default();
    let jumped = simulator
        .tick_with_controls(&mut jumping, jump(), &Floor)
        .unwrap();
    let plain = simulator
        .tick(&mut ordinary, MovementInput::default(), &Floor)
        .unwrap();

    assert!(jumped.jump_initiated);
    assert_eq!(jumped.tick_result.movement, plain.movement);
    assert_eq!(jumping.velocity, ordinary.velocity);
}

#[test]
fn sprint_jump_can_counter_opposing_horizontal_motion() {
    let simulator = Simulator::default();
    let input = MovementInput {
        sprinting: true,
        yaw_degrees: 45.0,
        ..jump()
    };
    let mut standing = state(true, 1.0, Vec3::ZERO);
    let takeoff = simulator.tick(&mut standing, input, &Floor).unwrap();
    let mut opposed = state(
        true,
        1.0,
        Vec3::new(-takeoff.movement.x, 0.1, -takeoff.movement.z),
    );
    let countered = simulator
        .tick_with_controls(&mut opposed, input, &Floor)
        .unwrap();

    assert!(countered.jump_initiated);
    assert!(countered.tick_result.movement.y > 0.0);
    assert_eq!(countered.tick_result.movement.x, 0.0);
    assert_eq!(countered.tick_result.movement.z, 0.0);
}

#[test]
fn near_ground_jump_request_waits_for_ground_contact() {
    let mut falling = state(false, 1.05, Vec3::new(0.45, -0.1, -0.35));
    let simulator = Simulator::default();
    let landed = simulator
        .tick_with_controls(&mut falling, jump(), &Floor)
        .unwrap();

    assert!(!landed.jump_initiated);
    assert!(falling.on_ground);
    assert!(landed.tick_result.movement.y < 0.0);
    let jumped = simulator
        .tick_with_controls(&mut falling, jump(), &Floor)
        .unwrap();
    assert!(jumped.jump_initiated);
    assert!(jumped.tick_result.movement.y > 0.0);
}

#[test]
fn takeoff_tick_retains_ground_drag_on_horizontal_knockback() {
    let mut grounded = state(true, 1.0, Vec3::new(0.45, 0.1, -0.35));
    let mut airborne = state(false, 1.0, grounded.velocity);
    let simulator = Simulator::default();
    simulator.tick(&mut grounded, jump(), &Floor).unwrap();
    simulator.tick(&mut airborne, jump(), &Floor).unwrap();

    assert!(!grounded.on_ground);
    assert!(!airborne.on_ground);
    assert!(grounded.velocity.x.abs() < airborne.velocity.x.abs());
    assert!(grounded.velocity.z.abs() < airborne.velocity.z.abs());
}

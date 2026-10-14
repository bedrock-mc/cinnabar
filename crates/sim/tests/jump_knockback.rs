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

/// A skipped travel tick still initiates a valid jump and leaves its motion for the next tick.
#[test]
fn skipped_travel_retains_ground_jump_motion_without_moving() {
    let simulator = Simulator::default();
    let mut ordinary = state(true, 1.0, Vec3::ZERO);
    let normal = simulator
        .tick_with_controls(&mut ordinary, jump(), &Floor)
        .unwrap();
    let mut teleported = state(true, 1.0, Vec3::ZERO);
    let target = teleported.position;
    let skipped = simulator
        .tick_with_controls(
            &mut teleported,
            MovementInput {
                skip_travel: true,
                ..jump()
            },
            &Floor,
        )
        .unwrap();
    assert!(skipped.jump_initiated);
    assert_eq!(skipped.tick_result.position, target);
    assert_eq!(skipped.tick_result.movement, Vec3::ZERO);
    assert_eq!(skipped.tick_result.velocity, normal.tick_result.movement);
    assert_eq!(teleported.jump_delay, ordinary.jump_delay);
}

/// Travel suppression decrements an active jump cooldown without resetting it or damping impulses.
#[test]
fn skipped_travel_keeps_jump_cooldown_and_independent_motion() {
    let simulator = Simulator::default();
    let mut teleported = state(true, 1.0, Vec3::new(0.5, 0.2, -0.25));
    teleported.jump_delay = sim::JUMP_DELAY_TICKS;
    let target = teleported.position;
    let velocity = Vec3::new(
        f64::from(teleported.velocity.x as f32),
        f64::from(teleported.velocity.y as f32),
        f64::from(teleported.velocity.z as f32),
    );
    let skipped = simulator
        .tick_with_controls(
            &mut teleported,
            MovementInput {
                skip_travel: true,
                forward: 1.0,
                ..jump()
            },
            &Floor,
        )
        .unwrap();
    assert!(!skipped.jump_initiated);
    assert_eq!(skipped.tick_result.position, target);
    assert_eq!(skipped.tick_result.velocity, velocity);
    assert_eq!(teleported.jump_delay, sim::JUMP_DELAY_TICKS - 1);
}

/// Spectator flight also holds its teleport destination for the skipped travel tick.
#[test]
fn skipped_spectator_travel_does_not_apply_flight_controls() {
    let mut teleported = state(false, 10.0, Vec3::ZERO);
    let target = teleported.position;
    let output = Simulator::default()
        .tick(
            &mut teleported,
            MovementInput {
                skip_travel: true,
                spectator: true,
                mode: sim::MovementMode::Flying,
                forward: 1.0,
                ..jump()
            },
            &Floor,
        )
        .unwrap();
    assert_eq!(output.position, target);
    assert_eq!(output.movement, Vec3::ZERO);
    assert_eq!(output.velocity, Vec3::ZERO);
}

/// Travel suppression keeps flight steering at its ordinary strength while descent is held.
#[test]
fn skipped_flight_travel_preserves_controls_while_descending() {
    let simulator = Simulator::default();
    let input = MovementInput {
        mode: sim::MovementMode::Flying,
        forward: 1.0,
        sneaking: true,
        ..Default::default()
    };
    let mut ordinary = state(false, 10.0, Vec3::ZERO);
    let expected = simulator
        .tick_with_controls(&mut ordinary, input, &Floor)
        .unwrap();
    let mut teleported = state(false, 10.0, Vec3::ZERO);
    let target = teleported.position;
    let skipped = simulator
        .tick_with_controls(
            &mut teleported,
            MovementInput {
                skip_travel: true,
                ..input
            },
            &Floor,
        )
        .unwrap();
    assert_eq!(skipped.controls, expected.controls);
    assert_eq!(skipped.tick_result.position, target);
    assert_eq!(skipped.tick_result.velocity, Vec3::ZERO);
}

/// The vehicle still owns motion and jumping during a skipped mounted tick.
#[test]
fn skipped_riding_travel_keeps_the_existing_mount_motion_contract() {
    let simulator = Simulator::default();
    let input = MovementInput {
        mode: sim::MovementMode::Riding,
        forward: 1.0,
        jumping: true,
        ..Default::default()
    };
    let mut teleported = state(false, 10.0, Vec3::new(0.2, 0.2, 0.2));
    teleported.jump_delay = sim::JUMP_DELAY_TICKS;
    let target = teleported.position;
    let mut ordinary = teleported.clone();
    let expected = simulator
        .tick_with_controls(&mut ordinary, input, &Floor)
        .unwrap();
    let skipped = simulator
        .tick_with_controls(
            &mut teleported,
            MovementInput {
                skip_travel: true,
                ..input
            },
            &Floor,
        )
        .unwrap();
    assert_eq!(skipped.tick_result.velocity, expected.tick_result.velocity);
    assert_eq!(skipped.controls, expected.controls);
    assert_eq!(skipped.tick_result.position, target);
    assert_eq!(teleported.jump_delay, ordinary.jump_delay);
    assert!(!skipped.jump_initiated);
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

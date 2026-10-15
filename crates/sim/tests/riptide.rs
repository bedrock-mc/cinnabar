use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementInput, MovementMode, PlayerState, Simulator,
    Vec3, WorldQueryError,
};

/// Empty collision space keeps the launch independent of terrain and fluid drag.
struct Air;

impl CollisionWorld for Air {
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }
}

/// A solid wall terminates a spin when its launch reaches the wall.
struct Wall;

impl CollisionWorld for Wall {
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(vec![Aabb::new(
            Vec3::new(-8.0, 0.0, 1.0),
            Vec3::new(8.0, 128.0, 2.0),
        )]))
    }
}

/// A level floor keeps downward launches grounded without a side collision.
struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(vec![Aabb::new(
            Vec3::new(-128.0, 0.0, -128.0),
            Vec3::new(128.0, 1.0, 128.0),
        )]))
    }
}

#[test]
fn a_grounded_spin_ends_on_its_sixth_tick() {
    let mut player = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    player.on_ground = true;
    let simulator = Simulator::default();
    simulator
        .tick(
            &mut player,
            MovementInput {
                riptide: sim::RiptideLaunch::new(3, [30.0, 0.0]),
                ..Default::default()
            },
            &Floor,
        )
        .unwrap();
    assert_eq!(player.spin_attack_ticks, Some(1));
    for elapsed in 2..=5 {
        simulator
            .tick(&mut player, MovementInput::default(), &Floor)
            .unwrap();
        assert!(player.on_ground);
        assert_eq!(player.spin_attack_ticks, Some(elapsed));
    }
    simulator
        .tick(&mut player, MovementInput::default(), &Floor)
        .unwrap();
    assert!(player.on_ground);
    assert_eq!(player.spin_attack_ticks, None);
}

#[test]
fn a_side_collision_ends_the_spin_on_the_launch_tick() {
    let mut player = PlayerState::new(Vec3::new(0.0, 64.0, 0.0));
    Simulator::default()
        .tick(
            &mut player,
            MovementInput {
                riptide: sim::RiptideLaunch::new(3, [0.0; 2]),
                ..Default::default()
            },
            &Wall,
        )
        .unwrap();
    assert!(player.collisions.z);
    assert!(player.movement.z < 1.0);
    assert_eq!(player.spin_attack_ticks, None);
}

#[test]
fn riding_and_spectator_requests_do_not_start_a_spin() {
    for (mode, spectator) in [(MovementMode::Riding, false), (MovementMode::Walking, true)] {
        let mut player = PlayerState::new(Vec3::new(0.0, 64.0, 0.0));
        Simulator::default()
            .tick(
                &mut player,
                MovementInput {
                    riptide: sim::RiptideLaunch::new(3, [0.0; 2]),
                    mode,
                    spectator,
                    ..Default::default()
                },
                &Air,
            )
            .unwrap();
        assert_eq!(player.spin_attack_ticks, None);
        assert_eq!(player.movement.z, 0.0);
    }
}

#[test]
fn released_riptide_adds_the_level_impulse_before_travel() {
    for (level, distance) in [(1, 1.5), (2, 2.25), (3, 3.0)] {
        let mut player = PlayerState::new(Vec3::new(0.0, 64.0, 0.0));
        player.velocity.z = 0.25;
        let result = Simulator::default()
            .tick(
                &mut player,
                MovementInput {
                    riptide: sim::RiptideLaunch::new(level, [0.0; 2]),
                    ..MovementInput::default()
                },
                &Air,
            )
            .unwrap();
        assert_eq!(result.movement.z, distance + 0.25);
        assert_eq!(player.spin_attack_ticks, Some(1));
    }
}

#[test]
fn spin_ends_after_twenty_ticks_without_repeating_the_impulse() {
    let mut player = PlayerState::new(Vec3::new(0.0, 64.0, 0.0));
    let sim = Simulator::default();
    sim.tick(
        &mut player,
        MovementInput {
            riptide: sim::RiptideLaunch::new(3, [0.0; 2]),
            ..Default::default()
        },
        &Air,
    )
    .unwrap();
    for elapsed in 2..20 {
        sim.tick(&mut player, MovementInput::default(), &Air)
            .unwrap();
        assert_eq!(player.spin_attack_ticks, Some(elapsed));
        assert!(player.movement.z < 3.0);
    }
    sim.tick(&mut player, MovementInput::default(), &Air)
        .unwrap();
    assert_eq!(player.spin_attack_ticks, None);
}

#[test]
fn unsupported_riptide_does_not_fail_a_tick() {
    let mut player = PlayerState::new(Vec3::new(0.0, 64.0, 0.0));
    let result = Simulator::default().tick(
        &mut player,
        MovementInput {
            riptide: sim::RiptideLaunch::new(u8::MAX, [0.0; 2]),
            ..Default::default()
        },
        &Air,
    );
    assert!(
        result.is_ok(),
        "unsupported launch must leave ordinary movement available: {result:?}"
    );
    assert_eq!(player.tick, 1);
    assert_eq!(player.spin_attack_ticks, None);
    assert_eq!(player.movement.z, 0.0);
}

#[test]
fn serialized_riptide_cannot_bypass_the_query_budget() {
    let mut value = serde_json::to_value(MovementInput::default()).unwrap();
    value["riptide"] = serde_json::json!({ "level": u8::MAX, "rotation": [0.0, 0.0] });
    let input = serde_json::from_value(value).unwrap();
    let mut player = PlayerState::new(Vec3::new(0.0, 64.0, 0.0));
    Simulator::default().tick(&mut player, input, &Air).unwrap();
    assert_eq!(player.tick, 1);
    assert_eq!(player.movement.z, 0.0);
    assert_eq!(player.spin_attack_ticks, None);
}

#[test]
fn supported_levels_above_three_keep_their_unclamped_impulse() {
    let mut player = PlayerState::new(Vec3::new(0.0, 64.0, 0.0));
    let input = MovementInput {
        riptide: sim::RiptideLaunch::new(4, [0.0; 2]),
        ..Default::default()
    };
    Simulator::default().tick(&mut player, input, &Air).unwrap();
    assert_eq!(player.movement.z, 3.75);
    assert_eq!(player.spin_attack_ticks, Some(1));
}

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementEffects, MovementInput, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

#[derive(Clone, Copy)]
struct SurfaceWorld {
    facts: BlockPhysicsFacts,
    floor: bool,
}

impl CollisionWorld for SurfaceWorld {
    /// Returns the homogeneous fixture floor with its explicit source material.
    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<sim::ProvenancedCollider>>, WorldQueryError> {
        let boxes = self.collision_boxes(query)?;
        Ok(CollisionQuery {
            value: boxes
                .value
                .into_iter()
                .map(|aabb| sim::ProvenancedCollider {
                    aabb,
                    block: Some([0, 0, 0]),
                    runtime_id: None,
                })
                .collect(),
            identity: boxes.identity,
        })
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-8.0, 0.0, -8.0), Vec3::new(8.0, 1.0, 8.0));
        Ok(CollisionQuery::synthetic(
            (self.floor && floor.intersects(query))
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([self.facts]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn surface(response: SurfaceResponse) -> SurfaceWorld {
    SurfaceWorld {
        facts: BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 0.0,
            flags: BlockPhysicsFlags::default(),
            surface_response: response,
        },
        floor: true,
    }
}

#[test]
fn grounded_web_applies_one_slowdown_and_weaving_overrides_each_axis() {
    let mut world = surface(SurfaceResponse::None);
    world.facts.flags = BlockPhysicsFlags::COBWEB;
    world.facts.horizontal_speed_factor = 0.25;
    world.facts.vertical_speed_factor = 0.05;
    for (weaving, horizontal, vertical) in [(false, 0.25, 0.05), (true, 0.5, 0.25)] {
        let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
        state.on_ground = true;
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    forward: 1.0,
                    effects: MovementEffects {
                        weaving,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &world,
            )
            .unwrap();
        assert!((tick.movement.z - 0.098_000_004_887_580_87 * horizontal).abs() < 1.0e-8);
        state.on_ground = false;
        state.position.y = 3.0;
        state.velocity = Vec3::new(0.8, -0.8, 0.8);
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    effects: MovementEffects {
                        weaving,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &world,
            )
            .unwrap();
        assert!((tick.movement.y + 0.8 * vertical).abs() < 1.0e-8);
        assert_eq!(state.velocity.x, 0.0);
        assert_eq!(state.velocity.z, 0.0);
    }
}

#[test]
fn cobweb_scales_each_axis_and_stops_residual_motion_after_move() {
    let mut world = surface(SurfaceResponse::None);
    world.floor = false;
    world.facts.flags = BlockPhysicsFlags::COBWEB;
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.velocity = Vec3::new(0.8, -0.8, 0.8);
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(tick.environment.in_cobweb);
    assert!((tick.movement.x - 0.2).abs() <= f64::from(f32::EPSILON));
    assert!((tick.movement.y + 0.04).abs() <= f64::from(f32::EPSILON));
    assert!((tick.movement.z - 0.2).abs() <= f64::from(f32::EPSILON));
    assert_eq!(state.velocity.x, 0.0);
    assert!((state.velocity.y + 0.0784).abs() <= f64::from(f32::EPSILON));
    assert_eq!(state.velocity.z, 0.0);
}

#[test]
fn cobweb_zeroes_post_move_velocity_before_vertical_effect_precedence() {
    let mut world = surface(SurfaceResponse::None);
    world.floor = false;
    world.facts.flags = BlockPhysicsFlags::COBWEB;

    for (effects, expected_y) in [
        (MovementEffects::default(), -0.0784),
        (
            MovementEffects {
                slow_falling: true,
                ..MovementEffects::default()
            },
            -0.0784,
        ),
        (
            MovementEffects {
                levitation: Some(0),
                ..MovementEffects::default()
            },
            0.0098,
        ),
        (
            MovementEffects {
                levitation: Some(0),
                slow_falling: true,
                ..MovementEffects::default()
            },
            0.0098,
        ),
        (
            MovementEffects {
                levitation: Some(-2),
                slow_falling: true,
                ..MovementEffects::default()
            },
            -0.0098,
        ),
    ] {
        let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
        state.velocity = Vec3::new(0.8, -0.8, 0.8);
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    effects,
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();

        assert!(tick.environment.in_cobweb);
        assert_eq!(state.velocity.x, 0.0);
        assert!((state.velocity.y - expected_y).abs() <= f64::from(f32::EPSILON));
        assert_eq!(state.velocity.z, 0.0);
    }
}

#[test]
fn slime_and_bed_bounce_while_sneaking_suppresses_both() {
    for (response, expected) in [
        (SurfaceResponse::Slime, 0.6076),
        (SurfaceResponse::Bed, 0.4361),
    ] {
        let mut state = PlayerState::new(Vec3::new(0.0, 1.2, 0.0));
        state.velocity.y = -0.7;
        let tick = Simulator::default()
            .tick(&mut state, MovementInput::default(), &surface(response))
            .unwrap();
        assert!(tick.collisions.y);
        assert!((state.velocity.y - expected).abs() <= f64::from(f32::EPSILON));
    }

    let mut sneaking = PlayerState::new(Vec3::new(0.0, 1.2, 0.0));
    sneaking.velocity.y = -0.7;
    Simulator::default()
        .tick(
            &mut sneaking,
            MovementInput {
                sneaking: true,
                ..MovementInput::default()
            },
            &surface(SurfaceResponse::Slime),
        )
        .unwrap();
    assert!(sneaking.velocity.y <= 0.0);

    let mut grounded = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    grounded.on_ground = true;
    grounded.velocity.y = -0.2;
    Simulator::default()
        .tick(
            &mut grounded,
            MovementInput::default(),
            &surface(SurfaceResponse::Slime),
        )
        .unwrap();
    assert_eq!(
        grounded.velocity.y,
        f64::from((0.2_f32 - 0.08_f32) * 0.98_f32)
    );
}

/// Vanilla bed restitution is 0.75, without a one-block velocity cap.
#[test]
fn bed_restitution_is_uncapped() {
    let mut state = PlayerState::new(Vec3::new(0.0, 1.2, 0.0));
    state.velocity.y = -2.0;
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput::default(),
            &surface(SurfaceResponse::Bed),
        )
        .unwrap();
    assert!(tick.collisions.y);
    assert!((state.velocity.y - 1.3916).abs() <= f64::from(f32::EPSILON));
}

#[test]
fn soul_sand_slows_horizontal_motion() {
    let ordinary = surface(SurfaceResponse::None);
    let mut ordinary_state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    ordinary_state.on_ground = true;
    let normal = Simulator::default()
        .tick(
            &mut ordinary_state,
            MovementInput {
                forward: 1.0,
                ..MovementInput::default()
            },
            &ordinary,
        )
        .unwrap();

    // Soul sand slows through its acceleration friction, not its speed factor.
    let mut sand = surface(SurfaceResponse::SoulSand);
    sand.facts.horizontal_speed_factor = 0.543;
    let mut sand_state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    sand_state.on_ground = true;
    let slowed = Simulator::default()
        .tick(
            &mut sand_state,
            MovementInput {
                forward: 1.0,
                ..MovementInput::default()
            },
            &sand,
        )
        .unwrap();
    assert!(
        slowed.movement.horizontal_length_squared() < normal.movement.horizontal_length_squared()
    );
    assert_eq!(
        slowed.environment.surface_response,
        SurfaceResponse::SoulSand
    );
}

#[test]
fn powder_snow_uses_authoritative_passable_slowing_without_a_guessed_cube() {
    let mut world = surface(SurfaceResponse::None);
    world.floor = false;
    world.facts.flags = BlockPhysicsFlags::POWDER_SNOW;
    world.facts.horizontal_speed_factor = 0.25;
    world.facts.vertical_speed_factor = 0.5;
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.velocity = Vec3::new(0.4, -0.4, 0.4);
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(tick.environment.in_powder_snow);
    assert!(tick.movement.x.abs() < 0.4);
    assert!(tick.movement.y.abs() < 0.4);
    assert!(tick.movement.z.abs() < 0.4);
}

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

#[derive(Clone, Copy)]
struct SurfaceWorld {
    facts: BlockPhysicsFacts,
    floor: bool,
}

impl CollisionWorld for SurfaceWorld {
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
    assert!((tick.movement.x - 0.2).abs() <= 1.0e-12);
    assert!((tick.movement.y + 0.04).abs() <= 1.0e-12);
    assert!((tick.movement.z - 0.2).abs() <= 1.0e-12);
    assert_eq!(state.velocity, Vec3::ZERO);
}

#[test]
fn slime_and_bed_bounce_while_sneaking_suppresses_slime() {
    for (response, expected) in [
        (SurfaceResponse::Slime, 0.6076),
        (SurfaceResponse::Bed, 0.374_36),
    ] {
        let mut state = PlayerState::new(Vec3::new(0.0, 1.2, 0.0));
        state.velocity.y = -0.7;
        let tick = Simulator::default()
            .tick(&mut state, MovementInput::default(), &surface(response))
            .unwrap();
        assert!(tick.collisions.y);
        assert!((state.velocity.y - expected).abs() <= 1.0e-12);
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
    assert!(grounded.velocity.y <= 0.0);
}

#[test]
fn authoritative_soul_sand_and_honey_factors_slow_horizontal_motion() {
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

    for response in [SurfaceResponse::SoulSand, SurfaceResponse::Honey] {
        let mut slowed_world = surface(response);
        slowed_world.facts.horizontal_speed_factor = 0.4;
        let mut slowed_state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
        slowed_state.on_ground = true;
        let slowed = Simulator::default()
            .tick(
                &mut slowed_state,
                MovementInput {
                    forward: 1.0,
                    ..MovementInput::default()
                },
                &slowed_world,
            )
            .unwrap();
        assert!(
            slowed.movement.horizontal_length_squared()
                < normal.movement.horizontal_length_squared()
        );
        assert_eq!(slowed.environment.surface_response, response);
    }
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

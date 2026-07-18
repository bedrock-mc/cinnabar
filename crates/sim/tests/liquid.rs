use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

#[derive(Clone, Copy)]
struct FluidWorld {
    facts: BlockPhysicsFacts,
}

impl CollisionWorld for FluidWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([self.facts]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn fluid(flags: BlockPhysicsFlags, response: SurfaceResponse) -> FluidWorld {
    FluidWorld {
        facts: BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 1.0,
            flags,
            surface_response: response,
        },
    }
}

fn submerged() -> PlayerState {
    let mut state = PlayerState::new(Vec3::new(0.5, 0.1, 0.5));
    state.velocity = Vec3::new(0.4, -0.3, 0.2);
    state
}

#[test]
fn water_applies_flowless_buoyancy_and_drag_while_lava_is_slower() {
    let mut water_state = submerged();
    let water_tick = Simulator::default()
        .tick(
            &mut water_state,
            MovementInput {
                forward: 1.0,
                jumping: true,
                ..MovementInput::default()
            },
            &fluid(BlockPhysicsFlags::WATER, SurfaceResponse::None),
        )
        .unwrap();
    assert!(water_tick.environment.in_water);
    assert!(water_state.velocity.y > -0.3);
    assert!(water_state.velocity.x.abs() < 0.4);

    let mut lava_state = submerged();
    Simulator::default()
        .tick(
            &mut lava_state,
            MovementInput {
                forward: 1.0,
                jumping: true,
                ..MovementInput::default()
            },
            &fluid(BlockPhysicsFlags::LAVA, SurfaceResponse::None),
        )
        .unwrap();
    assert!(
        lava_state.velocity.horizontal_length_squared()
            < water_state.velocity.horizontal_length_squared()
    );
}

#[test]
fn entering_and_exiting_water_changes_only_authoritative_environment_motion() {
    let mut state = submerged();
    let entered = Simulator::default()
        .tick(
            &mut state,
            MovementInput::default(),
            &fluid(BlockPhysicsFlags::WATER, SurfaceResponse::None),
        )
        .unwrap();
    assert!(entered.environment.in_water);

    state.position.y = 2.0;
    let air = FluidWorld {
        facts: BlockPhysicsFacts {
            fluid_height_blocks: 0.0,
            flags: BlockPhysicsFlags::default(),
            ..fluid(BlockPhysicsFlags::WATER, SurfaceResponse::None).facts
        },
    };
    let exited = Simulator::default()
        .tick(&mut state, MovementInput::default(), &air)
        .unwrap();
    assert!(!exited.environment.in_water);
    assert!(!exited.environment.in_lava);
}

#[test]
fn bubble_columns_apply_bounded_directional_vertical_response() {
    for (response, direction) in [
        (SurfaceResponse::BubbleUp, 1.0),
        (SurfaceResponse::BubbleDown, -1.0),
    ] {
        let mut state = submerged();
        state.velocity = Vec3::ZERO;
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput::default(),
                &fluid(BlockPhysicsFlags::WATER, response),
            )
            .unwrap();
        assert_eq!(tick.environment.surface_response, response);
        assert!(state.velocity.y * direction > 0.0);
        assert!(state.velocity.y.abs() <= 0.4);
    }
}

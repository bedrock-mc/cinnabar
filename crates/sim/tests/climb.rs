use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

struct ClimbWorld {
    flags: BlockPhysicsFlags,
}

impl CollisionWorld for ClimbWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: self.flags,
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

#[test]
fn ladder_ascend_descend_and_sneak_hold_use_climb_velocity_clamps() {
    let world = ClimbWorld {
        flags: BlockPhysicsFlags::CLIMBABLE,
    };
    let mut ascending = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let up = Simulator::default()
        .tick(
            &mut ascending,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(up.environment.on_climbable);
    assert!(up.movement.y > 0.0);
    assert!(up.movement.y <= 0.2);

    let mut descending = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    descending.velocity.y = -1.0;
    let down = Simulator::default()
        .tick(&mut descending, MovementInput::default(), &world)
        .unwrap();
    assert!((down.movement.y + 0.2).abs() <= 1.0e-12);

    let mut holding = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    holding.velocity.y = -1.0;
    let held = Simulator::default()
        .tick(
            &mut holding,
            MovementInput {
                sneaking: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert_eq!(held.movement.y, 0.0);
}

#[test]
fn scaffolding_uses_the_same_bounded_vertical_controls() {
    let world = ClimbWorld {
        flags: BlockPhysicsFlags::SCAFFOLDING,
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(tick.environment.in_scaffolding);
    assert!((tick.movement.y - 0.2).abs() <= 1.0e-12);
}

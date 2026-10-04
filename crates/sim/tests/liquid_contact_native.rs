//! LiquidBlocksFetch senses the preceding pose before SwimTrigger changes it.

use std::cell::RefCell;

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, MovementMode, PlayerState, PredictionHistory, SimulationError, Simulator,
    SurfaceResponse, Vec3, WorldQueryError,
};

struct ContactWorld {
    liquid: BlockPhysicsFlags,
    liquid_y: i32,
    fail_upper: bool,
    queries: RefCell<Vec<Aabb>>,
}

impl ContactWorld {
    fn new(liquid_y: i32, liquid: BlockPhysicsFlags) -> Self {
        Self {
            liquid,
            liquid_y,
            fail_upper: false,
            queries: RefCell::new(Vec::new()),
        }
    }
}

impl CollisionWorld for ContactWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.queries.borrow_mut().push(query);
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        if self.fail_upper && block[1] == 1 {
            return Err(WorldQueryError::QueryExtentExceeded);
        }
        let liquid = block[1] == self.liquid_y;
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if liquid { 1.0 } else { 0.0 },
                flags: if liquid {
                    self.liquid
                } else {
                    BlockPhysicsFlags::PASSABLE
                },
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

#[test]
fn stopping_swim_keeps_old_low_box_liquid_contact_for_first_standing_tick() {
    // With feet .65, the low water probe clamps to its .95 center. The
    // standing water probe begins above 1 after its .401 inset, so only
    // native sensing of the preceding box remains in cell-zero water.
    for liquid in [BlockPhysicsFlags::WATER, BlockPhysicsFlags::LAVA] {
        let world = ContactWorld::new(0, liquid);
        let mut state = PlayerState::new(Vec3::new(0.5, 0.65, 0.5));
        state.swim_amount = 1.0;
        state.swim_pose_active = true;
        let output = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    jumping: true,
                    liquid_contact_height: Some(MovementMode::Swimming.hitbox_height(false)),
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        assert_eq!(
            output.environment.in_water,
            liquid == BlockPhysicsFlags::WATER
        );
        assert_eq!(
            output.environment.in_lava,
            liquid == BlockPhysicsFlags::LAVA
        );
        assert_eq!(output.movement.y, f64::from(f32::from_bits(0x3d23_d70a)));
        assert!(world.queries.borrow()[0].max.y > 2.4);

        let mut without_old_pose = PlayerState::new(Vec3::new(0.5, 0.65, 0.5));
        let legacy = Simulator::default()
            .tick(&mut without_old_pose, MovementInput::default(), &world)
            .unwrap();
        assert!(!legacy.environment.in_water);
        assert!(!legacy.environment.in_lava);
    }
}

#[test]
fn entering_swim_senses_old_standing_box_and_moves_with_the_new_low_box() {
    let world = ContactWorld::new(1, BlockPhysicsFlags::WATER);
    let mut state = PlayerState::new(Vec3::new(0.5, 0.0, 0.5));
    let output = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Swimming,
                jumping: true,
                liquid_attach_height: Some(1.5),
                liquid_contact_height: Some(MovementMode::Walking.hitbox_height(false)),
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(output.environment.in_water);
    assert_eq!(output.movement.y, f64::from(f32::from_bits(0x3d23_d70a)));
    // The retained sensing box must not enlarge this tick's collision box.
    assert!(world.queries.borrow()[0].max.y < 0.7);
    assert_eq!(state.swim_amount, 0.0);
    assert!(state.swim_pose_active);
}

#[test]
fn old_box_only_queries_remain_bounded_and_transactional() {
    let mut world = ContactWorld::new(1, BlockPhysicsFlags::WATER);
    world.fail_upper = true;
    let mut state = PlayerState::new(Vec3::new(0.5, 0.0, 0.5));
    let before = state.clone();
    assert!(matches!(
        Simulator::default().tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Swimming,
                liquid_contact_height: Some(MovementMode::Walking.hitbox_height(false)),
                ..MovementInput::default()
            },
            &world,
        ),
        Err(SimulationError::World(WorldQueryError::QueryExtentExceeded))
    ));
    assert_eq!(state, before);
    // An unspecified legacy low pose does not query the upper cell.
    Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Swimming,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
}

#[test]
fn captured_contact_height_survives_prediction_replay_and_rejects_invalid_values() {
    let world = ContactWorld::new(0, BlockPhysicsFlags::WATER);
    let simulator = Simulator::default();
    let mut state = PlayerState::new(Vec3::new(0.5, 0.65, 0.5));
    let mut history = PredictionHistory::new(8).unwrap();
    history
        .predict(&mut state, MovementInput::default(), &simulator, &world)
        .unwrap();
    let corrected = history.state_at(1).unwrap().clone();
    let live = history
        .predict(
            &mut state,
            MovementInput {
                jumping: true,
                liquid_contact_height: Some(MovementMode::Swimming.hitbox_height(false)),
                ..MovementInput::default()
            },
            &simulator,
            &world,
        )
        .unwrap();
    let (_, replayed) = history
        .rewind_and_replay_with_controls(&mut state, corrected, &simulator, &world, &[])
        .unwrap();
    assert_eq!(replayed[0].tick_result, live);
    assert!(live.environment.in_water);

    for height in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        let before = state.clone();
        assert!(matches!(
            simulator.tick(
                &mut state,
                MovementInput {
                    liquid_contact_height: Some(height),
                    ..MovementInput::default()
                },
                &world,
            ),
            Err(SimulationError::InvalidLiquidContactHeight)
        ));
        assert_eq!(state, before);
    }
}

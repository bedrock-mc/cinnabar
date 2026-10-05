use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, MovementMode, PlayerState, ProvenancedCollider, Simulator, SurfaceResponse,
    Vec3, WorldQueryError, pose_fits,
};

/// Fixed solids with optional per-world block flags on every sampled cell.
struct Solids {
    boxes: Vec<(Aabb, [i32; 3])>,
    flags: BlockPhysicsFlags,
}

impl CollisionWorld for Solids {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.boxes
                .iter()
                .map(|(shape, _)| *shape)
                .filter(|shape| shape.intersects(query.grown(0.01)))
                .collect(),
        ))
    }

    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.boxes
                .iter()
                .filter(|(shape, _)| shape.intersects(query.grown(0.01)))
                .map(|(shape, block)| ProvenancedCollider {
                    aabb: *shape,
                    block: Some(*block),
                    runtime_id: Some(1),
                })
                .collect(),
        ))
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

fn empty(flags: BlockPhysicsFlags) -> Solids {
    Solids {
        boxes: Vec::new(),
        flags,
    }
}

fn unit_block() -> Aabb {
    Aabb::new(Vec3::new(-8.0, 0.0, -8.0), Vec3::new(8.0, 1.0, 8.0))
}

fn tick(state: &mut PlayerState, input: MovementInput, world: &Solids) -> sim::TickResult {
    Simulator::default().tick(state, input, world).unwrap()
}

#[test]
fn flying_hovers_without_gravity_and_jump_sneak_steer_vertically() {
    let world = empty(BlockPhysicsFlags::default());
    let mut state = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
    let hover = tick(
        &mut state,
        MovementInput {
            mode: MovementMode::Flying,
            ..MovementInput::default()
        },
        &world,
    );
    assert_eq!(hover.movement.y, 0.0);

    let up = tick(
        &mut state,
        MovementInput {
            mode: MovementMode::Flying,
            jumping: true,
            ..MovementInput::default()
        },
        &world,
    );
    assert!(up.movement.y > 0.0);

    let mut state = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
    let down = tick(
        &mut state,
        MovementInput {
            mode: MovementMode::Flying,
            sneaking: true,
            ..MovementInput::default()
        },
        &world,
    );
    assert!(down.movement.y < 0.0);
}

/// A server-granted FlySpeed scales steady flight linearly, with no clamp to the default.
#[test]
fn ability_fly_speed_scales_steady_flight_velocity() {
    let world = empty(BlockPhysicsFlags::default());
    let steady = |fly_speed| {
        let mut state = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        let input = MovementInput {
            mode: MovementMode::Flying,
            forward: 1.0,
            fly_speed,
            ..MovementInput::default()
        };
        (0..100)
            .map(|_| tick(&mut state, input, &world).movement.z.abs())
            .last()
            .unwrap()
    };
    let ratio = steady(Some(0.5)) / steady(None);
    assert!((ratio - 10.0).abs() < 0.05, "10x FlySpeed flew {ratio}x");
}

#[test]
fn gliding_descends_faster_while_diving() {
    let world = empty(BlockPhysicsFlags::default());
    let mut state = PlayerState::new(Vec3::new(0.5, 50.0, 0.5));
    state.velocity = Vec3::new(0.0, 0.0, 0.5);
    let mut previous_y = 0.0;
    for _ in 0..5 {
        let result = tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Gliding,
                pitch_degrees: 50.0,
                ..MovementInput::default()
            },
            &world,
        );
        assert!(result.movement.y < previous_y);
        previous_y = result.movement.y;
    }
}

#[test]
fn low_poses_fit_a_one_block_gap_that_standing_does_not() {
    let world = Solids {
        boxes: vec![
            (
                Aabb::new(Vec3::new(-8.0, -1.0, -8.0), Vec3::new(8.0, 0.0, 8.0)),
                [0, -1, 0],
            ),
            (
                Aabb::new(Vec3::new(-8.0, 1.0, -8.0), Vec3::new(8.0, 2.0, 8.0)),
                [0, 1, 0],
            ),
        ],
        flags: BlockPhysicsFlags::default(),
    };
    let feet = Vec3::new(0.5, 0.0, 0.5);
    assert!(!pose_fits(&world, feet, MovementMode::Walking, false).unwrap());
    assert!(!pose_fits(&world, feet, MovementMode::Walking, true).unwrap());
    assert!(pose_fits(&world, feet, MovementMode::Crawling, false).unwrap());
    assert!(pose_fits(&world, feet, MovementMode::Swimming, false).unwrap());
}

#[test]
fn scaffolding_supports_from_above_and_is_passable_when_sneaking_or_beside() {
    let world = Solids {
        boxes: vec![(unit_block(), [0, 0, 0])],
        flags: BlockPhysicsFlags::SCAFFOLDING,
    };
    let mut standing = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    standing.velocity.y = -0.3;
    let landed = tick(&mut standing, MovementInput::default(), &world);
    assert!(landed.movement.y.abs() < 1.0e-9);

    let mut sneaking = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let sunk = tick(
        &mut sneaking,
        MovementInput {
            sneaking: true,
            ..MovementInput::default()
        },
        &world,
    );
    assert!(sunk.movement.y < 0.0);

    let mut inside = PlayerState::new(Vec3::new(0.5, 0.4, 0.5));
    let walked = tick(
        &mut inside,
        MovementInput {
            forward: 1.0,
            yaw_degrees: 0.0,
            ..MovementInput::default()
        },
        &world,
    );
    assert!(!walked.collisions.z);
}

#[test]
fn creative_hover_damps_harder_and_vertical_speed_scales_ascent() {
    let world = empty(BlockPhysicsFlags::default());
    let second_tick_y = |creative_flight| {
        let mut state = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        state.velocity.y = 1.0;
        let input = MovementInput {
            mode: MovementMode::Flying,
            creative_flight,
            ..MovementInput::default()
        };
        tick(&mut state, input, &world);
        tick(&mut state, input, &world).movement.y
    };
    assert!(second_tick_y(true) < second_tick_y(false));

    let ascend = |vertical_fly_speed| {
        let mut state = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Flying,
                jumping: true,
                vertical_fly_speed,
                ..MovementInput::default()
            },
            &world,
        )
        .movement
        .y
    };
    assert!(ascend(Some(2.0)) > ascend(None));
}

#[test]
fn riding_freezes_player_motion_but_still_reports_a_tick() {
    let world = empty(BlockPhysicsFlags::default());
    let mut state = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
    state.velocity = Vec3::new(0.3, -0.5, 0.3);
    let result = tick(
        &mut state,
        MovementInput {
            mode: MovementMode::Riding,
            forward: 1.0,
            ..MovementInput::default()
        },
        &world,
    );
    assert_eq!(state.position, Vec3::new(0.5, 10.0, 0.5));
    assert_eq!(result.movement, Vec3::ZERO);
    assert_eq!(result.velocity, Vec3::ZERO);
    assert_eq!(state.tick, 1);
}

/// Water only above a low pose's box: fluid contact follows the pose box, not a standing one.
struct WaterAboveFloor;

impl CollisionWorld for WaterAboveFloor {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let water = block[1] == 11;
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if water { 1.0 } else { 0.0 },
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

#[test]
fn fluid_contact_samples_the_current_pose_box() {
    let in_water = |mode| {
        let mut state = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        let input = MovementInput {
            mode,
            ..MovementInput::default()
        };
        Simulator::default()
            .tick_with_controls(&mut state, input, &WaterAboveFloor)
            .unwrap()
            .tick_result
            .environment
            .in_water
    };
    assert!(
        in_water(MovementMode::Walking),
        "a standing body reaches the water"
    );
    assert!(
        !in_water(MovementMode::Crawling),
        "a 0.6-high crawler stays below it"
    );
}

/// The native scaffold support tolerance does not catch feet already below its top.
#[test]
fn scaffold_support_does_not_extend_a_millimetre_below_the_top() {
    let world = Solids {
        boxes: vec![(unit_block(), [0, 0, 0])],
        flags: BlockPhysicsFlags::SCAFFOLDING,
    };
    let mut state = PlayerState::new(Vec3::new(0.5, f64::from(1.0_f32 - 0.000_01), 0.5));
    state.velocity.y = -0.03;
    let result = tick(&mut state, MovementInput::default(), &world);
    assert!(!result.collisions.y);
    assert_eq!(result.movement.y, f64::from(-0.03_f32));
}

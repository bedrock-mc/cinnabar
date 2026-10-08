//! Vanilla swim-amount and head-in-water guards on jumping.

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, MovementMode, PlayerState, PredictionHistory, SimulationError, Simulator,
    SurfaceResponse, Vec3, WorldQueryError, sample_liquid_submersion, sample_water_head,
};

struct JumpWorld {
    flags: BlockPhysicsFlags,
    water_top: i32,
    fluid_height: f64,
    secondary: bool,
    ground: bool,
}

impl JumpWorld {
    fn submerged() -> Self {
        Self {
            flags: BlockPhysicsFlags::WATER,
            water_top: 128,
            fluid_height: 1.0,
            secondary: false,
            ground: false,
        }
    }
}

impl CollisionWorld for JumpWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-8.0, -1.0, -8.0), Vec3::new(8.0, 0.0, 8.0));
        Ok(CollisionQuery::synthetic(
            (self.ground && floor.intersects(query))
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let flags = if block[1] < self.water_top {
            self.flags
        } else {
            BlockPhysicsFlags::PASSABLE
        };
        let facts = BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: self.fluid_height,
            flags,
            surface_response: SurfaceResponse::None,
        };
        Ok(BlockPhysicsSample {
            layers: if self.secondary {
                Box::new([
                    BlockPhysicsFacts {
                        flags: BlockPhysicsFlags::PASSABLE,
                        fluid_height_blocks: 0.0,
                        ..facts
                    },
                    facts,
                ])
            } else {
                Box::new([facts])
            },
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn jump(mode: MovementMode) -> MovementInput {
    MovementInput {
        mode,
        jumping: true,
        jump_pressed: true,
        liquid_attach_height: Some(0.5),
        ..MovementInput::default()
    }
}

#[test]
fn native_swim_blend_advances_before_jump_and_zeroes_partial_wet_ascent() {
    // The swim-amount blend advances before the jump step and the swim
    // trigger. The first entry tick retains
    // zero; subsequent additions follow the prior swimming/crawling flag.
    let blend_bits = [
        0,
        0x3dcc_cccd,
        0x3e4c_cccd,
        0x3e99_999a,
        0x3ecc_cccd,
        0x3f00_0000,
        0x3f19_999a,
        0x3f33_3334,
        0x3f4c_ccce,
        0x3f66_6668,
        0x3f80_0000,
    ];
    for mode in [MovementMode::Swimming, MovementMode::Crawling] {
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        state.velocity.y = 0.3;
        for (index, expected) in blend_bits.into_iter().enumerate() {
            let result = Simulator::default()
                .tick(&mut state, jump(mode), &JumpWorld::submerged())
                .unwrap();
            assert_eq!(
                state.swim_amount.to_bits(),
                expected,
                "{mode:?}, tick {index}"
            );
            assert_eq!(
                result.movement.y,
                if index == 0 {
                    f64::from(f32::from_bits(0x3eae_147b))
                } else if index == 10 {
                    f64::from(f32::from_bits(if mode == MovementMode::Swimming {
                        0x3d23_d70a
                    } else {
                        0x3d0f_5c29
                    }))
                } else {
                    0.0
                },
                "{mode:?}, tick {index}"
            );
        }
    }
}

#[test]
fn exiting_swim_blend_suppresses_ground_jump_until_native_zero() {
    let dry_ground = JumpWorld {
        flags: BlockPhysicsFlags::PASSABLE,
        ground: true,
        ..JumpWorld::submerged()
    };
    let expected_blend = [
        0x3f66_6666,
        0x3f4c_cccc,
        0x3f33_3332,
        0x3f19_9998,
        0x3eff_fffd,
        0x3ecc_ccca,
        0x3e99_9997,
        0x3e4c_ccc8,
        0x3dcc_ccc3,
        0,
    ];
    let mut state = PlayerState::new(Vec3::new(0.5, 0.0, 0.5));
    state.on_ground = true;
    state.swim_amount = 1.0;
    state.swim_pose_active = true;
    // StopSwimming follows the writer: this first dry tick still has amount1.
    Simulator::default()
        .tick(&mut state, MovementInput::default(), &dry_ground)
        .unwrap();
    assert_eq!(state.swim_amount, 1.0);
    assert!(!state.swim_pose_active);
    for (index, expected) in expected_blend.into_iter().enumerate() {
        let result = Simulator::default()
            .tick_with_controls(&mut state, jump(MovementMode::Walking), &dry_ground)
            .unwrap();
        assert_eq!(state.swim_amount.to_bits(), expected);
        assert_eq!(result.jump_initiated, index == 9);
        if index < 9 {
            assert_eq!(state.position.y, 0.0);
            assert_eq!(state.jump_delay, 0);
        } else {
            assert_eq!(result.tick_result.movement.y, f64::from(0.42_f32));
        }
    }
}

#[test]
fn partial_dry_blend_keeps_velocity_while_wet_blend_clears_it() {
    for wet in [false, true] {
        let world = JumpWorld {
            flags: if wet {
                BlockPhysicsFlags::WATER
            } else {
                BlockPhysicsFlags::PASSABLE
            },
            ..JumpWorld::submerged()
        };
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        state.velocity.y = 0.2;
        state.swim_amount = 0.5;
        let result = Simulator::default()
            .tick(&mut state, jump(MovementMode::Walking), &world)
            .unwrap();
        assert_eq!(
            result.movement.y,
            if wet { 0.0 } else { f64::from(0.2_f32) }
        );
    }
}

#[test]
fn fully_blended_swim_jump_still_requires_primary_head_water() {
    for (height, secondary, permitted) in
        [(0.5, false, true), (0.7, false, false), (0.5, true, false)]
    {
        let world = JumpWorld {
            water_top: 6,
            fluid_height: 4.0 / 9.0,
            secondary,
            ..JumpWorld::submerged()
        };
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        state.swim_amount = 1.0;
        state.swim_pose_active = true;
        state.velocity.y = 0.2;
        let result = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    liquid_attach_height: Some(height),
                    ..jump(MovementMode::Swimming)
                },
                &world,
            )
            .unwrap();
        // The permitted path retains incoming velocity and adds ordinary .04.
        assert_eq!(
            result.movement.y,
            if permitted {
                f64::from(f32::from_bits(0x3e75_c290))
            } else {
                0.0
            }
        );
    }
}

#[test]
fn head_water_uses_source_level_surface_and_rejects_lava() {
    let source = JumpWorld {
        water_top: 6,
        fluid_height: 8.0 / 9.0,
        ..JumpWorld::submerged()
    };
    let feet = Vec3::new(0.5, 5.0, 0.5);
    assert!(sample_water_head(&source, feet, 0.95).unwrap().value);
    assert!(!sample_water_head(&source, feet, 1.0).unwrap().value);
    let lava = JumpWorld {
        flags: BlockPhysicsFlags::LAVA,
        ..source
    };
    assert!(!sample_water_head(&lava, feet, 0.5).unwrap().value);
}

/// Breathing-point submersion accepts either liquid against the same surface.
#[test]
fn breathing_submersion_admits_water_and_lava_below_the_surface() {
    let source = JumpWorld {
        water_top: 6,
        fluid_height: 8.0 / 9.0,
        ..JumpWorld::submerged()
    };
    let lava = JumpWorld {
        flags: BlockPhysicsFlags::LAVA,
        ..source
    };
    for world in [&source, &lava] {
        assert!(
            sample_liquid_submersion(world, Vec3::new(0.5, 5.95, 0.5))
                .unwrap()
                .value
        );
        assert!(
            !sample_liquid_submersion(world, Vec3::new(0.5, 6.0, 0.5))
                .unwrap()
                .value
        );
    }
}

#[test]
fn invalid_blend_rejects_transactionally_and_legacy_state_defaults_to_zero() {
    for amount in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        state.swim_amount = amount;
        let before_tick = state.tick;
        assert!(matches!(
            Simulator::default().tick(
                &mut state,
                jump(MovementMode::Swimming),
                &JumpWorld::submerged()
            ),
            Err(SimulationError::InvalidSwimAmount)
        ));
        assert_eq!(state.tick, before_tick);
        assert_eq!(state.swim_amount.to_bits(), amount.to_bits());
    }
    let mut value = serde_json::to_value(PlayerState::new(Vec3::ZERO)).unwrap();
    value.as_object_mut().unwrap().remove("swim_amount");
    let legacy: PlayerState = serde_json::from_value(value).unwrap();
    assert_eq!(legacy.swim_amount, 0.0);
    assert!(!legacy.swim_pose_active);
}

#[test]
fn first_stop_tick_uses_retained_pose_for_blend_before_current_ground_jump() {
    let dry_ground = JumpWorld {
        flags: BlockPhysicsFlags::PASSABLE,
        ground: true,
        ..JumpWorld::submerged()
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 0.0, 0.5));
    state.on_ground = true;
    state.swim_amount = 1.0;
    state.swim_pose_active = true;
    let output = Simulator::default()
        .tick_with_controls(&mut state, jump(MovementMode::Walking), &dry_ground)
        .unwrap();
    assert_eq!(state.swim_amount, 1.0);
    assert!(!state.swim_pose_active);
    assert!(output.jump_initiated);
}

#[test]
fn correction_replay_retains_prior_pose_and_rebuilds_the_blend_jump_gate() {
    let world = JumpWorld {
        flags: BlockPhysicsFlags::PASSABLE,
        ground: true,
        ..JumpWorld::submerged()
    };
    let simulator = Simulator::default();
    let mut state = PlayerState::new(Vec3::new(0.5, 0.0, 0.5));
    state.on_ground = true;
    state.swim_amount = 1.0;
    state.swim_pose_active = true;
    let mut history = PredictionHistory::new(16).unwrap();
    history
        .predict(&mut state, MovementInput::default(), &simulator, &world)
        .unwrap();
    let mut live = Vec::new();
    for _ in 0..10 {
        live.push(
            history
                .predict_with_controls(&mut state, jump(MovementMode::Walking), &simulator, &world)
                .unwrap(),
        );
    }
    let corrected = history.state_at(1).unwrap().clone();
    let (_, replayed) = history
        .rewind_and_replay_with_controls(&mut state, corrected, &simulator, &world, &[])
        .unwrap();
    assert_eq!(replayed, live);
    assert_eq!(state.swim_amount, 0.0);
    assert!(replayed.last().unwrap().jump_initiated);
}

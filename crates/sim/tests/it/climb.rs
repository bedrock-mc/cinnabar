use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, PlayerState, ProvenancedCollider, Simulator, SurfaceResponse, Vec3,
    WorldQueryError,
};

struct ClimbWorld {
    flags: BlockPhysicsFlags,
}

/// A held climb jump updates motion without moving during the skipped travel tick.
#[test]
fn skipped_travel_preserves_climb_jump_motion() {
    let world = ClimbWorld {
        flags: BlockPhysicsFlags::CLIMBABLE,
    };
    let mut player = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let target = player.position;
    let output = Simulator::default()
        .tick(
            &mut player,
            MovementInput {
                skip_travel: true,
                jumping: true,
                ..Default::default()
            },
            &world,
        )
        .unwrap();
    assert_eq!(output.position, target);
    assert_eq!(output.movement, Vec3::ZERO);
    assert_eq!(output.velocity.y, f64::from(0.2_f32));
    assert_eq!(player.jump_delay, 0);
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
    assert!(up.movement.y <= f64::from(0.2_f32));

    let mut descending = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    descending.velocity.y = -1.0;
    let down = Simulator::default()
        .tick(&mut descending, MovementInput::default(), &world)
        .unwrap();
    assert!((down.movement.y + 0.2).abs() <= f64::from(f32::EPSILON));

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

/// A held jump inside supported scaffolding ascends at the scaffold speed, not the ladder one.
#[test]
fn scaffolding_ascends_at_its_own_speed() {
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
    assert_eq!(tick.movement.y as f32, 0.15_f32);
}

/// Scaffold cells with unit collision, solid cells, and air everywhere else.
struct ScaffoldWorld {
    scaffolds: Vec<[i32; 3]>,
    solids: Vec<[i32; 3]>,
}

impl ScaffoldWorld {
    fn cells(&self) -> impl Iterator<Item = [i32; 3]> + '_ {
        self.scaffolds.iter().chain(&self.solids).copied()
    }
}

fn unit(block: [i32; 3]) -> Aabb {
    let min = Vec3::new(
        f64::from(block[0]),
        f64::from(block[1]),
        f64::from(block[2]),
    );
    Aabb::new(min, Vec3::new(min.x + 1.0, min.y + 1.0, min.z + 1.0))
}

impl CollisionWorld for ScaffoldWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.cells()
                .map(unit)
                .filter(|shape| shape.intersects(query))
                .collect(),
        ))
    }

    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.cells()
                .filter(|block| unit(*block).intersects(query))
                .map(|block| ProvenancedCollider {
                    aabb: unit(block),
                    block: Some(block),
                    runtime_id: Some(1),
                    cell_has_scaffolding: None,
                })
                .collect(),
        ))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let flags = if self.scaffolds.contains(&block) {
            BlockPhysicsFlags::SCAFFOLDING
        } else if self.solids.contains(&block) {
            BlockPhysicsFlags::default()
        } else {
            BlockPhysicsFlags::PASSABLE
        };
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags,
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn sneaking() -> MovementInput {
    MovementInput {
        sneaking: true,
        ..MovementInput::default()
    }
}

/// Sneaking over supported scaffolding descends at a fixed speed with drag but no gravity.
#[test]
fn sneaking_descends_a_supported_scaffold_column_at_a_fixed_speed() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 2.0, 0.5));
    state.on_ground = true;
    for _ in 0..2 {
        let tick = Simulator::default()
            .tick(&mut state, sneaking(), &world)
            .unwrap();
        assert_eq!(tick.movement.y as f32, -0.15_f32);
        assert_eq!(tick.velocity.y as f32, -0.15_f32 * 0.98_f32);
    }
}

/// A tick with suppressed travel still starts the descent, without moving.
#[test]
fn skipped_travel_still_starts_the_scaffold_descent() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 2.0, 0.5));
    state.on_ground = true;
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                skip_travel: true,
                ..sneaking()
            },
            &world,
        )
        .unwrap();
    assert!(tick.environment.descending_through_block);
    assert_eq!(tick.position.y, 2.0);
    assert_eq!(tick.velocity.y as f32, -0.15_f32);
}

/// A scaffold bridge over air is not a descending block, so sneaking keeps its support.
#[test]
fn sneaking_on_a_scaffold_bridge_over_air_stays_on_top() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0]],
        solids: Vec::new(),
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.on_ground = true;
    let tick = Simulator::default()
        .tick(&mut state, sneaking(), &world)
        .unwrap();
    assert_eq!(tick.position.y, 1.0);
    assert!(tick.on_ground);
}

/// Jumping inside scaffolding climbs instead of jumping and starts the jump delay.
#[test]
fn jumping_inside_scaffolding_climbs_without_a_ground_jump() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.on_ground = true;
    let output = Simulator::default()
        .tick_with_controls(
            &mut state,
            MovementInput {
                jumping: true,
                jump_pressed: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(!output.jump_initiated);
    assert_eq!(output.tick_result.movement.y as f32, 0.15_f32);
    assert_eq!(state.jump_delay, sim::JUMP_DELAY_TICKS - 1);
}

/// The environment query reports the same feet-layer scaffolding contact as the tick.
#[test]
fn the_environment_query_reports_scaffolding_contact() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 1, 0]],
        solids: vec![[0, 0, 0]],
    };
    let environment = Simulator::default()
        .movement_environment(
            Vec3::new(0.5, 1.0, 0.5),
            sim::MovementMode::Walking,
            false,
            &world,
        )
        .unwrap();
    assert!(environment.value.in_scaffolding);
}

/// Scaffolding over air is no ascending block, yet a held jump inside it still climbs.
#[test]
fn climbing_inside_a_scaffold_bridge_over_air_still_ascends() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 1, 0]],
        solids: Vec::new(),
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.75, 0.5));
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
    assert!(!tick.environment.in_ascending_block);
    assert_eq!(tick.movement.y as f32, 0.15_f32);
}

/// The block climber scans the layer below the feet every tick, not only while sneaking.
#[test]
fn standing_on_supported_scaffolding_reports_the_layer_below_without_sneaking() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.on_ground = true;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    let environment = tick.environment;
    assert!(environment.over_scaffolding);
    assert!(environment.over_descending_block);
    assert!(!environment.in_scaffolding);
    assert!(!environment.in_ascending_block);
    assert!(!environment.descending_through_block);
    assert_eq!(tick.position.y, 1.0);
}

/// Sneaking over supported scaffolding reports the descent it performs.
#[test]
fn sneaking_over_supported_scaffolding_reports_the_descent() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let tick = Simulator::default()
        .tick(&mut state, sneaking(), &world)
        .unwrap();
    let environment = tick.environment;
    assert!(environment.in_scaffolding && environment.in_ascending_block);
    assert!(environment.over_scaffolding && environment.over_descending_block);
    assert!(environment.descending_through_block);
}

fn powder_snow() -> ClimbWorld {
    ClimbWorld {
        flags: BlockPhysicsFlags::from_bits(
            BlockPhysicsFlags::POWDER_SNOW.bits() | BlockPhysicsFlags::PASSABLE.bits(),
        )
        .unwrap(),
    }
}

/// A snow-standing wearer climbs out of powder snow at the climbing-block speed.
#[test]
fn a_held_jump_climbs_powder_snow_only_for_a_snow_standing_wearer() {
    for can_stand_on_snow in [false, true] {
        let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    jumping: true,
                    can_stand_on_snow,
                    ..MovementInput::default()
                },
                &powder_snow(),
            )
            .unwrap();
        let environment = tick.environment;
        assert_eq!(environment.in_ascending_block, can_stand_on_snow);
        assert_eq!(environment.over_descending_block, can_stand_on_snow);
        assert_eq!(environment.on_climbable, can_stand_on_snow);
        assert!(!environment.in_scaffolding && !environment.over_scaffolding);
        assert_eq!(tick.movement.y > 0.0, can_stand_on_snow);
    }
}

/// Powder snow is climbable for a snow-standing wearer: descent is clamped as on a ladder.
#[test]
fn powder_snow_clamps_a_snow_standing_fall_like_a_ladder() {
    for can_stand_on_snow in [false, true] {
        let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
        state.velocity.y = -1.0;
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    can_stand_on_snow,
                    ..MovementInput::default()
                },
                &powder_snow(),
            )
            .unwrap();
        // The fixture's unit speed factors leave the clamped request unscaled.
        let expected = if can_stand_on_snow { -0.2_f32 } else { -1.0 };
        assert!(
            (tick.movement.y as f32 - expected).abs() <= 1.0e-6,
            "{can_stand_on_snow}: {}",
            tick.movement.y
        );
    }
}

/// Sneaking over powder snow descends through it for a snow-standing wearer.
#[test]
fn sneaking_over_powder_snow_descends_for_a_snow_standing_wearer() {
    struct SnowBelow;
    impl CollisionWorld for SnowBelow {
        fn collision_boxes(
            &self,
            _query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Ok(CollisionQuery::synthetic(Vec::new()))
        }

        fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
            let flags = if block[1] < 2 {
                BlockPhysicsFlags::POWDER_SNOW.bits() | BlockPhysicsFlags::PASSABLE.bits()
            } else {
                BlockPhysicsFlags::PASSABLE.bits()
            };
            ClimbWorld {
                flags: BlockPhysicsFlags::from_bits(flags).unwrap(),
            }
            .block_physics(block)
        }
    }
    let mut state = PlayerState::new(Vec3::new(0.5, 2.0, 0.5));
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                sneaking: true,
                can_stand_on_snow: true,
                ..MovementInput::default()
            },
            &SnowBelow,
        )
        .unwrap();
    assert!(tick.environment.over_descending_block);
    assert!(tick.environment.descending_through_block);
    assert!(!tick.environment.in_ascending_block);
    assert_eq!(tick.movement.y as f32, -0.15_f32);
}

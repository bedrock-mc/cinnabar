use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, PlayerState, ProvenancedCollider, Simulator, SurfaceResponse, Vec3,
    WorldQueryError,
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

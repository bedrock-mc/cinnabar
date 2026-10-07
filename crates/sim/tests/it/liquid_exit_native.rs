//! Vanilla liquid exit regressions. The exit probe uses the actual resolved pose
//! box in f32, for every liquid travel mode.

use std::cell::{Cell, RefCell};

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, MovementMode, PlayerState, SimulationError, Simulator, SurfaceResponse, Vec3,
    WorldQueryError,
};

struct Shore {
    liquid: BlockPhysicsFlags,
    roof: bool,
    raised_liquid: bool,
    secondary_raised_liquid: bool,
    fail_probe: bool,
    conflict_probe: bool,
    collision_started: Cell<bool>,
    queries: RefCell<Vec<Aabb>>,
}

impl Shore {
    fn water() -> Self {
        Self {
            liquid: BlockPhysicsFlags::WATER,
            roof: false,
            raised_liquid: false,
            secondary_raised_liquid: false,
            fail_probe: false,
            conflict_probe: false,
            collision_started: Cell::new(false),
            queries: RefCell::new(Vec::new()),
        }
    }
}

impl CollisionWorld for Shore {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.collision_started.set(true);
        self.queries.borrow_mut().push(query);
        let ledge = Aabb::new(Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0));
        let roof = Aabb::new(Vec3::new(0.0, 2.0, 0.0), Vec3::new(2.0, 2.2, 1.0));
        let boxes = [Some(ledge), self.roof.then_some(roof)]
            .into_iter()
            .flatten()
            .filter(|shape| shape.intersects(query))
            .collect();
        Ok(CollisionQuery::synthetic(boxes))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let probing = self.collision_started.get();
        if probing && self.fail_probe {
            return Err(WorldQueryError::QueryExtentExceeded);
        }
        let liquid = block[1] == 0 || (block[1] >= 1 && self.raised_liquid);
        let primary = BlockPhysicsFacts {
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
        };
        let mut sample = BlockPhysicsSample {
            layers: if block[1] >= 1 && self.secondary_raised_liquid {
                Box::new([
                    primary,
                    BlockPhysicsFacts {
                        flags: self.liquid,
                        fluid_height_blocks: 1.0,
                        ..primary
                    },
                ])
            } else {
                Box::new([primary])
            },
            identity: CollisionQuery::synthetic(()).identity,
        };
        if probing && self.conflict_probe {
            sample.identity.registry.preg_sha256[0] = 1;
        }
        Ok(sample)
    }
}

fn swim_into_shore(mode: MovementMode, world: &Shore) -> sim::TickResult {
    let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
    state.swim_amount = 1.0;
    state.swim_pose_active = true;
    state.velocity.x = 0.5;
    Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                mode,
                jumping: true,
                ..MovementInput::default()
            },
            world,
        )
        .unwrap()
}

#[test]
fn liquid_ledge_exit_applies_to_swim_crawl_and_lava_travel() {
    for mode in [
        MovementMode::Walking,
        MovementMode::Swimming,
        MovementMode::Crawling,
    ] {
        for liquid in [BlockPhysicsFlags::WATER, BlockPhysicsFlags::LAVA] {
            let world = Shore {
                liquid,
                ..Shore::water()
            };
            let result = swim_into_shore(mode, &world);
            assert!(result.collisions.x, "{mode:?}, {liquid:?}");
            assert_eq!(result.movement.x, 0.0);
            assert_eq!(result.velocity.y, f64::from(f32::from_bits(0x3e99_999a)));
        }
    }
}

#[test]
fn raised_exit_probe_uses_the_low_pose_below_a_roof() {
    for mode in [MovementMode::Swimming, MovementMode::Crawling] {
        let world = Shore {
            roof: true,
            ..Shore::water()
        };
        let result = swim_into_shore(mode, &world);
        assert_eq!(result.velocity.y, f64::from(f32::from_bits(0x3e99_999a)));
        let queries = world.queries.borrow();
        let probe = queries.last().unwrap();
        assert!(probe.max.y < 2.0, "exit probe must retain the low pose");
        assert!(probe.min.y > 1.0);
        assert!(probe.max.y - probe.min.y < 0.7);
    }
}

#[test]
fn raised_primary_liquid_denies_exit_but_secondary_liquid_does_not() {
    for secondary in [false, true] {
        let world = Shore {
            raised_liquid: !secondary,
            secondary_raised_liquid: secondary,
            ..Shore::water()
        };
        let result = swim_into_shore(MovementMode::Swimming, &world);
        if secondary {
            assert_eq!(result.velocity.y, f64::from(f32::from_bits(0x3e99_999a)));
        } else {
            assert_eq!(result.velocity.y, f64::from(f32::from_bits(0x3d03_126f)));
        }
    }
}

#[test]
fn liquid_exit_probe_failure_rolls_back_swimming_prediction() {
    for conflict in [false, true] {
        let world = Shore {
            fail_probe: !conflict,
            conflict_probe: conflict,
            ..Shore::water()
        };
        let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
        state.velocity.x = 0.5;
        let before = state.clone();
        let failure = Simulator::default().tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Swimming,
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        );
        assert_eq!(state, before);
        assert!(matches!(
            failure,
            Err(SimulationError::World(WorldQueryError::QueryExtentExceeded))
                | Err(SimulationError::World(
                    WorldQueryError::RegistryIdentityMismatch
                ))
        ));
    }
}

#[test]
fn retained_dry_swimming_pose_keeps_ordinary_travel_acceleration() {
    let world = Shore {
        liquid: BlockPhysicsFlags::PASSABLE,
        ..Shore::water()
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 3.0, 0.5));
    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Swimming,
                forward: 1.0,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(!result.environment.in_water);
    assert!(!result.environment.in_lava);
    assert_eq!(result.movement.z, f64::from(f32::from_bits(0x3ca0_902e)));
    assert!(result.velocity.y < 0.0);
}

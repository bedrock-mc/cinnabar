use std::cell::Cell;

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementEffects, MovementInput, PlayerState, Simulator, SurfaceResponse, Vec3,
    WorldCollisionIdentity, WorldQueryError,
};

#[derive(Clone, Copy)]
struct FluidWorld {
    facts: BlockPhysicsFacts,
}

struct MixedLiquidWorld;

struct DryBubbleBoundary;

struct SharedBudgetWorld {
    calls: Cell<usize>,
}

struct IdentityProbeWorld {
    initial_identity: WorldCollisionIdentity,
    collision_identity: WorldCollisionIdentity,
    liquid_probe_identity: WorldCollisionIdentity,
    physics_calls: Cell<usize>,
}

struct TranslatedBoundaryProbeWorld;

impl CollisionWorld for DryBubbleBoundary {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let facts = if block[1] == 0 {
            BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 1.0,
                flags: BlockPhysicsFlags::WATER,
                surface_response: SurfaceResponse::BubbleUp,
            }
        } else {
            BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: BlockPhysicsFlags::default(),
                surface_response: SurfaceResponse::None,
            }
        };
        Ok(BlockPhysicsSample {
            layers: Box::new([facts]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
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

impl CollisionWorld for MixedLiquidWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let water = BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 1.0,
            flags: BlockPhysicsFlags::WATER,
            surface_response: SurfaceResponse::None,
        };
        let lava = BlockPhysicsFacts {
            flags: BlockPhysicsFlags::LAVA,
            ..water
        };
        Ok(BlockPhysicsSample {
            layers: Box::new([water, lava]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

impl CollisionWorld for SharedBudgetWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let ledge = Aabb::new(Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0));
        Ok(CollisionQuery::synthetic(
            ledge
                .intersects(query)
                .then_some(ledge)
                .into_iter()
                .collect(),
        ))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.calls.set(self.calls.get() + 1);
        Ok(BlockPhysicsSample {
            layers: Box::new([water_facts()]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

impl CollisionWorld for IdentityProbeWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let ledge = Aabb::new(Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0));
        Ok(CollisionQuery {
            value: ledge
                .intersects(query)
                .then_some(ledge)
                .into_iter()
                .collect(),
            identity: self.collision_identity.clone(),
        })
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let calls = self.physics_calls.get();
        self.physics_calls.set(calls + 1);
        Ok(BlockPhysicsSample {
            layers: Box::new([water_facts()]),
            identity: if calls < 6 {
                self.initial_identity.clone()
            } else {
                self.liquid_probe_identity.clone()
            },
        })
    }
}

impl CollisionWorld for TranslatedBoundaryProbeWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let ledge = Aabb::new(Vec3::new(101.0, 0.0, 0.0), Vec3::new(102.0, 1.0, 1.0));
        Ok(CollisionQuery::synthetic(
            ledge
                .intersects(query)
                .then_some(ledge)
                .into_iter()
                .collect(),
        ))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let water = (block == [100, 0, 0]) || (block == [101, 1, 0]);
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

/// Returns full-height water facts for worlds that exercise liquid query behavior.
fn water_facts() -> BlockPhysicsFacts {
    BlockPhysicsFacts {
        friction: 0.6,
        horizontal_speed_factor: 1.0,
        vertical_speed_factor: 1.0,
        fluid_height_blocks: 1.0,
        flags: BlockPhysicsFlags::WATER,
        surface_response: SurfaceResponse::None,
    }
}

/// Builds a public wire identity with one distinct compatible test chunk.
fn probe_identity(chunk_x: i32, revision: u64, preg_byte: u8) -> WorldCollisionIdentity {
    serde_json::from_value(serde_json::json!({
        "protocol": 1001,
        "id_space": "sequential",
        "preg_sha256": vec![preg_byte; 32],
        "chunks": [{
            "dimension": 0,
            "x": chunk_x,
            "z": 0,
            "revision": revision,
        }],
    }))
    .expect("the test identity uses the public collision wire format")
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
fn liquid_vertical_order_is_drag_then_gravity_or_levitation() {
    for (flags, drag, gravity_per_tick) in [
        (BlockPhysicsFlags::WATER, 0.8, 0.005),
        (BlockPhysicsFlags::LAVA, 0.5, 0.02),
    ] {
        let world = fluid(flags, SurfaceResponse::None);

        let mut gravity = submerged();
        Simulator::default()
            .tick(&mut gravity, MovementInput::default(), &world)
            .unwrap();
        let expected_gravity = -0.3 * drag - gravity_per_tick;
        assert!((gravity.velocity.y - expected_gravity).abs() <= 1.0e-12);

        let mut levitation = submerged();
        Simulator::default()
            .tick(
                &mut levitation,
                MovementInput {
                    effects: MovementEffects {
                        levitation: Some(0),
                        ..MovementEffects::default()
                    },
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        let after_drag = -0.3 * drag;
        let expected_levitation = after_drag + (0.05 - after_drag) * 0.2;
        assert!((levitation.velocity.y - expected_levitation).abs() <= 1.0e-12);
    }
}

#[test]
fn mixed_water_and_lava_uses_water_drag_and_gravity_precedence() {
    let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
    state.velocity.y = -0.3;
    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &MixedLiquidWorld)
        .unwrap();

    assert!(result.environment.in_water && result.environment.in_lava);
    assert!(
        (result.velocity.y - (-0.3 * 0.8 - 0.005)).abs() <= 1.0e-12,
        "mixed-liquid vertical velocity = {}",
        result.velocity.y
    );
}

#[test]
fn liquid_exit_probe_shares_the_tick_block_sample_budget() {
    let within_cap = SharedBudgetWorld {
        calls: Cell::new(0),
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 0.1, 0.5));
    state.velocity.x = 29.0;
    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &within_cap,
        )
        .unwrap();
    assert!(result.collisions.x);
    assert_eq!(within_cap.calls.get(), 64);

    let beyond_cap = SharedBudgetWorld {
        calls: Cell::new(0),
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
    state.velocity.x = 20.0;
    let before = state.clone();
    assert!(matches!(
        Simulator::default().tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &beyond_cap,
        ),
        Err(sim::SimulationError::World(
            WorldQueryError::QueryExtentExceeded
        ))
    ));
    assert_eq!(beyond_cap.calls.get(), 64);
    assert_eq!(state, before);
}

#[test]
fn liquid_exit_probe_merges_distinct_collision_and_liquid_identities() {
    let initial_identity = probe_identity(2, 10, 7);
    let collision_identity = probe_identity(3, 11, 7);
    let liquid_probe_identity = probe_identity(4, 12, 7);
    let world = IdentityProbeWorld {
        initial_identity: initial_identity.clone(),
        collision_identity: collision_identity.clone(),
        liquid_probe_identity: liquid_probe_identity.clone(),
        physics_calls: Cell::new(0),
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
    state.velocity.x = 0.5;

    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert_eq!(
        result.world_identity,
        initial_identity
            .merge(&collision_identity)
            .unwrap()
            .merge(&liquid_probe_identity)
            .unwrap()
    );
    assert_eq!(world.physics_calls.get(), 8);
}

#[test]
fn incompatible_liquid_exit_probe_identity_fails_transactionally() {
    let world = IdentityProbeWorld {
        initial_identity: probe_identity(2, 10, 7),
        collision_identity: probe_identity(3, 11, 7),
        liquid_probe_identity: probe_identity(4, 12, 8),
        physics_calls: Cell::new(0),
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
    state.velocity.x = 0.5;
    let before = state.clone();

    assert!(matches!(
        Simulator::default().tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        ),
        Err(sim::SimulationError::World(
            WorldQueryError::RegistryIdentityMismatch
        ))
    ));
    assert_eq!(world.physics_calls.get(), 8);
    assert_eq!(state, before);
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

#[test]
fn dry_bubble_support_boundary_does_not_apply_a_column_response() {
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &DryBubbleBoundary)
        .unwrap();
    assert!(!tick.environment.in_water);
    assert_eq!(tick.environment.surface_response, SurfaceResponse::None);
    assert!(tick.velocity.y < 0.0);
}

struct LiquidExitWorld {
    boxes: Box<[Aabb]>,
    water_top: i32,
    fail_liquid_probe: bool,
    partial_probe_liquid: bool,
}

impl CollisionWorld for LiquidExitWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.boxes
                .iter()
                .copied()
                .filter(|shape| shape.intersects(query))
                .collect(),
        ))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        if self.fail_liquid_probe && block[1] >= 2 {
            return Err(WorldQueryError::QueryExtentExceeded);
        }
        let water = (-1..1).contains(&block[0])
            && (0..self.water_top).contains(&block[1])
            && (-1..2).contains(&block[2]);
        let partial_probe_liquid = self.partial_probe_liquid
            && (-1..1).contains(&block[0])
            && block[1] == 1
            && (-1..2).contains(&block[2]);
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if water {
                    1.0
                } else if partial_probe_liquid {
                    0.1
                } else {
                    0.0
                },
                flags: if water || partial_probe_liquid {
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

/// Returns the observed vertical velocity for one named v0.1.4 liquid case.
fn v014_expected_vertical_velocity(scenario: &str) -> f64 {
    let trace = include_str!("../fixtures/bedsim-v0.1.4-liquid.jsonl")
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .find_map(|record| {
            let record = record.expect("the liquid fixture is canonical JSONL");
            (record["scenario"] == scenario).then_some(record)
        })
        .expect("the named liquid scenario is present");
    assert_eq!(trace["scenario"], scenario);
    trace["steps"][0]["expected"]["velocity"]["y"]
        .as_f64()
        .expect("the liquid fixture records a finite vertical velocity")
}

#[test]
fn bedsim_v0_1_4_clear_water_ledge_exit_replay_receives_observed_boost() {
    let world = LiquidExitWorld {
        boxes: Box::new([Aabb::new(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 1.0),
        )]),
        water_top: 1,
        fail_liquid_probe: false,
        partial_probe_liquid: false,
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
    state.velocity = Vec3::new(0.5, 0.0, 0.0);

    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();

    assert!(result.collisions.x);
    assert!(
        (result.velocity.y - v014_expected_vertical_velocity("water_ledge_exit_boost")).abs()
            <= 1.0e-6,
        "clear liquid ledge exit velocity = {}, expected v0.1.4 evidence",
        result.velocity.y
    );
}

#[test]
fn liquid_exit_probe_rejects_a_partial_liquid_cell_below_its_surface() {
    let world = LiquidExitWorld {
        boxes: Box::new([Aabb::new(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 1.0),
        )]),
        water_top: 1,
        fail_liquid_probe: false,
        partial_probe_liquid: true,
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
    state.velocity.x = 0.5;

    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();

    assert!(result.collisions.x);
    assert!(
        (result.velocity.y - v014_expected_vertical_velocity("water_ledge_exit_blocked_above"))
            .abs()
            <= 1.0e-6,
        "partial liquid cell must deny the ledge boost, got {}",
        result.velocity.y
    );
}

#[test]
fn liquid_exit_probe_excludes_a_translated_touching_boundary_cell() {
    let mut state = PlayerState::new(Vec3::new(100.5, 0.5, 0.5));
    state.velocity.x = 0.5;
    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &TranslatedBoundaryProbeWorld,
        )
        .unwrap();

    assert!(result.collisions.x);
    assert!(
        (result.velocity.y - v014_expected_vertical_velocity("water_ledge_exit_boost")).abs()
            <= 1.0e-6,
        "touching boundary liquid must not deny the clear boost, got {}",
        result.velocity.y
    );
}

#[test]
fn out_of_range_exclusive_query_maximum_fails_transactionally() {
    let mut state = PlayerState::new(Vec3::new(f64::from(i32::MAX) - 2.0, 0.5, 0.5));
    state.velocity.x = 4.0;
    let before = state.clone();

    assert!(matches!(
        Simulator::default().tick(&mut state, MovementInput::default(), &MixedLiquidWorld),
        Err(sim::SimulationError::World(
            WorldQueryError::CoordinateOutOfRange
        ))
    ));
    assert_eq!(state, before);
}

#[test]
fn bedsim_v0_1_4_open_water_held_ascent_uses_observed_water_gravity() {
    let mut state = PlayerState::new(Vec3::new(0.5, 0.5, 0.5));
    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &fluid(BlockPhysicsFlags::WATER, SurfaceResponse::None),
        )
        .unwrap();

    assert!(!result.collisions.x && !result.collisions.z);
    assert!(
        (result.velocity.y - v014_expected_vertical_velocity("open_water_held_ascent")).abs()
            <= 1.0e-6,
        "open-water held-ascent velocity = {}, expected v0.1.4 evidence",
        result.velocity.y
    );
}

#[test]
fn water_ledge_exit_does_not_treat_an_unavailable_liquid_probe_as_clear() {
    let world = LiquidExitWorld {
        boxes: Box::new([Aabb::new(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 1.0),
        )]),
        water_top: 1,
        fail_liquid_probe: true,
        partial_probe_liquid: false,
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 0.1, 0.5));
    state.velocity = Vec3::new(0.5, 0.0, 0.0);
    let before = state.clone();

    assert!(matches!(
        Simulator::default().tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        ),
        Err(sim::SimulationError::World(
            WorldQueryError::QueryExtentExceeded
        ))
    ));
    assert_eq!(state, before);
}

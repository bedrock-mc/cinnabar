//! Spatial sensing regressions corroborated by BedSim commit
//! 9baeb2a99a57f882cab001d07e9d0b664699b6e4. These independently authored
//! cases do not close version-matched retail movement acceptance.

use std::cell::RefCell;

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MAX_BLOCK_SAMPLES_PER_TICK, MovementInput, PlayerState, SimulationError, Simulator,
    SurfaceResponse, Vec3, WorldCollisionIdentity, WorldQueryError,
};
use world::{ChunkCollisionRevision, ChunkKey};

struct CobwebWorld {
    cell: [i32; 3],
    secondary_layer: bool,
    fault: Option<WorldQueryError>,
    conflicting_identity: bool,
    track_chunks: bool,
    queried: RefCell<Vec<[i32; 3]>>,
}

impl CobwebWorld {
    fn at(cell: [i32; 3]) -> Self {
        Self {
            cell,
            secondary_layer: false,
            fault: None,
            conflicting_identity: false,
            track_chunks: false,
            queried: RefCell::default(),
        }
    }
}

impl CollisionWorld for CobwebWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        // Cobweb occupancy must not depend on solid collision geometry.
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.queried.borrow_mut().push(block);
        let ordinary = BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 0.0,
            flags: BlockPhysicsFlags::default(),
            surface_response: SurfaceResponse::None,
        };
        let mut layers = vec![ordinary];
        let mut identity = CollisionQuery::synthetic(()).identity;
        if self.track_chunks {
            identity = WorldCollisionIdentity::new(
                identity.registry,
                [ChunkCollisionRevision {
                    chunk: ChunkKey::new(0, block[0].div_euclid(16), block[2].div_euclid(16)),
                    revision: 1,
                }],
            )?;
        }
        if block == self.cell {
            if let Some(error) = &self.fault {
                return Err(error.clone());
            }
            if self.secondary_layer {
                layers.push(ordinary);
            }
            layers.last_mut().unwrap().flags = BlockPhysicsFlags::COBWEB;
            if self.conflicting_identity {
                identity.registry.preg_sha256[0] = 1;
            }
        }
        Ok(BlockPhysicsSample {
            layers: layers.into_boxed_slice(),
            identity,
        })
    }
}

fn moving_state(position: Vec3, velocity: Vec3) -> PlayerState {
    let mut state = PlayerState::new(position);
    state.velocity = velocity;
    state
}

fn assert_close(actual: Vec3, expected: Vec3) {
    for axis in 0..3 {
        assert!((actual[axis] - expected[axis]).abs() <= f64::from(f32::EPSILON));
    }
}

#[test]
fn support_only_cobweb_does_not_slow_motion() {
    let world = CobwebWorld::at([0, 0, 0]);
    let mut state = moving_state(Vec3::new(0.5, 1.0, 0.5), Vec3::new(0.4, 0.0, 0.0));
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(!tick.environment.in_cobweb);
    assert_close(tick.movement, Vec3::new(0.4, 0.0, 0.0));
    assert!(world.queried.borrow().contains(&world.cell));
}

#[test]
fn swept_only_cobweb_does_not_slow_motion_until_next_tick() {
    let world = CobwebWorld::at([1, 1, 0]);
    let mut state = moving_state(Vec3::new(0.5, 1.0, 0.5), Vec3::new(0.4, 0.0, 0.0));
    let first = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(!first.environment.in_cobweb);
    assert_close(first.movement, Vec3::new(0.4, 0.0, 0.0));
    assert!(world.queried.borrow().contains(&world.cell));
    let second = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(second.environment.in_cobweb);
    assert!((second.movement.x - first.velocity.x * 0.25).abs() <= f64::from(f32::EPSILON));
}

#[test]
fn tangent_support_boundary_is_not_cobweb_occupancy() {
    let world = CobwebWorld::at([-3, -4, 7]);
    let mut state = moving_state(Vec3::new(-2.5, -3.0, 7.5), Vec3::new(0.0, -0.2, 0.0));
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(!tick.environment.in_cobweb);
    assert_close(tick.movement, Vec3::new(0.0, -0.2, 0.0));
    assert!(world.queried.borrow().contains(&world.cell));
}

#[test]
fn translated_noncollidable_cobweb_senses_both_primary_and_secondary_layers() {
    for secondary_layer in [false, true] {
        let mut world = CobwebWorld::at([-32, 64, -24]);
        world.secondary_layer = secondary_layer;
        let mut state = moving_state(Vec3::new(-31.5, 64.0, -23.5), Vec3::new(0.4, -0.4, 0.4));
        let tick = Simulator::default()
            .tick(&mut state, MovementInput::default(), &world)
            .unwrap();
        assert!(tick.environment.in_cobweb);
        assert_close(tick.movement, Vec3::new(0.1, -0.02, 0.1));
        assert_eq!(state.velocity.x, 0.0);
        assert_eq!(state.velocity.z, 0.0);
        assert!(world.queried.borrow().len() <= MAX_BLOCK_SAMPLES_PER_TICK);
    }
}

#[test]
fn nonoverlapping_cobweb_samples_still_fail_transactionally_on_bad_authority() {
    for conflicting_identity in [false, true] {
        let mut world = CobwebWorld::at([1, 1, 0]);
        world.conflicting_identity = conflicting_identity;
        if !conflicting_identity {
            world.fault = Some(WorldQueryError::UnknownRuntimeId {
                runtime_id: 1,
                block: world.cell,
            });
        }
        let mut state = moving_state(Vec3::new(0.5, 1.0, 0.5), Vec3::new(0.4, 0.0, 0.0));
        let before = state.clone();
        let expected = if conflicting_identity {
            WorldQueryError::RegistryIdentityMismatch
        } else {
            world.fault.clone().unwrap()
        };
        assert_eq!(
            Simulator::default().tick(&mut state, MovementInput::default(), &world),
            Err(SimulationError::World(expected))
        );
        assert_eq!(state, before);
        assert!(world.queried.borrow().contains(&world.cell));
    }
}

#[test]
fn swept_only_cobweb_chunk_still_contributes_to_tick_identity() {
    let mut world = CobwebWorld::at([16, 1, 0]);
    world.track_chunks = true;
    let mut state = moving_state(Vec3::new(15.5, 1.0, 0.5), Vec3::new(0.4, 0.0, 0.0));
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(!tick.environment.in_cobweb);
    assert!(world.queried.borrow().contains(&world.cell));
    assert_eq!(tick.world_identity.chunks.len(), 2);
    assert!(
        tick.world_identity
            .chunks
            .iter()
            .any(|revision| { revision.chunk == ChunkKey::new(0, 1, 0) && revision.revision == 1 })
    );
}

#[test]
fn wide_cobweb_query_retains_the_existing_sample_budget() {
    let world = CobwebWorld::at([0, 1, 0]);
    let mut state = moving_state(Vec3::new(0.5, 1.0, 0.5), Vec3::new(40.0, 0.0, 0.0));
    let before = state.clone();
    assert_eq!(
        Simulator::default().tick(&mut state, MovementInput::default(), &world),
        Err(SimulationError::World(WorldQueryError::QueryExtentExceeded))
    );
    assert_eq!(state, before);
    assert!(world.queried.borrow().is_empty());
}

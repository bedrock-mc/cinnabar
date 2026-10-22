//! The anchor probe reads the mover's contextual collision.
//!
//! Teleports and corrections can place the player inside scaffolding or
//! powder snow. The player's own move passes through both there, and vanilla's
//! closest-space push reads the same contextual shapes, so the probe must
//! leave such an anchor where the server put it. Ordinary solids still count.

use std::time::Duration;

use protocol::PLAYER_NETWORK_OFFSET;
use sim::{
    Aabb, BlockPhysicsFlags, CollisionRegistry, MovementInput, PaletteWorld, SurfaceResponse, Vec3,
};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

use super::anchor_probe::{AnchorMover, AnchorProbeState, BeforeTick};
use super::{
    LocalPhysicsController, MovementTicker, PhysicsCorrectionMode,
    reconcile_candidate_physics_correction,
};

const AIR: u32 = 0;
const STONE: u32 = 1;
const SCAFFOLDING: u32 = 2;
const POWDER_SNOW: u32 = 3;

/// Scaffolding column at `x = 6, z = 6` from `y = 1` through `y = 9`.
const COLUMN: [u8; 2] = [6, 6];
const COLUMN_TOP: u8 = 9;
/// Feet just above the floor, as a teleport onto it can round: the horizontal
/// escape is then the shortest way out of an ordinary unit-cube column.
const BOTTOM_FEET: f64 = 1.001;
/// Powder snow cell, resting on the stone floor.
const SNOW: [u8; 3] = [10, 1, 10];

/// Air, a unit stone cube, scaffolding whose ordinary shape is the unit cube,
/// and shapeless powder snow, as the live registry describes them.
fn registry() -> CollisionRegistry {
    let unit = Aabb::new(Vec3::ZERO, Vec3::ONE);
    let mut registry = CollisionRegistry::new();
    registry.register(AIR, []).unwrap();
    registry.register(STONE, [unit]).unwrap();
    registry
        .register_physics(
            SCAFFOLDING,
            [unit],
            sim::DEFAULT_SURFACE_FRICTION,
            1.0,
            1.0,
            0.0,
            BlockPhysicsFlags::SCAFFOLDING,
            SurfaceResponse::None,
        )
        .unwrap();
    let snow = BlockPhysicsFlags::from_bits(
        BlockPhysicsFlags::POWDER_SNOW.bits() | BlockPhysicsFlags::PASSABLE.bits(),
    )
    .unwrap();
    registry
        .register_physics(
            POWDER_SNOW,
            [],
            sim::DEFAULT_SURFACE_FRICTION,
            0.9,
            1.5,
            0.0,
            snow,
            SurfaceResponse::None,
        )
        .unwrap();
    registry
}

/// One loaded chunk: a stone floor at `y = 0`, the scaffolding column (or a
/// stone column in its place) and one powder snow cell.
fn store(column: u32) -> ChunkStore {
    let mut store = ChunkStore::new();
    store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    let mut set = |[x, y, z]: [u8; 3], runtime_id: u32| {
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(x, y, z, 0, runtime_id),
                0,
            )
            .unwrap();
    };
    for x in 0..16 {
        for z in 0..16 {
            set([x, 0, z], STONE);
        }
    }
    for y in 1..=COLUMN_TOP {
        set([COLUMN[0], y, COLUMN[1]], column);
    }
    set(SNOW, POWDER_SNOW);
    store
}

/// Network position of feet at the centre of `[x, z]` and height `y`.
fn network([x, y, z]: [f64; 3]) -> [f32; 3] {
    [x as f32, y as f32 + PLAYER_NETWORK_OFFSET, z as f32]
}

fn column_feet(y: f64) -> [f64; 3] {
    [f64::from(COLUMN[0]) + 0.5, y, f64::from(COLUMN[1]) + 0.5]
}

fn sneaking() -> MovementInput {
    MovementInput {
        sneaking: true,
        ..MovementInput::default()
    }
}

/// Hard-anchors a fresh controller at `feet` and runs its first tick.
fn first_tick_after_teleport(
    world: &PaletteWorld<'_>,
    feet: [f64; 3],
    on_ground: bool,
    input: MovementInput,
) -> super::PhysicsMovementSample {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(network(feet), 0, on_ground);
    let frame = physics.advance(Duration::from_millis(50), input, world);
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    frame.samples.into_iter().next().expect("one sample")
}

/// Asserts the sample stayed on the anchored column without horizontal motion.
fn assert_not_pushed(sample: &super::PhysicsMovementSample, feet: [f64; 3]) {
    let anchored = network(feet);
    assert_eq!(
        [sample.position[0], sample.position[2]],
        [anchored[0], anchored[2]],
        "the anchor must stay where the server put it",
    );
    assert_eq!([sample.movement[0], sample.movement[2]], [0.0, 0.0]);
}

/// A teleport to the bottom of a scaffolding column keeps its position; the
/// scaffolding's ordinary unit shape used to push it 0.8 blocks to an edge.
#[test]
fn a_teleport_inside_scaffolding_is_not_pushed_out() {
    let (store, registry) = (store(SCAFFOLDING), registry());
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let feet = column_feet(BOTTOM_FEET);
    let sample = first_tick_after_teleport(&world, feet, false, MovementInput::default());
    assert_not_pushed(&sample, feet);
}

/// A teleport into the middle of the column, idle or sneaking down through it,
/// keeps its horizontal position.
#[test]
fn a_teleport_mid_column_is_not_pushed_out_while_idle_or_descending() {
    let (store, registry) = (store(SCAFFOLDING), registry());
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let feet = column_feet(4.1);
    for input in [MovementInput::default(), sneaking()] {
        let sample = first_tick_after_teleport(&world, feet, false, input);
        assert_not_pushed(&sample, feet);
    }
}

/// The same teleport into a stone column is still pushed clear.
#[test]
fn a_teleport_inside_a_solid_column_is_still_pushed_clear() {
    let (store, registry) = (store(STONE), registry());
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let feet = column_feet(BOTTOM_FEET);
    let sample = first_tick_after_teleport(&world, feet, false, MovementInput::default());
    let column = Aabb::new(
        Vec3::new(f64::from(COLUMN[0]), 1.0, f64::from(COLUMN[1])),
        Vec3::new(
            f64::from(COLUMN[0] + 1),
            f64::from(COLUMN_TOP + 1),
            f64::from(COLUMN[1] + 1),
        ),
    );
    let pushed = Vec3::new(
        f64::from(sample.position[0]),
        f64::from(sample.position[1] - PLAYER_NETWORK_OFFSET),
        f64::from(sample.position[2]),
    );
    assert!(!Aabb::player_at(pushed).intersects(column), "{pushed:?}");
}

/// A replayed correction that lands inside the column re-arms the probe, which
/// must still leave the player in the scaffolding.
#[test]
fn a_correction_inside_scaffolding_is_not_pushed_out() {
    let (store, registry) = (store(SCAFFOLDING), registry());
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let floor = column_feet(1.0);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(network(floor), 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, network(floor));
    ticker.set_source(super::MovementSource::Physics);
    let established = physics
        .advance(Duration::from_millis(50), MovementInput::default(), &world)
        .samples
        .pop()
        .expect("one established sample");
    let established_tick = established.tick;
    ticker.enqueue_completed_physics(established).unwrap();

    let corrected = column_feet(3.1);
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        network(corrected),
        established_tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &world,
    )
    .expect("the replay applies");
    let resumed = physics
        .advance(Duration::from_millis(50), MovementInput::default(), &world)
        .samples
        .pop()
        .expect("one resumed sample");
    assert_not_pushed(&resumed, corrected);
}

/// Powder snow the player sinks into is passable to the probe, with or without
/// the snow-standing capability.
#[test]
fn a_teleport_into_powder_snow_is_not_pushed_out() {
    let (store, registry) = (store(SCAFFOLDING), registry());
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let feet = [
        f64::from(SNOW[0]) + 0.5,
        f64::from(SNOW[1]) + 0.3,
        f64::from(SNOW[2]) + 0.5,
    ];
    for can_stand_on_snow in [false, true] {
        let input = MovementInput {
            can_stand_on_snow,
            ..MovementInput::default()
        };
        let sample = first_tick_after_teleport(&world, feet, false, input);
        assert_not_pushed(&sample, feet);
    }
}

/// The probe itself proceeds inside scaffolding and adjusts inside stone.
#[test]
fn the_probe_reads_the_movers_contextual_collision() {
    let registry = registry();
    let feet = column_feet(BOTTOM_FEET);
    let feet = Vec3::new(feet[0], feet[1], feet[2]);
    let state = sim::PlayerState::new(feet);
    let input = MovementInput::default();
    let mover = AnchorMover {
        state: &state,
        input: &input,
    };
    for (column, pushed) in [(SCAFFOLDING, false), (STONE, true)] {
        let store = store(column);
        let world = PaletteWorld::for_movement(&store, &registry, 0);
        let mut probe = AnchorProbeState::new();
        probe.note_hard_anchor();
        let decision = probe.before_tick(&world, mover, feet, sim::PLAYER_HEIGHT);
        let moved = matches!(decision, BeforeTick::Adjust(cleared) if cleared != feet);
        assert_eq!(moved, pushed, "{column}: {decision:?}");
    }
}

use sim::{
    Aabb, BlockPhysicsFlags, CollisionRegistry, CollisionWorld, MovementInput, PaletteWorld,
    PlayerState, Simulator, SurfaceResponse, TickResult, Vec3,
};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

const AIR: u32 = 0;
const SNOW: u32 = 1;
const STONE: u32 = 2;

/// Air, powder snow with its vanilla speed factors, and a full stone cube.
fn registry() -> CollisionRegistry {
    let mut registry = CollisionRegistry::new();
    registry.register(AIR, []).unwrap();
    let flags = BlockPhysicsFlags::from_bits(
        BlockPhysicsFlags::POWDER_SNOW.bits() | BlockPhysicsFlags::PASSABLE.bits(),
    )
    .unwrap();
    registry
        .register_physics(
            SNOW,
            [],
            sim::DEFAULT_SURFACE_FRICTION,
            0.9,
            1.5,
            0.0,
            flags,
            SurfaceResponse::None,
        )
        .unwrap();
    registry
        .register(STONE, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    registry
}

/// One loaded chunk with powder snow at (4, 4, 4) over stone at (4, 3, 4),
/// and bare stone at (8, 3, 8).
fn store() -> ChunkStore {
    let mut store = ChunkStore::new();
    store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    for ([x, y, z], runtime_id) in [([4, 4, 4], SNOW), ([4, 3, 4], STONE), ([8, 3, 8], STONE)] {
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(x, y, z, 0, runtime_id),
                0,
            )
            .unwrap();
    }
    store
}

/// Runs `ticks` ticks of `input` from `state`, returning every result.
fn run(state: &mut PlayerState, input: MovementInput, ticks: usize) -> Vec<TickResult> {
    let (store, registry) = (store(), registry());
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    (0..ticks)
        .map(|_| Simulator::default().tick(state, input, &world).unwrap())
        .collect()
}

/// A wearer who can stand on snow stands on its top; anyone else sinks in.
#[test]
fn only_a_snow_standing_wearer_stands_on_powder_snow() {
    for can_stand_on_snow in [false, true] {
        let mut state = PlayerState::new(Vec3::new(4.5, 5.0, 4.5));
        state.on_ground = true;
        let ticks = run(
            &mut state,
            MovementInput {
                can_stand_on_snow,
                ..MovementInput::default()
            },
            3,
        );
        let last = ticks.last().unwrap();
        assert_eq!(last.position.y == 5.0, can_stand_on_snow, "{last:?}");
        assert_eq!(last.on_ground, can_stand_on_snow);
    }
}

/// Sneaking on powder snow descends through it even for a snow-standing wearer.
#[test]
fn sneaking_descends_through_standable_powder_snow() {
    let mut state = PlayerState::new(Vec3::new(4.5, 5.0, 4.5));
    state.on_ground = true;
    let ticks = run(
        &mut state,
        MovementInput {
            sneaking: true,
            can_stand_on_snow: true,
            ..MovementInput::default()
        },
        1,
    );
    assert!(ticks[0].environment.descending_through_block);
    assert_eq!(ticks[0].movement.y as f32, -0.15_f32);
}

/// A fall longer than two and a half blocks lands on the lowered fall-into
/// shape when a tick starts at the snow's top, then sinks once landing clears
/// the fall distance; a short fall never meets that shape.
#[test]
fn a_long_fall_lands_on_the_lowered_powder_snow_shape() {
    // From 9.1 a tick starts at 5.0367, above the snow's top, after a 4-block fall.
    for (start, lands_on_shape) in [(9.1, true), (6.5, false)] {
        let mut state = PlayerState::new(Vec3::new(4.5, start, 4.5));
        let ticks = run(&mut state, MovementInput::default(), 30);
        let landing = ticks
            .iter()
            .position(|tick| tick.position.y as f32 == 4.9_f32 && tick.on_ground);
        assert_eq!(landing.is_some(), lands_on_shape, "from {start}");
        if let Some(landing) = landing {
            assert!(
                ticks[landing + 2].position.y < 4.9,
                "{:?}",
                ticks[landing + 2]
            );
        }
    }
}

/// Falling through the air accumulates the drop, and landing clears it.
#[test]
fn fall_distance_accumulates_while_falling_and_clears_on_landing() {
    let (store, registry) = (store(), registry());
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let mut state = PlayerState::new(Vec3::new(8.5, 7.0, 8.5));
    let mut expected = 0.0_f32;
    for _ in 0..40 {
        let tick = Simulator::default()
            .tick(&mut state, MovementInput::default(), &world)
            .unwrap();
        if tick.on_ground {
            assert!(expected > 0.0);
            assert_eq!(state.fall_distance(), 0.0);
            return;
        }
        expected -= tick.movement.y as f32;
        assert_eq!(state.fall_distance(), expected);
    }
    panic!("the player never landed");
}

/// The climber keeps the fall distance while its descending block is powder
/// snow, so a snow-standing wearer's long fall still lands on the fall-into
/// shape rather than the full block.
#[test]
fn a_snow_standing_fall_over_powder_snow_keeps_its_fall_distance() {
    let mut state = PlayerState::new(Vec3::new(4.5, 9.1, 4.5));
    let ticks = run(
        &mut state,
        MovementInput {
            can_stand_on_snow: true,
            ..MovementInput::default()
        },
        12,
    );
    let landing = ticks.iter().find(|tick| tick.on_ground).unwrap();
    assert_eq!(landing.position.y as f32, 4.9_f32, "{landing:?}");
}

/// Ordinary collision queries never see powder snow; only the mover-aware
/// query lists its cell.
#[test]
fn only_the_mover_query_lists_powder_snow() {
    let (store, registry) = (store(), registry());
    let world = PaletteWorld::for_movement(&store, &registry, 0);
    let query = Aabb::new(Vec3::new(4.2, 4.5, 4.2), Vec3::new(4.8, 4.9, 4.8));
    assert!(world.collision_boxes(query).unwrap().value.is_empty());
    assert!(
        world
            .collision_boxes_with_provenance(query)
            .unwrap()
            .value
            .is_empty()
    );
    let mover = world.collision_boxes_for_mover(query).unwrap().value;
    assert!(mover.colliders.is_empty());
    assert_eq!(mover.powder_snow, vec![[4, 4, 4]]);
}

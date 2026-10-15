use super::*;
use crate::{CollisionRegistry, DoorFacing, DoorState};
use world::{BlockUpdate, ChunkStore};

use crate::world::section::SectionCache;

const AIR: u32 = 0;
const STONE: u32 = 1;
const WATER: u32 = 2;
const DOOR_LOWER: u32 = 3;
const DOOR_UPPER: u32 = 4;
const UNREGISTERED: u32 = 99;
const BOTH: TerrainGeometry = TerrainGeometry {
    collision: true,
    selection: true,
};

fn cube() -> Aabb {
    Aabb::new(Vec3::ZERO, Vec3::ONE)
}

fn registry() -> CollisionRegistry {
    let mut registry = CollisionRegistry::new();
    registry.register(AIR, []).unwrap();
    registry.register(STONE, [cube()]).unwrap();
    registry.register(WATER, []).unwrap();
    for (id, upper) in [(DOOR_LOWER, false), (DOOR_UPPER, true)] {
        registry.register(id, [cube()]).unwrap();
        registry.set_door_state(
            id,
            DoorState {
                family: [1; 32],
                facing: DoorFacing::North,
                upper,
                open: false,
                hinge_right: false,
            },
        );
    }
    registry
}

fn set(store: &mut ChunkStore, [x, y, z]: [i32; 3], layer: u32, id: u32) {
    let key = SubChunkKey::new(0, x >> 4, y >> 4, z >> 4);
    let update = BlockUpdate::new(
        x.rem_euclid(16) as u8,
        y.rem_euclid(16) as u8,
        z.rem_euclid(16) as u8,
        layer,
        id,
    );
    store.update_block(key, update, AIR).unwrap();
}

fn cell(sample: &TerrainSample, min: [i32; 3], size: [u8; 3], at: [i32; 3]) -> usize {
    let [_, sy, sz] = size.map(usize::from);
    let [x, y, z] = [0, 1, 2].map(|axis| (at[axis] - min[axis]) as usize);
    let cell = (x * sz + z) * sy + y;
    assert!(cell < sample.states.len() / usize::from(sample.layer_count));
    cell
}

fn layer(sample: &TerrainSample, cell: usize, layer: usize) -> Option<u32> {
    let cells = sample.states.len() / usize::from(sample.layer_count);
    let index = sample.states[layer * cells + cell];
    (index != TERRAIN_NO_LAYER).then(|| sample.palette[usize::from(index)].runtime_id)
}

fn loaded(sample: &TerrainSample, cell: usize) -> bool {
    sample.loaded[cell / 64] & (1 << (cell % 64)) != 0
}

fn shape(sample: &TerrainSample, index: u32) -> Vec<f64> {
    let range = sample.shape_offsets[index as usize] as usize * 6
        ..sample.shape_offsets[index as usize + 1] as usize * 6;
    sample.shape_boxes[range].to_vec()
}

/// Unknown cells never read as air, and authoritative sections without storage do.
#[test]
fn residency_is_explicit_and_storage_free_sections_are_air() {
    let mut store = ChunkStore::new();
    store
        .mark_sub_chunk_loaded(SubChunkKey::new(0, 0, 0, 0))
        .unwrap();
    let registry = registry();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let (min, size) = ([14, 15, 0], [4, 2, 1]);
    let sample = TerrainSampler::default()
        .sample(&world, min, size, BOTH, 64)
        .unwrap();
    let known = cell(&sample, min, size, [15, 15, 0]);
    assert!(loaded(&sample, known));
    assert_eq!(layer(&sample, known, 0), Some(AIR));
    for unknown in [[15, 16, 0], [16, 15, 0]] {
        let unknown = cell(&sample, min, size, unknown);
        assert!(!loaded(&sample, unknown));
        assert_eq!(layer(&sample, unknown, 0), None);
    }
    let unloaded = TerrainColumn {
        x: 1,
        z: 0,
        revision: None,
    };
    assert!(sample.columns.contains(&unloaded));
}

/// Every layer matches the per-block reader across negative coordinates and sections, and
/// sections with fewer storages report no layer rather than air.
#[test]
fn layers_match_the_block_reader_in_documented_order() {
    let mut store = ChunkStore::new();
    for x in -1..=0 {
        store.mark_chunk_loaded(ChunkKey::new(0, x, 0)).unwrap();
    }
    for (index, x) in (-3..3).enumerate() {
        set(&mut store, [x, 15 + (index as i32 % 3), 2], 0, STONE);
    }
    set(&mut store, [-2, 16, 3], 1, WATER);
    let registry = registry();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let (min, size) = ([-3, 14, 1], [6, 4, 3]);
    let sample = TerrainSampler::default()
        .sample(&world, min, size, TerrainGeometry::default(), 64)
        .unwrap();
    assert_eq!(sample.layer_count, 2);
    for x in min[0]..min[0] + 6 {
        for y in min[1]..min[1] + 4 {
            for z in min[2]..min[2] + 3 {
                let index = cell(&sample, min, size, [x, y, z]);
                assert!(loaded(&sample, index));
                let mut section = SectionCache::default();
                let expected = world.layers_at([x, y, z], &mut section).unwrap();
                for layer_index in 0..2 {
                    let actual = layer(&sample, index, layer_index);
                    if layer_index < expected.len() {
                        assert_eq!(actual, expected.get(layer_index), "{x} {y} {z}");
                    } else {
                        assert_eq!(actual, None, "{x} {y} {z} layer {layer_index}");
                    }
                }
            }
        }
    }
    assert!(sample.palette.iter().all(|state| {
        state.collision == TerrainShapeBinding::NotRequested && state.physics.is_some()
    }));
    assert!(sample.shape_boxes.is_empty() && sample.overrides.is_empty());
}

/// Static shapes are shared; paired doors resolve per cell and fail closed without a neighbour.
#[test]
fn positional_geometry_overrides_only_its_occurrences() {
    let mut store = ChunkStore::new();
    store
        .mark_sub_chunk_loaded(SubChunkKey::new(0, 1, 0, 0))
        .unwrap();
    for z in 0..2 {
        set(&mut store, [16, 1, z], 0, STONE);
    }
    set(&mut store, [17, 0, 0], 0, DOOR_LOWER);
    set(&mut store, [17, 1, 0], 0, DOOR_UPPER);
    set(&mut store, [18, 15, 0], 0, DOOR_LOWER);
    set(&mut store, [19, 0, 0], 0, UNREGISTERED);
    let registry = registry();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let (min, size) = ([16, 0, 0], [4, 16, 2]);
    let sample = TerrainSampler::default()
        .sample(&world, min, size, BOTH, 64)
        .unwrap();
    let state = |id| {
        *sample
            .palette
            .iter()
            .find(|state| state.runtime_id == id)
            .unwrap()
    };
    let TerrainShapeBinding::Fixed(TerrainShape::Known(stone)) = state(STONE).collision else {
        panic!("stone has static collision");
    };
    assert_eq!(shape(&sample, stone), [0., 0., 0., 1., 1., 1.]);
    assert_eq!(state(STONE).selection, state(STONE).collision);
    assert_eq!(
        state(AIR).collision,
        TerrainShapeBinding::Fixed(TerrainShape::Known(TERRAIN_EMPTY_SHAPE))
    );
    let unknown = state(UNREGISTERED);
    assert_eq!(unknown.physics, None);
    assert_eq!(
        unknown.collision,
        TerrainShapeBinding::Fixed(TerrainShape::Unavailable)
    );
    assert_eq!(state(DOOR_LOWER).collision, TerrainShapeBinding::PerCell);
    assert_eq!(sample.overrides.len(), 3);
    let at = |position| {
        let slot = cell(&sample, min, size, position) as u32;
        *sample
            .overrides
            .iter()
            .find(|entry| entry.slot == slot)
            .unwrap()
    };
    let Some(TerrainShape::Known(door)) = at([17, 0, 0]).collision else {
        panic!("paired door resolves");
    };
    assert_eq!(shape(&sample, door).len(), 6);
    assert_eq!(at([17, 1, 0]).collision, Some(TerrainShape::Known(door)));
    assert_eq!(at([17, 0, 0]).selection, Some(TerrainShape::Known(door)));
    assert_eq!(at([18, 15, 0]).collision, Some(TerrainShape::Unavailable));
    assert!(
        sample
            .overrides
            .windows(2)
            .all(|pair| pair[0].slot < pair[1].slot)
    );
}

/// Box admission fails before emitting more distinct boxes than allowed.
#[test]
fn distinct_boxes_are_bounded() {
    let mut store = ChunkStore::new();
    store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    set(&mut store, [0, 0, 0], 0, STONE);
    let registry = registry();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let mut sampler = TerrainSampler::default();
    assert_eq!(
        sampler.sample(&world, [0; 3], [1; 3], BOTH, 0),
        Err(TerrainSampleError::TooManyBoxes)
    );
    assert!(sampler.sample(&world, [0; 3], [1; 3], BOTH, 1).is_ok());
}

/// Volumes ending at the largest block coordinate stay in range; past it they are refused.
#[test]
fn volumes_at_the_coordinate_limit_neither_overflow_nor_wrap() {
    let store = ChunkStore::new();
    let registry = registry();
    let world = PaletteWorld::new(&store, &registry, 0, Default::default());
    let mut sampler = TerrainSampler::default();
    for min in [[i32::MAX - 1; 3], [i32::MAX - 16; 3], [i32::MIN; 3]] {
        let size = [min[0].abs_diff(i32::MAX).clamp(1, 16) as u8; 3];
        let sample = sampler.sample(&world, min, size, BOTH, 64).unwrap();
        assert!(sample.loaded.iter().all(|&word| word == 0));
        assert!(sample.states.iter().all(|&state| state == TERRAIN_NO_LAYER));
    }
    assert_eq!(
        sampler.sample(&world, [i32::MAX - 1, 0, 0], [2, 1, 1], BOTH, 64),
        Err(TerrainSampleError::OutOfRange)
    );
}

/// Actor-owned selection needs a versioned owner column; an absent one is unavailable.
#[test]
fn piston_arm_selection_requires_a_loaded_owner_column() {
    use assets::block_shapes::{
        CompiledBlockShapes, OutlineContext, PISTON_DIRECTIONS, PickRule, SelectionContext,
        ShapeBox, ShapeFlags,
    };
    const ARM: u32 = 5;
    let facing = PISTON_DIRECTIONS
        .iter()
        .position(|direction| *direction == [1, 0, 0])
        .unwrap();
    let piece = ShapeBox::new([0.25; 3], [0.75; 3]);
    let shapes = CompiledBlockShapes {
        state_identity: [3; 32],
        actor_facing: Some(facing as u8),
        actor_visual: None,
        selection: Box::new([piece]),
        selection_context: SelectionContext::PistonArm,
        contextual_selection: Box::new([]),
        outline: None,
        connected_outline: None,
        outline_context: OutlineContext::Static,
        pick_rule: PickRule::Always,
        flags: ShapeFlags::empty(),
        door: None,
        flow: None,
        tags: Box::new([]),
    };
    let mut registry = registry();
    registry.register(ARM, [cube()]).unwrap();
    assert!(registry.set_compiled_selection(ARM, &shapes));
    let mut store = ChunkStore::new();
    store
        .mark_sub_chunk_loaded(SubChunkKey::new(0, 1, 0, 0))
        .unwrap();
    set(&mut store, [16, 0, 0], 0, ARM);
    let (min, size) = ([16, 0, 0], [1, 1, 1]);
    let selection = |store: &ChunkStore| {
        let world = PaletteWorld::new(store, &registry, 0, Default::default());
        let sample = TerrainSampler::default()
            .sample(&world, min, size, BOTH, 64)
            .unwrap();
        let owner = TerrainColumn {
            x: 0,
            z: 0,
            revision: store
                .collision_revision(ChunkKey::new(0, 0, 0))
                .map(|revision| revision.revision),
        };
        assert!(sample.columns.contains(&owner));
        sample.overrides[0].selection
    };
    assert_eq!(selection(&store), Some(TerrainShape::Unavailable));
    store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
    assert!(matches!(selection(&store), Some(TerrainShape::Known(_))));
}

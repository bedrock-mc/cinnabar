use super::{fixtures, id, record, store};
use assets::{NetworkIdMode, facts::BlockProperty as P, registry::ModelFamily};
use serde_json::{Map, Value};
use sim::CollisionWorld;
use world::{BlockUpdate, ChunkStore, SubChunkKey};

const LOWER: [i32; 3] = [8, 8, 8];
const UPPER: [i32; 3] = [8, 9, 8];

/// Writes a fixture cell using the palette's own air identity.
fn put(store: &mut ChunkStore, mode: NetworkIdMode, position: [i32; 3], block: u32) {
    store
        .update_block(
            SubChunkKey::new(0, position[0] >> 4, position[1] >> 4, position[2] >> 4),
            BlockUpdate::new(
                (position[0] & 15) as u8,
                (position[1] & 15) as u8,
                (position[2] & 15) as u8,
                0,
                block,
            ),
            id(record("minecraft:air", &[]), mode),
        )
        .unwrap();
}

/// Resolves modified properties through the same retained palette as gameplay.
fn resolve(mode: NetworkIdMode, source: u32, states: &Map<String, Value>) -> u32 {
    let registries = &fixtures().1;
    registries
        .block_state_runtime_id(
            mode,
            assets::facts::BlockStateIdentity::Compiled(
                registries.block_facts(mode, source).identity.unwrap(),
            ),
            states,
        )
        .unwrap()
}

#[test]
fn door_toggle_predicts_the_lower_open_state_from_either_half_in_both_palettes() {
    let (records, registries) = fixtures();
    let mut exercised = 0;
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for record in records {
            let source = id(record, mode);
            let fact = registries.block_facts(mode, source);
            if fact.family != ModelFamily::Door
                || !fact
                    .traits
                    .contains(assets::registry::BlockTraits::TOGGLES_BY_HAND)
            {
                continue;
            }
            let mut lower: Map<String, Value> =
                serde_json::from_str(&record.canonical_state).unwrap();
            if crate::placement_state::value(&lower, P::UpperBlockBit.name())
                .unwrap()
                .as_u64()
                != Some(0)
            {
                continue;
            }
            let open = crate::placement_state::value(&lower, P::OpenBit.name())
                .unwrap()
                .as_u64()
                == Some(1);
            let mut upper = lower.clone();
            crate::placement_state::set_bit(&mut upper, P::UpperBlockBit.name(), true).unwrap();
            // The upper half's independent open field does not own the pair's pose.
            crate::placement_state::set_bit(&mut upper, P::OpenBit.name(), !open).unwrap();
            let upper_id = resolve(mode, source, &upper);
            crate::placement_state::set_bit(&mut lower, P::OpenBit.name(), !open).unwrap();
            let expected = resolve(mode, source, &lower);
            let mut store = store(mode);
            put(&mut store, mode, LOWER, source);
            put(&mut store, mode, UPPER, upper_id);
            let world =
                sim::PaletteWorld::new(&store, registries.registry(mode), 0, Default::default());
            for (position, clicked) in [(LOWER, source), (UPPER, upper_id)] {
                assert_eq!(
                    crate::block_use::predicted_toggle(registries, mode, &world, position, clicked,),
                    Some((LOWER, expected)),
                    "{} {} clicked {position:?} in {mode:?}",
                    record.name,
                    record.canonical_state,
                );
            }
            exercised += 1;
        }
    }
    assert!(exercised > 0);
}

#[test]
fn door_toggle_defers_when_the_partner_is_missing_or_another_block_type() {
    let registries = &fixtures().1;
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let lower = id(
            record(
                "minecraft:wooden_door",
                &[(P::UpperBlockBit.name(), 0.into())],
            ),
            mode,
        );
        let other = id(record("minecraft:iron_door", &[]), mode);
        for partner in [id(record("minecraft:air", &[]), mode), other] {
            let mut store = store(mode);
            put(&mut store, mode, LOWER, lower);
            put(&mut store, mode, UPPER, partner);
            let world =
                sim::PaletteWorld::new(&store, registries.registry(mode), 0, Default::default());
            assert_eq!(
                crate::block_use::predicted_toggle(registries, mode, &world, LOWER, lower),
                None,
            );
        }
    }
}

#[test]
fn single_cell_switch_toggle_keeps_the_clicked_position() {
    let registries = &fixtures().1;
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let closed = record("minecraft:trapdoor", &[(P::OpenBit.name(), 0.into())]);
        let source = id(closed, mode);
        let mut states: Map<String, Value> = serde_json::from_str(&closed.canonical_state).unwrap();
        crate::placement_state::set_bit(&mut states, P::OpenBit.name(), true).unwrap();
        let expected = resolve(mode, source, &states);
        let mut store = store(mode);
        put(&mut store, mode, LOWER, source);
        let world =
            sim::PaletteWorld::new(&store, registries.registry(mode), 0, Default::default());
        assert_eq!(
            crate::block_use::predicted_toggle(registries, mode, &world, LOWER, source),
            Some((LOWER, expected)),
        );
    }
}

#[test]
fn door_toggle_reads_the_partner_across_a_subchunk_boundary_and_moves_both_shapes() {
    let registries = &fixtures().1;
    let lower_position = [8, 15, 8];
    let upper_position = [8, 16, 8];
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let lower = id(
            record(
                "minecraft:wooden_door",
                &[
                    (P::UpperBlockBit.name(), 0.into()),
                    (P::OpenBit.name(), 0.into()),
                ],
            ),
            mode,
        );
        let upper = id(
            record(
                "minecraft:wooden_door",
                &[(P::UpperBlockBit.name(), 1.into())],
            ),
            mode,
        );
        let mut store = store(mode);
        put(&mut store, mode, lower_position, lower);
        let predict = |store: &ChunkStore| {
            let world =
                sim::PaletteWorld::new(store, registries.registry(mode), 0, Default::default());
            crate::block_use::predicted_toggle(registries, mode, &world, lower_position, lower)
        };
        assert_eq!(predict(&store), None, "unloaded upper cell");
        store
            .mark_sub_chunk_loaded(SubChunkKey::new(0, 0, 1, 0))
            .unwrap();
        put(&mut store, mode, upper_position, upper);
        let query = sim::Aabb::new(
            sim::Vec3::new(8.0, 15.0, 8.0),
            sim::Vec3::new(9.0, 17.0, 9.0),
        );
        let boxes = |store: &ChunkStore| {
            sim::PaletteWorld::new(store, registries.registry(mode), 0, Default::default())
                .collision_boxes(query)
                .unwrap()
                .value
        };
        let before = boxes(&store);
        let (position, predicted) = predict(&store).expect("loaded matching pair");
        assert_eq!(position, lower_position);
        put(&mut store, mode, position, predicted);
        let after = boxes(&store);
        assert_eq!((before.len(), after.len()), (2, 2));
        for (before, after) in before.iter().zip(&after) {
            assert_ne!(before, after, "both halves rotate after the lower update");
        }
        assert_eq!(
            sim::PaletteWorld::new(&store, registries.registry(mode), 0, Default::default())
                .primary_runtime_id(upper_position)
                .unwrap(),
            upper,
        );
        // An authoritative state restores both shapes without changing the upper field.
        put(&mut store, mode, lower_position, lower);
        assert_eq!(boxes(&store), before);
    }
}

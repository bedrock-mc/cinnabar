use super::*;
use assets::{NetworkIdMode, RuntimeAssets, facts::BlockProperty as P, facts::StateValue};
use std::sync::Arc;
use world::{BlockUpdate, ChunkStore, SubChunkKey};

/// Selects source identities from the pinned registry in either session ID space.
fn fixture(mode: NetworkIdMode) -> (DecodeIds, u32, u32) {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let id = |name: &str, source: bool| {
        let record = records
            .iter()
            .find(|record| {
                record.name.as_ref() == name
                    && (!source || record.property(P::LiquidDepth) == Some(&StateValue::Int(0)))
            })
            .unwrap();
        match mode {
            NetworkIdMode::Sequential => record.sequential_id,
            NetworkIdMode::Hashed => record.network_hash,
        }
    };
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let biome_tints = Arc::new(assets.biome_assets().resolve_live(&[]).unwrap());
    let ids = DecodeIds {
        assets,
        custom_blocks: 0..0,
        custom_identities: Arc::new(Default::default()),
        remap: Arc::new(assets::SequentialIdRemap::new([])),
        diagnostics: Arc::new(Default::default()),
        session_id: 0,
        mode,
        air: id("minecraft:air", false),
        biome_tints,
        default_biome: 0,
    };
    (
        ids,
        id("minecraft:water", true),
        id("minecraft:oak_fence", false),
    )
}

#[test]
fn primary_fence_update_moves_source_water_to_the_secondary_layer() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let (ids, water, fence) = fixture(mode);
        let key = SubChunkKey::new(0, 0, 0, 0);
        let update = |runtime_id| BlockUpdate {
            x: 1,
            y: 2,
            z: 3,
            layer: 0,
            runtime_id,
        };
        let previous =
            ChunkStore::prepare_sub_chunk_blocks(key, None, &[update(water)], ids.air).unwrap();
        let result = super::super::prepare::prepare_resolved_block_mutations(
            vec![BlockMutationBatch {
                key,
                previous: previous.replacement().cloned().map(Arc::new),
                updates: vec![update(fence)],
            }],
            &ids,
        )
        .unwrap();
        let replacement = result.mutations[0].replacement().unwrap();
        assert_eq!(replacement.runtime_id(0, 1, 2, 3), Some(fence));
        assert_eq!(replacement.runtime_id(1, 1, 2, 3), Some(water), "{mode:?}");
    }
}

/// Reads a named pinned state for a transition control without duplicating runtime IDs.
fn state(mode: NetworkIdMode, name: &str, depth: Option<i32>) -> u32 {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let record = records
        .iter()
        .find(|record| {
            record.name.as_ref() == name
                && depth.is_none_or(|depth| {
                    record.property(P::LiquidDepth) == Some(&StateValue::Int(depth))
                })
        })
        .unwrap();
    match mode {
        NetworkIdMode::Sequential => record.sequential_id,
        NetworkIdMode::Hashed => record.network_hash,
    }
}

#[test]
fn source_water_retention_respects_replacements_and_explicit_layer_order() {
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let (ids, water, fence) = fixture(mode);
        let stone = state(mode, "minecraft:stone", None);
        let lava = state(mode, "minecraft:lava", Some(0));
        let flowing = state(mode, "minecraft:water", Some(1));
        let key = SubChunkKey::new(0, 0, 0, 0);
        let update = |layer, runtime_id| BlockUpdate::new(1, 2, 3, layer, runtime_id);
        for (initial, updates, expected_primary, expected_secondary) in [
            (water, vec![update(0, fence)], fence, water),
            (water, vec![update(0, stone)], stone, ids.air),
            (water, vec![update(0, ids.air)], ids.air, ids.air),
            (lava, vec![update(0, fence)], fence, ids.air),
            (flowing, vec![update(0, fence)], fence, ids.air),
            (water, vec![update(0, u32::MAX)], u32::MAX, ids.air),
            (
                ids.air,
                vec![update(0, water), update(0, fence)],
                fence,
                water,
            ),
            (
                water,
                vec![update(0, fence), update(0, fence)],
                fence,
                water,
            ),
            (
                water,
                vec![update(0, fence), update(1, ids.air)],
                fence,
                ids.air,
            ),
            (
                water,
                vec![update(0, fence), update(0, water)],
                water,
                ids.air,
            ),
            (
                water,
                vec![update(0, fence), update(0, stone)],
                stone,
                ids.air,
            ),
            (
                water,
                vec![update(0, fence), update(0, water), update(0, ids.air)],
                ids.air,
                ids.air,
            ),
            (water, vec![update(1, lava), update(0, fence)], fence, lava),
            (water, vec![update(0, fence), update(1, lava)], fence, lava),
        ] {
            let previous =
                ChunkStore::prepare_sub_chunk_blocks(key, None, &[update(0, initial)], ids.air)
                    .unwrap();
            let result = super::super::prepare_block_mutation(
                key,
                previous.replacement(),
                &updates,
                ids.air,
                mode,
            )
            .unwrap();
            let sample = |layer| {
                result
                    .replacement()
                    .and_then(|chunk| chunk.runtime_id(layer, 1, 2, 3))
                    .unwrap_or(ids.air)
            };
            assert_eq!(sample(0), expected_primary, "{mode:?}: {updates:?}");
            assert_eq!(sample(1), expected_secondary, "{mode:?}: {updates:?}");
        }
    }
}

#[test]
fn water_mutation_rejects_invalid_cells_before_sampling() {
    let (ids, water, fence) = fixture(NetworkIdMode::Sequential);
    let key = SubChunkKey::new(0, 0, 0, 0);
    let previous = ChunkStore::prepare_sub_chunk_blocks(
        key,
        None,
        &[BlockUpdate::new(1, 2, 3, 0, water)],
        ids.air,
    )
    .unwrap();
    let invalid = [
        BlockUpdate::new(16, 2, 3, 0, fence),
        BlockUpdate::new(1, 2, 3, world::MAX_STORAGE_COUNT as u32, fence),
    ];
    for update in invalid {
        assert!(
            super::super::prepare_block_mutation(
                key,
                previous.replacement(),
                &[update],
                ids.air,
                ids.mode
            )
            .is_err()
        );
    }
    assert_eq!(
        previous.replacement().unwrap().runtime_id(0, 1, 2, 3),
        Some(water)
    );
}

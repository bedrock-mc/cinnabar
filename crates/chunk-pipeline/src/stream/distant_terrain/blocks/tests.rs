use std::sync::RwLock;

use lod::registry::MaterialRegistry;

use super::*;
use crate::stream::tests::camera_medium_assets;

/// Ids of [`camera_medium_assets`].
const AIR: u32 = 0;
const WATER: u32 = 1;
const LAVA: u32 = 2;
const STONE: u32 = 3;
const CUSTOM: u32 = 500;

fn blocks(mode: NetworkIdMode) -> SessionBlocks {
    let custom = HashMap::from([(
        CUSTOM,
        CustomBlockState {
            name: "example:lamp".into(),
            states: "example:lit=true".into(),
        },
    )]);
    SessionBlocks::new(Arc::new(camera_medium_assets()), mode, Arc::new(custom))
}

#[test]
fn asset_visuals_decide_classes_and_unknown_ids_are_air() {
    let blocks = blocks(NetworkIdMode::Sequential);
    for (value, class) in [
        (AIR, BlockClass::Air),
        (WATER, BlockClass::Water),
        (LAVA, BlockClass::Opaque),
        (STONE, BlockClass::Opaque),
        (99, BlockClass::Air),
    ] {
        assert_eq!(blocks.class_of_value(value), class, "id {value}");
    }
}

#[test]
fn vanilla_states_key_by_network_hash_and_custom_states_by_name() {
    let sequential = blocks(NetworkIdMode::Sequential);
    let Some(MaterialKey::Network(hash)) = sequential.key(STONE) else {
        panic!("sequential ids key by their pinned network hash");
    };
    assert_eq!(assets::pinned_block_sequential_id(hash), Some(STONE));
    let hashed = blocks(NetworkIdMode::Hashed);
    assert_eq!(hashed.key(hash), Some(MaterialKey::Network(hash)));
    let custom = MaterialKey::Custom(
        CustomStateKey::new("example:lamp", "example:lit=true").expect("short key"),
    );
    assert_eq!(sequential.key(CUSTOM), Some(custom.clone()));
    assert_eq!(hashed.key(CUSTOM), Some(custom));
}

#[test]
fn stored_materials_classify_through_their_keys() {
    let blocks = Arc::new(blocks(NetworkIdMode::Sequential));
    let registry: SharedRegistry = Arc::new(RwLock::new(MaterialRegistry::new()));
    let mut classifier = ColumnClassifier::new(&blocks, &registry);
    assert_eq!(classifier.classify(AIR), SampledBlock::AIR);
    let water = classifier.classify(WATER);
    let stone = classifier.classify(STONE);
    assert_eq!(water.class, BlockClass::Water);
    assert_eq!(stone.class, BlockClass::Opaque);
    assert_eq!(classifier.classify(STONE), stone, "one material per state");
    assert_eq!(
        MaterialClasses::class(&classifier, water.material),
        BlockClass::Water
    );

    let mut table = MaterialClassTable::default();
    table.refresh(&blocks, &registry);
    assert_eq!(table.class(water.material), BlockClass::Water);
    assert_eq!(table.class(stone.material), BlockClass::Opaque);
    let lava = classifier.classify(LAVA);
    table.refresh(&blocks, &registry);
    assert_eq!(table.class(lava.material), BlockClass::Opaque);
    assert_eq!(
        table.class(MaterialId::new(77).expect("id")),
        BlockClass::Opaque,
        "unassigned materials are opaque"
    );
}

#[test]
fn values_stored_as_air_are_counted_once_per_column() {
    let blocks = Arc::new(blocks(NetworkIdMode::Sequential));
    let registry: SharedRegistry = Arc::new(RwLock::new(MaterialRegistry::new()));
    {
        let mut registry = registry.write().expect("registry");
        let mut hash = 0_u32;
        while registry.len() < lod::registry::MAX_MATERIALS {
            if assets::pinned_block_sequential_id(hash).is_none() {
                registry
                    .get_or_insert(&MaterialKey::Network(hash))
                    .expect("room");
            }
            hash += 1;
        }
    }
    let mut classifier = ColumnClassifier::new(&blocks, &registry);
    for _ in 0..3 {
        assert_eq!(classifier.classify(99), SampledBlock::AIR, "unknown id");
        assert_eq!(
            classifier.classify(STONE),
            SampledBlock::AIR,
            "registry full"
        );
        assert_eq!(classifier.classify(AIR), SampledBlock::AIR);
    }
    assert_eq!(
        classifier.counts(),
        ClassifierCounts {
            unknown_ids: 1,
            unkeyed_ids: 0,
            registry_full: 1,
        }
    );
}

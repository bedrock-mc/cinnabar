use super::*;
use crate::inventory::recipes::{
    budget::Credits,
    model::{Batch, Ingredient, Output, RecipeUpdate, Record},
};
use crate::{ItemRegistryEntry, ItemRegistryVersion};
use std::{num::NonZeroU64, sync::Arc};

fn registry(capacity: Option<u8>) -> RecipeRegistrySnapshot {
    RecipeRegistrySnapshot::new(
        NonZeroU64::new(1).unwrap(),
        vec![ItemRegistryEntry {
            identifier: Arc::from("minecraft:planks"),
            network_id: 7,
            component_based: false,
            version: ItemRegistryVersion::None,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: capacity,
            canonical_empty_component_data: true,
            item_tags: std::sync::Arc::from([]),
        }]
        .into(),
    )
    .unwrap()
}

fn catalog(count: usize) -> RecipeCatalog {
    let owner = Credits::isolated(65536);
    let update = RecipeUpdate {
        screen: None,
        batch: Some(Arc::new(Batch {
            records: (0..count)
                .map(|index| Record {
                    id: index as u32 + 1,
                    recipe: Some(Recipe {
                        width: 1,
                        height: 1,
                        shapeless: false,
                        mirror: false,
                        priority: 0,
                        ingredients: std::array::from_fn(|index| {
                            (index == 0).then(|| Ingredient {
                                name: "minecraft:oak_log".into(),
                                tag: false,
                                aux: 0,
                                count: 1,
                            })
                        }),
                        output: Output {
                            id: 7,
                            aux: 0,
                            count: 4,
                            block: 5,
                            empty_envelope: true,
                        },
                    }),
                })
                .collect(),
            clear: true,
            _permit: owner.reserve(32768).unwrap(),
        })),
    };
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    catalog
}

#[test]
fn observations_are_bounded_structural_facts_not_names_or_handles() {
    let catalog = catalog(9);
    let result = catalog.observations(&registry(Some(4)));
    assert_eq!(result.supported_count, 9);
    assert!(result.truncated);
    assert_eq!(result.entries.iter().flatten().count(), 8);
    assert_eq!(
        result
            .entries
            .iter()
            .flatten()
            .map(|entry| entry.recipe_id)
            .collect::<Vec<_>>(),
        (1..=8).collect::<Vec<_>>()
    );
    let first = result.entries[0].unwrap();
    assert_eq!(first.recipe_id, 1);
    assert_eq!(first.dimensions, [1, 1]);
    assert_eq!(first.output_fits_capacity, Some(true));
    assert!(first.output_binding_supported);
    assert_eq!(
        first.ingredients[0].unwrap().name_sha256,
        <[u8; 32]>::from(Sha256::digest(b"minecraft:oak_log"))
    );
    let encoded = serde_json::to_string(&result).unwrap();
    assert!(encoded.len() <= 8192);
    assert!(!encoded.contains("minecraft:"));
    assert!(!encoded.contains("oak_log"));
    assert!(!encoded.contains("planks"));
}

#[test]
fn unknown_capacity_and_missing_registry_are_not_defaulted() {
    let catalog = catalog(1);
    let observed = catalog.observations(&registry(None)).entries[0].unwrap();
    assert_eq!(observed.output_capacity, None);
    assert_eq!(observed.output_fits_capacity, None);
    let missing = RecipeRegistrySnapshot::new(NonZeroU64::new(2).unwrap(), Arc::from([])).unwrap();
    let observed = catalog.observations(&missing).entries[0].unwrap();
    assert!(!observed.output_binding_supported);
    assert_eq!(observed.output_name_sha256, None);
    assert_eq!(
        catalog.observations(&registry(Some(3))).entries[0]
            .unwrap()
            .output_fits_capacity,
        Some(false)
    );
}

#[test]
fn stale_and_applied_unavailable_updates_remain_distinct() {
    let mut catalog = catalog(1);
    let registry = registry(Some(64));
    let before = catalog.observations(&registry);
    let unavailable = RecipeUpdate {
        batch: None,
        screen: None,
    };
    assert!(!catalog.apply(1, 1, &unavailable));
    assert_eq!(catalog.observations(&registry), before);
    assert!(catalog.apply(1, 2, &unavailable));
    let unavailable = catalog.observations(&registry);
    assert!(!unavailable.available);
    assert_eq!(unavailable.supported_count, 0);
    assert!(unavailable.entries.iter().all(Option::is_none));
}

#[test]
fn unsupported_advertised_records_are_not_invented_as_supported_recipes() {
    let owner = Credits::isolated(4096);
    let update = RecipeUpdate {
        screen: None,
        batch: Some(Arc::new(Batch {
            records: vec![Record {
                id: 123,
                recipe: None,
            }],
            clear: true,
            _permit: owner.reserve(512).unwrap(),
        })),
    };
    assert_eq!(update.record_count(), 1);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    let observed = catalog.observations(&registry(Some(64)));
    assert!(observed.available);
    assert_eq!(observed.supported_count, 0);
    assert!(observed.entries.iter().all(Option::is_none));
}

#[test]
fn maximal_fixed_summary_fits_reserved_bank_without_raw_text() {
    let registry = registry(Some(u8::MAX));
    let mut catalog = catalog(8);
    let result = catalog.observations(&registry);
    let mut maximal = result;
    maximal.revision = u64::MAX;
    maximal.supported_count = u16::MAX;
    for entry in maximal.entries.iter_mut().flatten() {
        entry.recipe_id = u32::MAX;
        entry.dimensions = [u8::MAX; 2];
        entry.ingredients = [Some(IngredientObservation {
            name_sha256: [255; 32],
            aux: u16::MAX,
            count: u8::MAX,
        }); 4];
        entry.output_name_sha256 = Some([255; 32]);
        entry.output_id = i32::MIN;
        entry.output_block = u32::MAX;
        entry.output_aux = u16::MAX;
        entry.output_count = u8::MAX;
        entry.output_capacity = Some(u16::MAX);
        entry.output_binding_supported = false;
        entry.output_fits_capacity = Some(false);
    }
    assert!(serde_json::to_vec(&maximal).unwrap().len() <= 8192);
    catalog.begin_session(2);
    assert!(!catalog.observations(&registry).available);
}

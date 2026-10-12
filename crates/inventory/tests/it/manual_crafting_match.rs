use ::protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture, recipe_update_from_batch,
};
use ::protocol::*;
use inventory::ManualCraftCell::{Empty, Present, Unknown};
use inventory::{
    ManualCraftInput, ManualCraftMatch, ManualCraftSnapshot, manual_craft_packet, match_manual_grid,
};
use sha2::{Digest, Sha256};
use std::{num::NonZeroU64, sync::Arc};

fn entries() -> Arc<[ItemRegistryEntry]> {
    [(6, "minecraft:oak_log"), (7, "minecraft:oak_planks")]
        .into_iter()
        .map(|(network_id, name)| ItemRegistryEntry {
            components: None,
            identifier: Arc::from(name),
            network_id,
            component_based: false,
            version: ItemRegistryVersion::None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: std::sync::Arc::from([]),
        })
        .collect()
}
fn registry(entries: Arc<[ItemRegistryEntry]>, revision: u64) -> RecipeRegistrySnapshot {
    RecipeRegistrySnapshot::new(NonZeroU64::new(revision).unwrap(), entries).unwrap()
}
fn stack(id: i32, count: u16, metadata: u32) -> VerifiedNetworkItemStack {
    let digest: [u8; 32] = Sha256::digest([]).into();
    VerifiedNetworkItemStack::try_new(
        NetworkItemStack {
            network_id: 6,
            count,
            metadata,
            stack_network_id: id,
            nbt_digest: digest,
            block_runtime_id: 0,
            extra_data: Arc::from([]),
        },
        digest,
    )
    .unwrap()
}
fn recipe(id: u32, height: i32, amount: i32) -> RecipeFixture {
    let ingredient = IngredientFixture {
        count: amount,
        ..IngredientFixture::named("minecraft:oak_log", 0)
    };
    let output = OutputFixture {
        user_data: vec![0; 10],
        ..OutputFixture::new(7, 4)
    };
    RecipeFixture {
        net_id: id,
        ..RecipeFixture::shaped(
            "test:preview",
            1,
            height,
            (0..height).map(|_| ingredient.clone()).collect(),
            vec![output],
        )
    }
}
fn update(recipes: Vec<RecipeFixture>, clear: bool) -> RecipeUpdate {
    CraftingDataFixture {
        recipes,
        clear_recipes: clear,
    }
    .update()
}
fn catalog(recipes: Vec<RecipeFixture>) -> RecipeCatalog {
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update(recipes, true)));
    catalog
}

#[test]
fn unique_preview_contains_display_data_not_request_identity() {
    let catalog = catalog(vec![recipe(17, 1, 1)]);
    let registry = registry(entries(), 1);
    let input = stack(-1, 1, 0);
    let ManualCraftMatch::Unique(preview) = match_manual_grid(
        &catalog,
        1,
        &registry,
        &[Present(&input), Empty, Empty, Empty],
    ) else {
        panic!("unique preview")
    };
    assert_eq!(preview.identifier.as_ref(), "minecraft:oak_planks");
    assert_eq!(
        (preview.count, preview.metadata, preview.block_runtime_id),
        (4, 0, 0)
    );
    // A known display stack need not have a usable request stack ID. The
    // atomic request still rejects that same unproven identity.
    let cursor = NetworkItemStack::empty();
    let digest = cursor.nbt_digest;
    let cursor = VerifiedNetworkItemStack::try_new(cursor, digest).unwrap();
    assert!(
        manual_craft_packet(
            ManualCraftSnapshot {
                session: 1,
                catalog: &catalog,
                registry: registry.entries(),
                inputs: [
                    Some(ManualCraftInput {
                        slot: 28,
                        stack: input
                    }),
                    None,
                    None,
                    None
                ],
                cursor: &cursor,
            },
            17,
            -3
        )
        .is_err()
    );
}

#[test]
fn shape_is_top_left_stride_two_and_metadata_and_counts_are_exact() {
    let catalog = catalog(vec![recipe(18, 2, 2)]);
    let registry = registry(entries(), 1);
    let enough = stack(101, 2, 0);
    let short = stack(102, 1, 0);
    let wrong_aux = stack(103, 2, 1);
    assert!(matches!(
        match_manual_grid(
            &catalog,
            1,
            &registry,
            &[Present(&enough), Empty, Present(&enough), Empty]
        ),
        ManualCraftMatch::Unique(_)
    ));
    for grid in [
        [Present(&enough), Present(&enough), Empty, Empty],
        [Present(&enough), Empty, Present(&short), Empty],
        [Present(&enough), Empty, Present(&wrong_aux), Empty],
        [Empty, Empty, Empty, Empty],
    ] {
        assert_eq!(
            match_manual_grid(&catalog, 1, &registry, &grid),
            ManualCraftMatch::NoMatch
        );
    }
}

#[test]
fn ambiguous_or_retired_candidates_never_choose_a_recipe() {
    let mut catalog = catalog(vec![recipe(17, 1, 1), recipe(18, 1, 1)]);
    let registry = registry(entries(), 1);
    let input = stack(101, 1, 0);
    let grid = [Present(&input), Empty, Empty, Empty];
    assert_eq!(
        match_manual_grid(&catalog, 1, &registry, &grid),
        ManualCraftMatch::Ambiguous
    );
    assert_eq!(
        match_manual_grid(&catalog, 2, &registry, &grid),
        ManualCraftMatch::Unavailable
    );
    catalog.apply(1, 2, &update(vec![], true));
    assert_eq!(
        match_manual_grid(&catalog, 1, &registry, &grid),
        ManualCraftMatch::NoMatch
    );
    let unavailable = decode_recipe_update(&[0x81, 0x40]).unwrap();
    catalog.apply(1, 3, &unavailable);
    assert_eq!(
        match_manual_grid(&catalog, 1, &registry, &grid),
        ManualCraftMatch::Unavailable
    );
}

#[test]
fn registry_binding_and_capacity_are_current_immutable_inputs() {
    let catalog = catalog(vec![recipe(17, 1, 1)]);
    let original = registry(entries(), 1);
    let cloned = original.clone();
    assert!(original.same_authority(&cloned));
    let input = stack(101, 1, 0);
    let grid = [Present(&input), Empty, Empty, Empty];
    let mut replacement = entries().to_vec();
    replacement[1].negotiated_max_stack_size = None;
    let replacement = registry(replacement.into(), 2);
    assert!(!original.same_authority(&replacement));
    assert_eq!(replacement.revision().get(), 2);
    assert_eq!(
        match_manual_grid(&catalog, 1, &replacement, &grid),
        ManualCraftMatch::NoMatch
    );
    assert!(matches!(
        match_manual_grid(&catalog, 1, &cloned, &grid),
        ManualCraftMatch::Unique(_)
    ));
    let mut replacement = entries().to_vec();
    replacement[1].identifier = Arc::from("minecraft:birch_planks");
    let replacement = registry(replacement.into(), 3);
    let ManualCraftMatch::Unique(preview) = match_manual_grid(&catalog, 1, &replacement, &grid)
    else {
        panic!("preview")
    };
    assert_eq!(preview.identifier.as_ref(), "minecraft:birch_planks");
}

#[test]
fn existing_pinned_recipe_fixture_is_matchable_without_materializing_a_request() {
    let update = recipe_update_from_batch(include_bytes!(
        "../../../protocol/fixtures/crafting_data_manual_named_1x1.bin"
    ));
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    catalog.apply(1, 1, &update);
    let registry = registry(entries(), 1);
    let input = stack(101, 1, 0);
    assert!(matches!(
        match_manual_grid(
            &catalog,
            1,
            &registry,
            &[Present(&input), Empty, Empty, Empty]
        ),
        ManualCraftMatch::Unique(_)
    ));
}

#[test]
fn unsupported_descriptors_and_output_bindings_do_not_become_previews() {
    let input = stack(101, 1, 0);
    let grid = [Present(&input), Empty, Empty, Empty];
    let mut tagged = recipe(17, 1, 1);
    tagged.ingredients[0].descriptor[0].0 = "item_tag".into();
    assert_eq!(
        match_manual_grid(&catalog(vec![tagged]), 1, &registry(entries(), 1), &grid),
        ManualCraftMatch::NoMatch
    );
    let catalog = catalog(vec![recipe(17, 1, 1)]);
    let mut changed = entries().to_vec();
    changed[1].negotiated_max_stack_size = Some(3);
    assert_eq!(
        match_manual_grid(&catalog, 1, &registry(changed.clone().into(), 2), &grid),
        ManualCraftMatch::NoMatch
    );
    changed[1].negotiated_max_stack_size = Some(64);
    changed[1].component_based = true;
    changed[1].canonical_empty_component_data = false;
    assert_eq!(
        match_manual_grid(&catalog, 1, &registry(changed.into(), 3), &grid),
        ManualCraftMatch::NoMatch
    );
}

#[test]
fn bounded_catalog_scan_can_find_the_only_candidate_at_the_last_entry() {
    let mut recipes: Vec<_> = (1..=8192).map(|id| recipe(id, 1, 2)).collect();
    recipes.last_mut().unwrap().ingredients[0].count = 1;
    let catalog = catalog(recipes);
    assert!(catalog.recipe(8192).is_some());
    let registry = registry(entries(), 1);
    let input = stack(101, 1, 0);
    assert!(matches!(
        match_manual_grid(
            &catalog,
            1,
            &registry,
            &[Present(&input), Empty, Empty, Empty]
        ),
        ManualCraftMatch::Unique(_)
    ));
}

#[test]
fn unknown_cell_authority_is_not_an_empty_cell() {
    let catalog = catalog(vec![recipe(17, 1, 1)]);
    let registry = registry(entries(), 1);
    let input = stack(101, 1, 0);
    for unknown in 0..4 {
        let mut grid = [Present(&input), Empty, Empty, Empty];
        grid[unknown] = Unknown;
        assert_eq!(
            match_manual_grid(&catalog, 1, &registry, &grid),
            ManualCraftMatch::Unavailable
        );
    }
    assert!(matches!(
        match_manual_grid(
            &catalog,
            1,
            &registry,
            &[Present(&input), Empty, Empty, Empty]
        ),
        ManualCraftMatch::Unique(_)
    ));
}

/// Metadata 32767 accepts any variant in the manual preview too.
#[test]
fn wildcard_ingredient_metadata_matches_any_variant() {
    let mut wildcard = recipe(17, 1, 1);
    wildcard.ingredients[0].aux = 32767;
    let catalog = catalog(vec![wildcard]);
    let registry = registry(entries(), 1);
    let input = stack(101, 1, 3);
    assert!(matches!(
        match_manual_grid(
            &catalog,
            1,
            &registry,
            &[Present(&input), Empty, Empty, Empty]
        ),
        ManualCraftMatch::Unique(_)
    ));
}

use super::super::{
    RecipeCatalog, RecipeUnlockingContext, budget::Credits, decode::decode_with_credits,
};
use super::*;
use crate::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture, UnlockingFixture,
};
use bedrock_protocol_1_26_51::{packets::UnlockedRecipes, wire::Encode};

/// Produces ordinary named wire recipes with fixture-owned numeric item bindings.
fn recipe(name: &str, id: u32) -> RecipeFixture {
    RecipeFixture {
        net_id: id,
        ..RecipeFixture::shapeless(
            name,
            vec![IngredientFixture::named("test:input", 0)],
            vec![OutputFixture::new(7, 1)],
        )
    }
}

/// Applies real CraftingData under the same allocation owner as the tested state.
fn install(
    catalog: &mut RecipeCatalog,
    sequence: u64,
    recipes: Vec<RecipeFixture>,
    clear: bool,
    credits: &std::sync::Arc<Credits>,
) {
    let body = CraftingDataFixture {
        recipes,
        clear_recipes: clear,
        ..Default::default()
    }
    .encode();
    let update = decode_with_credits(&body, credits).unwrap();
    assert!(!update.is_unavailable());
    assert!(catalog.apply_with_credits(1, sequence, &update, credits));
}

/// Encodes names through the generated codec and decodes their bounded production body.
fn update(operation: u32, names: &[&str], credits: &std::sync::Arc<Credits>) -> RecipeUnlockUpdate {
    let body = UnlockedRecipes {
        packet_type: operation.into(),
        unlocked_recipes_list: names.iter().map(|name| (*name).into()).collect(),
    }
    .encode_to_vec()
    .unwrap();
    wire::decode_with_credits(&body, credits).unwrap()
}

#[test]
fn initial_is_additive_and_new_membership_is_separate() {
    let credits = Credits::isolated(1024 * 1024);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    install(
        &mut catalog,
        1,
        vec![recipe("test:A", 1), recipe("test:B", 2)],
        true,
        &credits,
    );
    let mut state = RecipeUnlockState::default();
    for name in ["test:A", "test:B"] {
        state.apply_with_credits(&catalog, &update(1, &[name], &credits), &credits);
    }
    state.apply_with_credits(&catalog, &update(1, &[], &credits), &credits);
    assert!(state.contains("test:A") && state.contains("test:B"));
    assert!(!state.is_newly_unlocked("test:A"));
    state.apply_with_credits(
        &catalog,
        &update(2, &["test:A", "test:A"], &credits),
        &credits,
    );
    assert!(state.is_newly_unlocked("test:A"));
    assert!(!state.is_newly_unlocked("test:B"));
    assert!(!state.contains("test:a"));
    state.apply_with_credits(
        &catalog,
        &update(1, &["test:a", " test:A"], &credits),
        &credits,
    );
    assert!(!state.contains("test:a") && !state.contains(" test:A"));
    state.apply_with_credits(&catalog, &update(0, &["test:B"], &credits), &credits);
    assert!(state.contains("test:B"));
    state.apply_with_credits(&catalog, &update(3, &["test:A"], &credits), &credits);
    assert!(!state.contains("test:A") && !state.is_newly_unlocked("test:A"));
    assert!(state.contains("test:B"));
    state.apply_with_credits(
        &catalog,
        &update(4, &["test:irrelevant"], &credits),
        &credits,
    );
    assert!(!state.contains("test:B"));
}

#[test]
fn unknown_before_catalog_never_revives_and_retained_names_survive_clear() {
    let credits = Credits::isolated(1024 * 1024);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    let mut state = RecipeUnlockState::default();
    state.apply_with_credits(&catalog, &update(2, &["test:A"], &credits), &credits);
    install(
        &mut catalog,
        1,
        vec![recipe("test:A", 1), recipe("test:B", 2)],
        true,
        &credits,
    );
    assert!(!state.contains("test:A"));
    state.apply_with_credits(&catalog, &update(71, &["test:A"], &credits), &credits);
    assert!(state.contains("test:A"));
    assert!(!state.is_newly_unlocked("test:A"));
    assert!(
        !state.contains("test:B"),
        "same output never grants sibling authority"
    );
    install(&mut catalog, 2, vec![], true, &credits);
    assert!(state.contains("test:A"));
    let retained = state.clone();
    state.apply_with_credits(&catalog, &update(3, &["test:A"], &credits), &credits);
    assert!(!state.contains("test:A"));
    state = retained;
    install(&mut catalog, 3, vec![recipe("test:A", 91)], true, &credits);
    let identity = catalog.resolve_unlock_identifier("test:A").unwrap();
    assert_eq!(identity.identity().network_id(), 91);
    assert!(state.is_unlocked(identity.identity()));
}

#[test]
fn contexts_and_unsupported_recipes_keep_exact_identity_without_grants() {
    let credits = Credits::isolated(1024 * 1024);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    let mut recipes = Vec::new();
    for (index, context) in [None, Some(0), Some(1), Some(2), Some(3), Some(127)]
        .into_iter()
        .enumerate()
    {
        let mut recipe = recipe(&format!("test:{index}"), index as u32 + 1);
        recipe.unlocking = context.map(|context| UnlockingFixture {
            context,
            ingredients: Some(recipe.ingredients.clone()),
        });
        recipes.push(recipe);
    }
    let mut unsupported = recipe("test:unsupported", 20);
    unsupported.ingredients[0].descriptor = vec![("custom".into(), "opaque".into())];
    unsupported.results[0].user_data = vec![1, 2];
    unsupported.tag = "furnace".into();
    recipes.push(unsupported);
    install(&mut catalog, 1, recipes, true, &credits);
    let state = RecipeUnlockState::default();
    for index in 0..6 {
        let identity = catalog
            .resolve_unlock_identifier(&format!("test:{index}"))
            .unwrap();
        assert_eq!(state.is_unlocked(identity.identity()), index == 2);
    }
    let identity = catalog
        .resolve_unlock_identifier("test:unsupported")
        .unwrap();
    assert_eq!(identity.identity().tag(), "furnace");
    assert_eq!(
        identity.identity().unlocking_context(),
        RecipeUnlockingContext::Ingredients
    );
    assert!(catalog.recipe(20).is_none());
}

#[test]
fn excluded_tags_duplicate_names_and_first_record_precedence_are_bounded() {
    let credits = Credits::isolated(1024 * 1024);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    let mut recipes = Vec::new();
    for (index, tag) in [
        "cartography_table",
        "smithing_table",
        "stonecutter",
        "deprecated",
        "campfire",
        "soul_campfire",
    ]
    .into_iter()
    .enumerate()
    {
        let mut recipe = recipe(tag, index as u32 + 1);
        recipe.tag = tag.into();
        recipes.push(recipe);
    }
    let mut duplicate = recipe("test:duplicate", 40);
    duplicate.tag = "furnace".into();
    recipes.extend([
        recipe("test:duplicate", 30),
        duplicate,
        recipe("test:first", 50),
    ]);
    install(&mut catalog, 1, recipes, true, &credits);
    assert!(
        catalog
            .resolve_unlock_identifier("test:duplicate")
            .is_none()
    );
    for identity in catalog
        .recipe_identities()
        .filter(|recipe| recipe.network_id() < 10)
    {
        assert!(
            catalog
                .resolve_unlock_identifier(identity.identifier())
                .is_none()
        );
    }
    install(
        &mut catalog,
        2,
        vec![recipe("test:first", 51)],
        false,
        &credits,
    );
    assert_eq!(
        catalog
            .resolve_unlock_identifier("test:first")
            .unwrap()
            .identity()
            .network_id(),
        50
    );
}

#[test]
fn snapshots_share_credit_and_retirement_never_releases_another_snapshot() {
    let credits = Credits::isolated(1024 * 1024);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    install(&mut catalog, 1, vec![recipe("test:A", 1)], true, &credits);
    let update = update(2, &["test:A"], &credits);
    let mut state = RecipeUnlockState::default();
    state.apply_with_credits(&catalog, &update, &credits);
    drop(update);
    assert!(state.contains("test:A"));
    let used = credits.used();
    let snapshot = state.clone();
    assert_eq!(credits.used(), used);
    state.clear();
    catalog.begin_session(2);
    assert!(snapshot.contains("test:A"));
    assert!(credits.used() > 0);
    drop(snapshot);
    drop(catalog);
    assert_eq!(credits.used(), 0);
}

#[test]
fn malformed_refused_and_unknown_operations_preserve_the_boundary() {
    let credits = Credits::isolated(1024 * 1024);
    let decoded = update(u32::MAX, &["test:A"], &credits);
    assert_eq!(
        decoded.operation(),
        RecipeUnlockOperation::Unknown(u32::MAX)
    );
    for bytes in [
        &[1, 0, 0][..],
        &[1, 0, 0, 0, 1][..],
        &[1, 0, 0, 0, 0, 0][..],
    ] {
        assert!(wire::decode_with_credits(bytes, &credits).is_err());
    }
    let body = UnlockedRecipes {
        packet_type: 1.into(),
        unlocked_recipes_list: vec!["test:A".into()],
    }
    .encode_to_vec()
    .unwrap();
    let occupied = credits.reserve(1024 * 1024 - credits.used()).unwrap();
    assert!(
        wire::decode_with_credits(&body, &credits)
            .unwrap()
            .is_unavailable()
    );
    drop(occupied);
    let mut oversized = vec![1, 0, 0, 0];
    oversized.extend([0x81, 0x40]);
    assert!(
        wire::decode_with_credits(&oversized, &credits)
            .unwrap()
            .is_unavailable()
    );
}

#[test]
fn refused_mutation_retires_current_authority_without_freeing_a_snapshot() {
    let credits = Credits::isolated(1024 * 1024);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    install(&mut catalog, 1, vec![recipe("test:A", 1)], true, &credits);
    let event = update(2, &["test:A"], &credits);
    let mut state = RecipeUnlockState::default();
    state.apply_with_credits(&catalog, &event, &credits);
    let snapshot = state.clone();
    assert!(snapshot.contains("test:A"));
    let occupied = credits.reserve(1024 * 1024 - credits.used()).unwrap();
    state.apply_with_credits(&catalog, &event, &credits);
    assert!(!state.contains("test:A"));
    assert!(snapshot.contains("test:A"));
    assert_eq!(credits.used(), 1024 * 1024);
    drop(occupied);
    let refused = RecipeUnlockUpdate {
        operation: RecipeUnlockOperation::Remove,
        names: None,
    };
    state = snapshot.clone();
    state.apply_with_credits(&catalog, &refused, &credits);
    assert!(!state.contains("test:A"));
    assert!(snapshot.contains("test:A"));
}

#[test]
fn invalid_utf8_never_aliases_the_empty_identifier() {
    let credits = Credits::isolated(1024 * 1024);
    let event = wire::decode_with_credits(&[1, 0, 0, 0, 1, 1, 0xff], &credits).unwrap();
    assert!(!event.is_unavailable());
    assert!(event.identifiers().next().is_none());
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    install(&mut catalog, 1, vec![recipe("", 1)], true, &credits);
    let mut state = RecipeUnlockState::default();
    state.apply_with_credits(&catalog, &event, &credits);
    assert!(!state.contains(""));
}

#[test]
fn membership_cardinality_is_bounded_across_catalog_replacements() {
    let credits = Credits::isolated(super::super::budget::RECIPE_OWNED_BYTES);
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    install(&mut catalog, 1, vec![recipe("test:old", 1)], true, &credits);
    let mut state = RecipeUnlockState::default();
    state.apply_with_credits(&catalog, &update(1, &["test:old"], &credits), &credits);
    let snapshot = state.clone();
    let names: Vec<_> = (0..super::super::budget::MAX_RECORDS)
        .map(|index| format!("test:next_{index}"))
        .collect();
    let recipes = names
        .iter()
        .enumerate()
        .map(|(index, name)| recipe(name, index as u32 + 1))
        .collect();
    install(&mut catalog, 2, recipes, true, &credits);
    let names: Vec<_> = names.iter().map(String::as_str).collect();
    state.apply_with_credits(&catalog, &update(1, &names, &credits), &credits);
    assert!(!state.contains("test:old"));
    assert!(!state.contains(names[0]));
    assert!(snapshot.contains("test:old"));
}

#[test]
fn one_retained_unlock_does_not_block_admission_after_a_large_catalog_clear() {
    for retain_unlock in [false, true] {
        let credits = Credits::isolated(2 * 1024 * 1024);
        let mut catalog = RecipeCatalog::default();
        catalog.begin_session(1);
        let mut recipes = vec![recipe("test:retained", 1)];
        let prefix = "x".repeat(15 * 1024);
        recipes.extend((0..72).map(|index| recipe(&format!("test:{prefix}{index}"), index + 2)));
        let body = CraftingDataFixture {
            recipes,
            clear_recipes: true,
            ..Default::default()
        }
        .encode();
        let first = decode_with_credits(&body, &credits).unwrap();
        assert!(!first.is_unavailable());
        assert!(catalog.apply_with_credits(1, 1, &first, &credits));
        drop(first);
        let mut state = RecipeUnlockState::default();
        if retain_unlock {
            state.apply_with_credits(&catalog, &update(2, &["test:retained"], &credits), &credits);
            assert!(state.contains("test:retained"));
        }
        install(&mut catalog, 2, vec![], true, &credits);
        let replacement = decode_with_credits(&body, &credits).unwrap();
        assert!(
            !replacement.is_unavailable(),
            "one retained name must not pin the cleared recipe batch"
        );
        assert!(catalog.apply_with_credits(1, 3, &replacement, &credits));
        assert!(catalog.is_available());
        assert_eq!(state.contains("test:retained"), retain_unlock);
        drop(replacement);
        drop(state);
        drop(catalog);
        assert_eq!(credits.used(), 0);
    }
}

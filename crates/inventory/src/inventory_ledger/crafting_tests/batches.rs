use super::*;
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};

/// Admits an original one-log recipe with four planks per craft.
fn four_planks(ledger: &PlayerInventoryLedger) -> protocol::RecipeHandle {
    let mut recipe = RecipeFixture::shapeless(
        "fixture:four_planks",
        vec![IngredientFixture::named("minecraft:oak_log", 0)],
        vec![OutputFixture::new(PLANKS, 4)],
    );
    recipe.net_id = 55;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &CraftingDataFixture::clearing(vec![recipe]).update()));
    unique(ledger, &catalog)
}

#[test]
fn craft_scopes_fill_all_256_outputs_with_bounded_results() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(LOG, 101, 64)));
    let recipe = four_planks(&ledger);
    let first = ledger.begin_craft_all(&recipe).unwrap();
    let total: u16 = (0..36)
        .filter_map(|slot| ledger.displayed_stack(slot))
        .map(|stack| stack.count)
        .sum();
    assert_eq!(total, 256);
    assert!(ledger.target_stack(InventoryTarget::Craft(28)).is_none());
    assert_eq!(ledger.pending_request_count(), 4);
    assert_eq!(ledger.queue.front().unwrap().request_id, first);
    for request in &ledger.queue {
        assert!(matches!(
            request.actions[0],
            StackRequestAction::CraftRecipe { crafts: 16, .. }
        ));
        assert!(
            matches!(&request.actions[1], StackRequestAction::CraftResultsDeprecated { crafts: 1, results }
            if results.len() == 1 && results[0].count == 4)
        );
        assert!(matches!(
            request.actions[2],
            StackRequestAction::Consume { amount: 16, .. }
        ));
        assert!(request.actions.iter().skip(3).all(|action| matches!(action,
            StackRequestAction::Place { source, amount, .. }
                if source.stack_network_id == request.request_id && *amount <= 64)));
    }
    assert!(ledger.created_output_stack().is_none());
}

#[test]
fn craft_scopes_declare_per_craft_results_once() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(LOG, 101, 5)));
    let recipe = four_planks(&ledger);
    ledger.begin_craft(&recipe, 5).unwrap();
    let request = ledger.newest_request().unwrap();
    assert!(matches!(
        request.actions[0],
        StackRequestAction::CraftRecipe { crafts: 5, .. }
    ));
    assert!(
        matches!(&request.actions[1], StackRequestAction::CraftResultsDeprecated { crafts: 1, results }
        if results[0].count == 4)
    );
    assert_eq!(ledger.cursor_stack().unwrap().count, 20);
}

#[test]
fn craft_scopes_explicit_count_remains_atomic_when_the_sink_cannot_fit() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(LOG, 101, 64)));
    let recipe = four_planks(&ledger);
    assert_eq!(
        ledger.begin_craft(&recipe, 64),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(28))
            .unwrap()
            .count,
        64
    );
    assert!(ledger.cursor_stack().is_none());
}

/// Resolves an original shapeless recipe whose identical ingredients need different counts.
fn unequal_recipe(ledger: &PlayerInventoryLedger) -> protocol::RecipeHandle {
    let one = IngredientFixture::named("minecraft:oak_log", 0);
    let two = IngredientFixture {
        count: 2,
        ..one.clone()
    };
    let mut recipe = RecipeFixture::shapeless(
        "fixture:unequal_counts",
        vec![one, two],
        vec![OutputFixture::new(PLANKS, 4)],
    );
    recipe.net_id = 56;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &CraftingDataFixture::clearing(vec![recipe]).update()));
    unique(ledger, &catalog)
}

#[test]
fn craft_scopes_keep_an_assignment_that_can_finish_the_requested_total() {
    for explicit in [false, true] {
        let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
        ledger.apply(&craft_slot(28, stack(LOG, 101, 64)));
        ledger.apply(&craft_slot(29, stack(LOG, 102, 32)));
        let recipe = unequal_recipe(&ledger);
        if explicit {
            ledger
                .begin_craft_into(&recipe, 32, CraftSink::Inventory)
                .unwrap();
        } else {
            ledger.begin_craft_all(&recipe).unwrap();
        }
        let total: u16 = (0..36)
            .filter_map(|slot| ledger.displayed_stack(slot))
            .map(|stack| stack.count)
            .sum();
        assert_eq!(total, 128);
        assert_eq!(ledger.pending_request_count(), 2);
        for slot in [28, 29] {
            assert!(ledger.target_stack(InventoryTarget::Craft(slot)).is_none());
        }
    }
}

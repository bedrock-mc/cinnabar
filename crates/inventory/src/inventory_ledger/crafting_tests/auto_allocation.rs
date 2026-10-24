//! Recipe-book planning reassigns overlapping descriptors while preserving ordinary first choices.

use super::*;
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};

/// Admits one original shapeless recipe through the real wire decoder and catalog.
fn recipe(ingredients: Vec<IngredientFixture>) -> protocol::RecipeHandle {
    let mut recipe = RecipeFixture::shapeless(
        "test:overlap",
        ingredients,
        vec![OutputFixture::new(FURNACE, 1)],
    );
    recipe.net_id = 77;
    let update = CraftingDataFixture::clearing(vec![recipe]).update();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    catalog.recipe(77).unwrap()
}

/// Supplies distinct authoritative stacks and two overlapping negotiated item tags.
fn stocked(stacks: &[(u8, i32, i32, u16)]) -> PlayerInventoryLedger {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    let mut entries = registry().entries.to_vec();
    for entry in &mut entries {
        entry.item_tags = match entry.network_id {
            LOG => Arc::from([Arc::from("test:wood")]),
            PLANKS => Arc::from([Arc::from("test:wood"), Arc::from("test:other")]),
            COBBLE => Arc::from([Arc::from("test:other")]),
            _ => Arc::from([]),
        };
    }
    ledger.apply_registry(&ItemRegistryEvent {
        entries: entries.into(),
    });
    for &(slot, item, identity, count) in stacks {
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: u16::from(slot),
            },
            stack: stack(item, identity, count),
            storage_item: None,
        }));
    }
    ledger
}

/// Reads actual request consumption in recipe/slot order, including its settled identities.
fn consumes(ledger: &PlayerInventoryLedger) -> Vec<(u8, u8, i32)> {
    ledger
        .newest_request()
        .unwrap()
        .actions
        .iter()
        .filter_map(|action| {
            if let StackRequestAction::Consume { amount, source } = action {
                Some((source.slot, *amount, source.stack_network_id))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn auto_craft_reassigns_the_only_exact_item_from_an_earlier_tag() {
    let recipe = recipe(vec![
        IngredientFixture::tagged("test:wood"),
        IngredientFixture::named("minecraft:oak_log", 0),
    ]);
    let mut ledger = stocked(&[(9, LOG, 101, 1), (10, PLANKS, 102, 1)]);
    assert!(ledger.can_auto_craft(&recipe));
    ledger.begin_auto_craft(&recipe, false).unwrap();
    assert_eq!(consumes(&ledger), [(10, 1, 102), (9, 1, 101)]);
    assert!(ledger.target_stack(InventoryTarget::Player(9)).is_none());
    assert!(ledger.target_stack(InventoryTarget::Player(10)).is_none());
    assert_eq!(placed_outputs(&ledger)[0].1.network_id, FURNACE);
    assert!(ledger.cursor_stack().is_none());
}

#[test]
fn auto_craft_splits_one_variant_across_slots_and_reserves_the_exact_variant() {
    let recipe = recipe(vec![
        IngredientFixture {
            count: 3,
            ..IngredientFixture::tagged("test:wood")
        },
        IngredientFixture {
            count: 2,
            ..IngredientFixture::named("minecraft:oak_log", 0)
        },
    ]);
    let mut ledger = stocked(&[(9, LOG, 101, 3), (10, PLANKS, 102, 1), (11, PLANKS, 103, 2)]);
    assert!(ledger.can_auto_craft(&recipe));
    ledger.begin_auto_craft(&recipe, false).unwrap();
    assert_eq!(consumes(&ledger), [(10, 1, 102), (11, 2, 103), (9, 2, 101)]);
    for slot in [10, 11] {
        assert!(ledger.target_stack(InventoryTarget::Player(slot)).is_none());
    }
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 1);
    assert_eq!(placed_outputs(&ledger)[0].1.count, 1);
}

/// A single ingredient cannot pool two concrete variants to meet its count.
#[test]
fn auto_craft_refuses_a_demand_that_only_mixed_variants_can_supply() {
    let recipe = recipe(vec![
        IngredientFixture {
            count: 3,
            ..IngredientFixture::tagged("test:wood")
        },
        IngredientFixture {
            count: 2,
            ..IngredientFixture::named("minecraft:oak_log", 0)
        },
    ]);
    let mut ledger = stocked(&[(9, LOG, 101, 3), (10, PLANKS, 102, 1), (11, PLANKS, 103, 1)]);
    assert!(!ledger.can_auto_craft(&recipe));
    assert_eq!(
        ledger.begin_auto_craft(&recipe, false),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.pending_request_count(), 0);
}

/// Shift-crafting stops at what one declared plank variant can supply in one request.
#[test]
fn shift_craft_names_and_consumes_only_one_plank_variant_per_ingredient() {
    let recipe = recipe(vec![IngredientFixture {
        count: 2,
        ..IngredientFixture::tagged("test:wood")
    }]);
    let mut ledger = plank_ledger(&[(9, PLANKS, 101, 3), (10, BIRCH_PLANKS, 102, 2)]);
    ledger.begin_auto_craft(&recipe, true).unwrap();
    let Some(StackRequestAction::AutoCraft {
        crafts,
        ingredients,
        ..
    }) = ledger.newest_action()
    else {
        panic!("expected auto-craft");
    };
    assert_eq!(crafts, 1);
    assert_eq!(ingredients.len(), 1);
    assert_eq!(
        ingredients[0].descriptor,
        protocol::StackItemDescriptor::Name {
            identifier: Arc::from("minecraft:oak_planks"),
            aux: i32::from(protocol::RECIPE_ANY_AUX),
        }
    );
    assert_eq!(consumes(&ledger), [(9, 2, 101)]);
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 1);
    assert_eq!(ledger.displayed_stack(10).unwrap().count, 2);
}

#[test]
fn auto_craft_follows_a_chain_of_prior_tag_assignments() {
    let recipe = recipe(vec![
        IngredientFixture::tagged("test:wood"),
        IngredientFixture::tagged("test:other"),
        IngredientFixture::named("minecraft:oak_log", 0),
    ]);
    let mut ledger = stocked(&[(9, LOG, 101, 1), (10, PLANKS, 102, 1), (11, COBBLE, 103, 1)]);
    assert!(ledger.can_auto_craft(&recipe));
    ledger.begin_auto_craft(&recipe, false).unwrap();
    assert_eq!(consumes(&ledger), [(10, 1, 102), (11, 1, 103), (9, 1, 101)]);
}

#[test]
fn successful_first_choice_allocations_keep_their_existing_order() {
    let recipe = recipe(vec![
        IngredientFixture::tagged("test:wood"),
        IngredientFixture::named("minecraft:oak_log", 0),
    ]);
    let mut ledger = stocked(&[(9, LOG, 101, 2), (10, PLANKS, 102, 1)]);
    ledger.begin_auto_craft(&recipe, false).unwrap();
    assert_eq!(consumes(&ledger), [(9, 1, 101), (9, 1, 101)]);
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Player(10))
            .unwrap()
            .count,
        1
    );
}

#[test]
fn an_incomplete_allocation_leaves_inventory_and_request_queue_unchanged() {
    let recipe = recipe(vec![
        IngredientFixture {
            count: 2,
            ..IngredientFixture::tagged("test:wood")
        },
        IngredientFixture::named("minecraft:oak_log", 0),
    ]);
    let mut ledger = stocked(&[(9, LOG, 101, 1), (10, PLANKS, 102, 1)]);
    let pending = ledger.pending_request_count();
    assert!(!ledger.can_auto_craft(&recipe));
    assert_eq!(
        ledger.begin_auto_craft(&recipe, false),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.pending_request_count(), pending);
    for slot in [9, 10] {
        assert_eq!(
            ledger
                .target_stack(InventoryTarget::Player(slot))
                .unwrap()
                .count,
            1
        );
    }
    assert!(ledger.cursor_stack().is_none());
}

/// Servers refuse a recipe-book craft whose tag ingredient is not named by the consumed item.
#[test]
fn auto_craft_names_a_tag_ingredient_by_the_item_it_consumes() {
    let recipe = recipe(vec![
        IngredientFixture::tagged("test:wood"),
        IngredientFixture::named("minecraft:oak_log", 0),
    ]);
    let mut ledger = stocked(&[(9, LOG, 101, 1), (10, PLANKS, 102, 1)]);
    ledger.begin_auto_craft(&recipe, false).unwrap();
    let Some(StackRequestAction::AutoCraft { ingredients, .. }) = ledger.newest_action() else {
        panic!("expected an auto-craft request");
    };
    let named = |identifier: &str, aux| protocol::StackItemDescriptor::Name {
        identifier: Arc::from(identifier),
        aux,
    };
    let descriptors: Vec<_> = ingredients
        .iter()
        .map(|ingredient| ingredient.descriptor.clone())
        .collect();
    assert_eq!(
        descriptors,
        [
            named("minecraft:oak_planks", i32::from(protocol::RECIPE_ANY_AUX)),
            named("minecraft:oak_log", 0),
        ]
    );
}

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
fn auto_craft_splits_quantities_across_slots_and_reuses_one_stack_for_two_demands() {
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
    assert!(ledger.can_auto_craft(&recipe));
    ledger.begin_auto_craft(&recipe, false).unwrap();
    assert_eq!(
        consumes(&ledger),
        [(9, 1, 101), (10, 1, 102), (11, 1, 103), (9, 2, 101)]
    );
    for slot in [9, 10, 11] {
        assert!(ledger.target_stack(InventoryTarget::Player(slot)).is_none());
    }
    assert_eq!(placed_outputs(&ledger)[0].1.count, 1);
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

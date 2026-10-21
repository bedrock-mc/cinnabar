//! Recipe-book crafts place their output into the inventory and repeat up to one output stack.

use super::*;

/// The log-to-planks recipe from the shared fixture catalog.
fn planks() -> protocol::RecipeHandle {
    catalog()
        .crafting_handles()
        .into_iter()
        .find(|recipe| recipe.network_id() == 1)
        .unwrap()
}

/// A personal inventory holding `stacks` as `(slot, item, stack id, count)`.
fn stocked(stacks: &[(u8, i32, i32, u16)]) -> PlayerInventoryLedger {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    for &(slot, item, id, count) in stacks {
        slots[usize::from(slot)] = stack(item, id, count);
    }
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::default(),
    }));
    ledger
}

/// The `(crafts, times crafted)` the newest request declares.
fn craft_counts(ledger: &PlayerInventoryLedger) -> (u8, u8) {
    let actions = &ledger.newest_request().unwrap().actions;
    let auto = actions.iter().find_map(|action| match action {
        StackRequestAction::AutoCraft { crafts, .. } => Some(*crafts),
        _ => None,
    });
    let results = actions.iter().find_map(|action| match action {
        StackRequestAction::CraftResultsDeprecated { results, crafts } => {
            assert_eq!(results[0].count, 1);
            Some(*crafts)
        }
        _ => None,
    });
    (auto.unwrap(), results.unwrap())
}

#[test]
fn a_book_craft_places_its_output_in_the_hotbar_and_keeps_the_held_stack() {
    let mut ledger = stocked(&[(0, COBBLE, 100, 5), (12, LOG, 101, 3)]);
    ledger.apply(&cursor_content(stack(COBBLE, 102, 2)));
    ledger.begin_auto_craft(&planks(), false).unwrap();
    assert_eq!(craft_counts(&ledger), (1, 1));
    let placed = placed_outputs(&ledger);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].0, 1);
    assert_eq!((placed[0].1.network_id, placed[0].1.count), (PLANKS, 1));
    assert_eq!(ledger.cursor_stack().unwrap().count, 2);
    assert_eq!(ledger.displayed_stack(12).unwrap().count, 2);
}

#[test]
fn a_book_craft_merges_into_a_partial_stack_before_an_empty_slot() {
    let mut ledger = stocked(&[(4, PLANKS, 100, 10), (12, LOG, 101, 3)]);
    ledger.begin_auto_craft(&planks(), false).unwrap();
    let placed = placed_outputs(&ledger);
    assert_eq!(placed.len(), 1);
    assert_eq!((placed[0].0, placed[0].1.count), (4, 11));
    assert!(matches!(
        ledger.newest_request().unwrap().actions.last(),
        Some(StackRequestAction::Place { destination, .. }) if destination.stack_network_id == 100
    ));
}

#[test]
fn crafting_all_from_the_book_repeats_until_ingredients_or_one_stack_run_out() {
    for (logs, crafts) in [(5u16, 5u8), (70, 64)] {
        let mut stacks = vec![(12, LOG, 101, logs.min(64))];
        if logs > 64 {
            stacks.push((13, LOG, 102, logs - 64));
        }
        let mut ledger = stocked(&stacks);
        ledger.begin_auto_craft(&planks(), true).unwrap();
        assert_eq!(craft_counts(&ledger), (crafts, crafts));
        let placed: u16 = placed_outputs(&ledger)
            .iter()
            .map(|(_, stack)| stack.count)
            .sum();
        assert_eq!(placed, u16::from(crafts));
        let left: u16 = [12, 13]
            .into_iter()
            .filter_map(|slot| ledger.displayed_stack(slot))
            .filter(|held| held.network_id == LOG)
            .map(|held| held.count)
            .sum();
        assert_eq!(left, logs - u16::from(crafts));
    }
}

#[test]
fn a_book_craft_needs_room_before_consuming_anything() {
    let mut full: Vec<_> = (0..35)
        .map(|slot| (slot, COBBLE, 100 + i32::from(slot), 64))
        .collect();
    full.push((35, LOG, 200, 1));
    // The log's slot would only free up once consumed, so there is no room to start.
    let mut ledger = stocked(&full);
    let pending = ledger.pending_request_count();
    assert_eq!(
        ledger.begin_auto_craft(&planks(), false),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.pending_request_count(), pending);

    // A partial stack with room for one craft is enough; consumed slots then take the rest.
    full[0] = (0, PLANKS, 100, 63);
    full[35].3 = 3;
    let mut ledger = stocked(&full);
    ledger.begin_auto_craft(&planks(), true).unwrap();
    let placed: Vec<_> = placed_outputs(&ledger)
        .into_iter()
        .map(|(slot, stack)| (slot, stack.count))
        .collect();
    assert_eq!(placed, [(0, 64), (35, 2)]);
    assert!(ledger.cursor_stack().is_none());
}

#[test]
fn a_rejected_book_craft_restores_ingredients_and_removes_the_placed_output() {
    let mut ledger = stocked(&[(12, LOG, 101, 5)]);
    let request = ledger.begin_auto_craft(&planks(), true).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Rejected,
            request_id: request,
            containers: Arc::from([]),
        }]),
    }));
    assert_eq!(ledger.displayed_stack(12).unwrap().count, 5);
    for slot in 0..protocol::PLAYER_INVENTORY_SLOTS {
        if slot != 12 {
            assert!(ledger.displayed_stack(slot).is_none(), "slot {slot}");
        }
    }
    assert!(ledger.cursor_stack().is_none());
    assert!(!ledger.resync_required());
}

#[test]
fn book_crafts_consume_and_place_under_the_combined_inventory_name() {
    // Vanilla servers refuse crafting ingredients consumed from the hotbar or inventory names.
    let mut ledger = stocked(&[(0, LOG, 101, 2), (20, LOG, 102, 2)]);
    ledger.begin_auto_craft(&planks(), true).unwrap();
    let actions = &ledger.newest_request().unwrap().actions;
    let mut named = 0;
    for action in actions.iter() {
        match action {
            StackRequestAction::Consume { source, .. } => {
                assert_eq!(source.container, StackRequestContainer::CombinedInventory);
                named += 1;
            }
            StackRequestAction::Place { destination, .. } => {
                assert_eq!(
                    destination.container,
                    StackRequestContainer::CombinedInventory
                );
                named += 1;
            }
            _ => {}
        }
    }
    assert_eq!(named, 3);
    assert!(protocol::item_stack_request_packet(-1, actions).is_ok());
}

/// A one-log recipe making four planks.
fn four_planks() -> protocol::RecipeHandle {
    use protocol::test_support::recipes::{
        CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
    };
    let mut fixture = RecipeFixture::shaped(
        "test:four_planks",
        1,
        1,
        vec![IngredientFixture::named("minecraft:oak_log", 0)],
        vec![OutputFixture::new(PLANKS, 4)],
    );
    fixture.net_id = 78;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &CraftingDataFixture::clearing(vec![fixture]).update()));
    catalog.recipe(78).unwrap()
}

#[test]
fn room_for_a_multi_item_output_is_one_cell_that_holds_all_of_it() {
    // A nearly full matching stack beside an empty slot still has room for the whole output.
    let mut ledger = stocked(&[(0, PLANKS, 100, 63), (12, LOG, 101, 1)]);
    ledger.begin_auto_craft(&four_planks(), false).unwrap();
    let placed: Vec<_> = placed_outputs(&ledger)
        .into_iter()
        .map(|(slot, stack)| (slot, stack.count))
        .collect();
    assert_eq!(placed, [(0, 64), (1, 3)]);

    // Only a partial stack with room for part of it is not enough.
    let mut full: Vec<_> = (1..36)
        .map(|slot| (slot, COBBLE, 100 + i32::from(slot), 64))
        .collect();
    full[34] = (35, LOG, 200, 1);
    full.push((0, PLANKS, 300, 61));
    let mut ledger = stocked(&full);
    assert_eq!(
        ledger.begin_auto_craft(&four_planks(), false),
        Err(InventoryGestureError::InvalidRequest)
    );
}

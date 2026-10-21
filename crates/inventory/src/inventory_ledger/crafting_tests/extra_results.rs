//! Several-result recipes create every result by index and return the extras to the grid
//! (manual crafts) or the inventory (recipe-book crafts).

use super::*;
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};

/// Two cobblestone make a furnace and give back two planks, like a cake's buckets.
fn recipe() -> protocol::RecipeHandle {
    let mut fixture = RecipeFixture::shapeless(
        "test:with_extra",
        vec![
            IngredientFixture::named("minecraft:cobblestone", 0),
            IngredientFixture::named("minecraft:cobblestone", 0),
        ],
        vec![
            OutputFixture::new(FURNACE, 1),
            OutputFixture::new(PLANKS, 2),
        ],
    );
    fixture.net_id = 77;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &CraftingDataFixture::clearing(vec![fixture]).update()));
    catalog.recipe(77).unwrap()
}

/// The newest request's actions after its consumes, as compact labels.
fn tail(ledger: &PlayerInventoryLedger) -> Vec<String> {
    ledger
        .newest_request()
        .unwrap()
        .actions
        .iter()
        .filter_map(|action| match action {
            StackRequestAction::Create { results_index } => Some(format!("create {results_index}")),
            StackRequestAction::Take {
                amount,
                destination,
                ..
            } => Some(format!("take {amount} {:?}", destination.container)),
            StackRequestAction::Place {
                amount,
                destination,
                ..
            } => Some(format!(
                "place {amount} {:?} {} {}",
                destination.container, destination.slot, destination.stack_network_id
            )),
            StackRequestAction::Drop { amount, .. } => Some(format!("drop {amount}")),
            _ => None,
        })
        .collect()
}

fn declared_results(ledger: &PlayerInventoryLedger) -> Vec<(Arc<str>, u16)> {
    ledger
        .newest_request()
        .unwrap()
        .actions
        .iter()
        .find_map(|action| match action {
            StackRequestAction::CraftResultsDeprecated { results, .. } => Some(
                results
                    .iter()
                    .map(|result| (Arc::clone(&result.identifier), result.count))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_manual_craft_takes_the_main_result_and_returns_extras_to_the_emptied_grid() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(COBBLE, 101, 1)));
    ledger.apply(&craft_slot(29, stack(COBBLE, 102, 1)));
    let request = ledger.begin_craft(&recipe(), 1).unwrap();
    assert_eq!(
        declared_results(&ledger),
        [
            (Arc::from("minecraft:furnace"), 1),
            (Arc::from("minecraft:oak_planks"), 2)
        ]
    );
    let grid = format!("{:?}", StackRequestContainer::CraftingInput);
    assert_eq!(
        tail(&ledger),
        [
            "create 0".to_owned(),
            "take 1 Cursor".to_owned(),
            "create 1".to_owned(),
            format!("place 2 {grid} 28 0"),
        ]
    );
    assert_eq!(ledger.cursor_stack().unwrap().network_id, FURNACE);
    let returned = ledger.target_stack(InventoryTarget::Craft(28)).unwrap();
    assert_eq!((returned.network_id, returned.count), (PLANKS, 2));
    assert!(ledger.target_stack(InventoryTarget::Craft(29)).is_none());

    assert!(ledger.mark_transport_enqueued(10));
    respond(
        &mut ledger,
        request,
        &[
            (CONTAINER_NAME_CRAFT_INPUT, 28, 2, 90),
            (CONTAINER_NAME_CRAFT_INPUT, 29, 0, 0),
            (CONTAINER_NAME_CURSOR, 0, 1, 91),
        ],
    );
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(28))
            .unwrap()
            .stack_network_id,
        90
    );
    assert!(!ledger.resync_required());
}

#[test]
fn extras_merge_into_a_matching_grid_stack_before_an_emptied_cell() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(COBBLE, 101, 1)));
    ledger.apply(&craft_slot(29, stack(COBBLE, 102, 1)));
    ledger.apply(&craft_slot(30, stack(PLANKS, 103, 5)));
    ledger.begin_craft(&recipe(), 1).unwrap();
    let grid = format!("{:?}", StackRequestContainer::CraftingInput);
    assert_eq!(
        tail(&ledger).last().unwrap(),
        &format!("place 2 {grid} 30 103")
    );
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(30))
            .unwrap()
            .count,
        7
    );
    assert!(ledger.target_stack(InventoryTarget::Craft(28)).is_none());
}

#[test]
fn a_book_craft_places_extras_then_the_main_result_in_the_inventory() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 12,
        },
        stack: stack(COBBLE, 101, 2),
        storage_item: None,
    }));
    ledger.begin_auto_craft(&recipe(), false).unwrap();
    let inventory = format!("{:?}", StackRequestContainer::CombinedInventory);
    assert_eq!(
        tail(&ledger),
        [
            "create 1".to_owned(),
            format!("place 2 {inventory} 0 0"),
            "create 0".to_owned(),
            format!("place 1 {inventory} 1 0"),
        ]
    );
    assert_eq!(ledger.displayed_stack(0).unwrap().network_id, PLANKS);
    assert_eq!(ledger.displayed_stack(1).unwrap().network_id, FURNACE);
    assert!(ledger.displayed_stack(12).is_none());
}

#[test]
fn a_rejected_craft_with_extras_restores_the_grid_and_cursor() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(COBBLE, 101, 1)));
    ledger.apply(&craft_slot(29, stack(COBBLE, 102, 1)));
    let request = ledger.begin_craft(&recipe(), 1).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Rejected,
            request_id: request,
            containers: Arc::from([]),
        }]),
    }));
    for (slot, id) in [(28, 101), (29, 102)] {
        let held = ledger.target_stack(InventoryTarget::Craft(slot)).unwrap();
        assert_eq!(
            (held.network_id, held.count, held.stack_network_id),
            (COBBLE, 1, id)
        );
    }
    assert!(ledger.cursor_stack().is_none());
    assert!(!ledger.resync_required());
}

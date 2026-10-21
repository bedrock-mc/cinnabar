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
fn a_manual_craft_takes_the_main_result_and_returns_extras_to_a_free_grid_cell() {
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
            format!("place 2 {grid} 30 0"),
        ]
    );
    assert_eq!(ledger.cursor_stack().unwrap().network_id, FURNACE);
    // Cells the craft empties are never reused within the same request.
    let returned = ledger.target_stack(InventoryTarget::Craft(30)).unwrap();
    assert_eq!((returned.network_id, returned.count), (PLANKS, 2));
    assert!(ledger.target_stack(InventoryTarget::Craft(28)).is_none());
    assert!(ledger.target_stack(InventoryTarget::Craft(29)).is_none());

    assert!(ledger.mark_transport_enqueued(10));
    respond(
        &mut ledger,
        request,
        &[
            (CONTAINER_NAME_CRAFT_INPUT, 28, 0, 0),
            (CONTAINER_NAME_CRAFT_INPUT, 29, 0, 0),
            (CONTAINER_NAME_CRAFT_INPUT, 30, 2, 90),
            (CONTAINER_NAME_CURSOR, 0, 1, 91),
        ],
    );
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(30))
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

#[test]
fn extras_with_nowhere_to_go_drop_in_encodable_amounts() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    let full: Vec<_> = (0..protocol::PLAYER_INVENTORY_SLOTS)
        .map(|slot| stack(LOG, 100 + i32::from(slot), 64))
        .collect();
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: full.into(),
        storage_item: NetworkItemStack::default(),
    }));
    let (mut actions, mut groups) = (Vec::new(), Vec::new());
    ledger
        .push_extra_outputs(
            &recipe(),
            64,
            -3,
            crafting::ExtraDestination::Inventory,
            &[],
            &mut actions,
            &mut groups,
        )
        .unwrap();
    let drops: Vec<_> = actions
        .iter()
        .filter_map(|action| match action {
            StackRequestAction::Drop { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(drops, [64, 64]);
    assert!(protocol::item_stack_request_packet(-3, &actions).is_ok());
}

#[test]
fn a_craft_too_large_to_encode_is_refused_before_it_is_queued() {
    let mut fixture = RecipeFixture::shapeless(
        "test:many_extras",
        vec![IngredientFixture::named("minecraft:cobblestone", 0)],
        std::iter::once(OutputFixture::new(FURNACE, 1))
            .chain((0..60).map(|_| OutputFixture::new(PLANKS, 1)))
            .collect(),
    );
    fixture.net_id = 79;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &CraftingDataFixture::clearing(vec![fixture]).update()));
    let recipe = catalog.recipe(79).unwrap();
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(COBBLE, 101, 1)));
    let pending = ledger.pending_request_count();
    assert_eq!(
        ledger.begin_craft(&recipe, 1),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.pending_request_count(), pending);
}

#[test]
fn every_created_result_keeps_its_own_lineage() {
    let mut fixture = RecipeFixture::shapeless(
        "test:two_extras",
        vec![IngredientFixture::named("minecraft:cobblestone", 0)],
        vec![
            OutputFixture::new(FURNACE, 1),
            OutputFixture::new(PLANKS, 1),
            OutputFixture::new(PLANKS, 1),
        ],
    );
    fixture.net_id = 80;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &CraftingDataFixture::clearing(vec![fixture]).update()));
    let recipe = catalog.recipe(80).unwrap();
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(COBBLE, 101, 1)));
    ledger.begin_craft(&recipe, 1).unwrap();
    let held = |cell| ledger.view().get(cell).unwrap().clone();
    let (main, first, second) = (
        held(Cell::Cursor),
        held(Cell::Craft(29)),
        held(Cell::Craft(30)),
    );
    assert_eq!(
        (first.stack.network_id, second.stack.network_id),
        (PLANKS, PLANKS)
    );
    assert!(!first.same_source(&second));
    assert!(!main.same_source(&first) && !main.same_source(&second));
}

#[test]
fn a_book_craft_all_keeps_every_result_within_one_stack() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    for (slot, id) in [(12, 101), (13, 102)] {
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot,
            },
            stack: stack(COBBLE, id, 64),
            storage_item: None,
        }));
    }
    ledger.begin_auto_craft(&recipe(), true).unwrap();
    // The main result alone would allow 64 crafts; the two planks each returns allow 32.
    let crafts = ledger
        .newest_request()
        .unwrap()
        .actions
        .iter()
        .find_map(|action| match action {
            StackRequestAction::AutoCraft { crafts, .. } => Some(*crafts),
            _ => None,
        });
    assert_eq!(crafts, Some(32));
    let created: Vec<_> = tail(&ledger)
        .into_iter()
        .filter(|label| label.starts_with("place") || label.starts_with("drop"))
        .collect();
    assert!(
        created.iter().all(|label| !label.starts_with("drop")),
        "{created:?}"
    );
}

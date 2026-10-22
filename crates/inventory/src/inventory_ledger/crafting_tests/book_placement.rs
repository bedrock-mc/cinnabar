//! A primary recipe-book click lays the recipe into the crafting grid instead of crafting.

use super::*;
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};

/// A fixture-catalog recipe by network id: 1 is log to planks, 2 the cobblestone ring.
fn fixture(id: u32) -> protocol::RecipeHandle {
    catalog()
        .crafting_handles()
        .into_iter()
        .find(|recipe| recipe.network_id() == id)
        .unwrap()
}

/// A shaped recipe admitted through the real wire decoder.
fn shaped(width: i32, height: i32, ingredients: Vec<IngredientFixture>) -> protocol::RecipeHandle {
    let mut recipe = RecipeFixture::shaped(
        "test:shaped",
        width,
        height,
        ingredients,
        vec![OutputFixture::new(FURNACE, 1)],
    );
    recipe.net_id = 88;
    let update = CraftingDataFixture::clearing(vec![recipe]).update();
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &update));
    catalog.recipe(88).unwrap()
}

/// A ledger of `window_type` holding `stacks` as `(slot, item, stack id, count)`.
fn stocked(window_type: i8, stacks: &[(u8, i32, i32, u16)]) -> PlayerInventoryLedger {
    let mut ledger = ledger(window_type);
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

/// Every queued request's actions, oldest first.
fn requests(ledger: &PlayerInventoryLedger) -> Vec<Vec<StackRequestAction>> {
    ledger
        .queue
        .iter()
        .map(|request| request.actions.clone())
        .collect()
}

/// `(source container, source slot, destination container, destination slot, amount)`.
fn place(
    action: &StackRequestAction,
) -> (StackRequestContainer, u8, StackRequestContainer, u8, u8) {
    let StackRequestAction::Place {
        amount,
        source,
        destination,
    } = action
    else {
        panic!("expected a place, got {action:?}");
    };
    (
        source.container,
        source.slot,
        destination.container,
        destination.slot,
        *amount,
    )
}

fn grid_count(ledger: &PlayerInventoryLedger, slot: u8) -> Option<(i32, u16)> {
    ledger
        .target_stack(InventoryTarget::Craft(slot))
        .map(|stack| (stack.network_id, stack.count))
}

#[test]
fn a_book_click_fills_the_grid_with_every_craft_the_inventory_supplies() {
    let mut ledger = stocked(PERSONAL_INVENTORY_WINDOW_TYPE, &[(0, LOG, 100, 3)]);
    ledger.begin_recipe_placement(&fixture(1)).unwrap();
    let requests = requests(&ledger);
    assert_eq!(requests.len(), 1, "one Place, no auto-craft: {requests:?}");
    assert_eq!(
        place(&requests[0][0]),
        (
            StackRequestContainer::CombinedInventory,
            0,
            StackRequestContainer::CraftingInput,
            28,
            3
        )
    );
    assert_eq!(grid_count(&ledger, 28), Some((LOG, 3)));
    assert!(ledger.displayed_stack(0).is_none());
    assert!(ledger.crafting_ghost_stack(28).is_none());
}

#[test]
fn each_recipe_cell_is_its_own_request_at_the_grid_top_left() {
    let ring = fixture(2);
    let mut ledger = stocked(WORKBENCH_WINDOW_TYPE, &[(4, COBBLE, 100, 20)]);
    ledger.begin_recipe_placement(&ring).unwrap();
    let requests = requests(&ledger);
    let destinations: Vec<_> = requests
        .iter()
        .map(|actions| {
            assert_eq!(actions.len(), 1);
            let (_, source, _, destination, amount) = place(&actions[0]);
            assert_eq!((source, amount), (4, 2));
            destination
        })
        .collect();
    assert_eq!(destinations, [32, 33, 34, 35, 37, 38, 39, 40]);
    assert!(grid_count(&ledger, 36).is_none());
    assert_eq!(ledger.displayed_stack(4).unwrap().count, 4);

    // A two-cell column sits in the first column of the personal grid.
    let mut ledger = stocked(
        PERSONAL_INVENTORY_WINDOW_TYPE,
        &[(9, PLANKS, 100, 1), (10, LOG, 101, 1)],
    );
    let column = shaped(
        1,
        2,
        vec![
            IngredientFixture::named("minecraft:oak_planks", 0),
            IngredientFixture::named("minecraft:oak_log", 0),
        ],
    );
    ledger.begin_recipe_placement(&column).unwrap();
    assert_eq!(grid_count(&ledger, 28), Some((PLANKS, 1)));
    assert_eq!(grid_count(&ledger, 30), Some((LOG, 1)));
}

#[test]
fn a_missing_ingredient_stays_a_ghost_until_the_grid_changes() {
    let recipe = shaped(
        2,
        1,
        vec![
            IngredientFixture::named("minecraft:oak_log", 0),
            IngredientFixture::named("minecraft:cobblestone", 0),
        ],
    );
    let mut ledger = stocked(PERSONAL_INVENTORY_WINDOW_TYPE, &[(0, LOG, 100, 5)]);
    ledger.begin_recipe_placement(&recipe).unwrap();
    // An uncraftable recipe places one of what the player has.
    assert_eq!(grid_count(&ledger, 28), Some((LOG, 1)));
    assert_eq!(
        ledger
            .crafting_ghost_stack(29)
            .map(|ghost| ghost.network_id),
        Some(COBBLE)
    );
    assert_eq!(
        ledger.crafting_ghost_output().map(|ghost| ghost.network_id),
        Some(FURNACE)
    );
    ledger
        .begin_target_gesture(InventoryTarget::Craft(28), CellGesture::Click)
        .unwrap();
    assert!(ledger.crafting_ghost_stack(29).is_none());
    assert!(ledger.crafting_ghost_output().is_none());
}

#[test]
fn a_second_click_returns_the_grid_to_its_sources_before_placing_again() {
    let mut ledger = stocked(
        PERSONAL_INVENTORY_WINDOW_TYPE,
        &[(5, LOG, 100, 2), (6, COBBLE, 101, 1)],
    );
    ledger.apply(&craft_slot(29, stack(COBBLE, 300, 4)));
    ledger.begin_recipe_placement(&fixture(1)).unwrap();
    let first = requests(&ledger);
    // The grid's cobblestone merges into the inventory before the logs move in.
    assert_eq!(first[0].len(), 1);
    assert!(matches!(
        &first[0][0],
        StackRequestAction::Place { source, destination, amount: 4 }
            if source.container == StackRequestContainer::CraftingInput && source.slot == 29
                && destination.slot == 6
    ));
    assert_eq!(place(&first[1][0]).3, 28);
    assert_eq!(ledger.displayed_stack(6).unwrap().count, 5);

    // Clicking again puts the logs back in slot 5 and lays them out again.
    let before = first.len();
    ledger.begin_recipe_placement(&fixture(1)).unwrap();
    let again = &requests(&ledger)[before..];
    assert_eq!(
        place(&again[0][0]),
        (
            StackRequestContainer::CraftingInput,
            28,
            StackRequestContainer::PlayerInventory,
            5,
            2
        )
    );
    assert_eq!(place(&again[1][0]).3, 28);
    assert_eq!(grid_count(&ledger, 28), Some((LOG, 2)));
}

/// A ghost shows the held variant even when earlier cells used all of it.
#[test]
fn a_ghost_names_the_held_item_an_earlier_cell_used_up() {
    let recipe = shaped(
        2,
        1,
        vec![
            IngredientFixture::named("minecraft:cobblestone", 0),
            IngredientFixture::tagged("test:stone"),
        ],
    );
    let mut ledger = stocked(PERSONAL_INVENTORY_WINDOW_TYPE, &[(0, COBBLE, 100, 1)]);
    let mut entries = registry().entries.to_vec();
    for entry in &mut entries {
        if entry.network_id == LOG || entry.network_id == COBBLE {
            entry.item_tags = Arc::from([Arc::from("test:stone")]);
        }
    }
    ledger.apply_registry(&ItemRegistryEvent {
        entries: entries.into(),
    });
    ledger.begin_recipe_placement(&recipe).unwrap();
    assert_eq!(grid_count(&ledger, 28), Some((COBBLE, 1)));
    assert_eq!(
        ledger
            .crafting_ghost_stack(29)
            .map(|ghost| ghost.network_id),
        Some(COBBLE)
    );
}

/// Returning the grid while a player cell is unknown could drop items that would fit there.
#[test]
fn placement_waits_until_every_player_cell_is_known() {
    let mut ledger = stocked(PERSONAL_INVENTORY_WINDOW_TYPE, &[(0, LOG, 100, 3)]);
    ledger.apply(&craft_slot(28, stack(COBBLE, 300, 4)));
    ledger.known[20] = false;
    let pending = ledger.pending_request_count();
    assert_eq!(
        ledger.begin_recipe_placement(&fixture(1)),
        Err(InventoryGestureError::ResyncRequired)
    );
    assert_eq!(ledger.pending_request_count(), pending);
    assert_eq!(grid_count(&ledger, 28), Some((COBBLE, 4)));
}

/// A tag cell leaves the only exact item to the cell that names it.
#[test]
fn placement_reserves_items_for_later_exact_ingredients() {
    let recipe = shaped(
        2,
        1,
        vec![
            IngredientFixture::tagged("test:wood"),
            IngredientFixture::named("minecraft:oak_log", 0),
        ],
    );
    let mut ledger = stocked(
        PERSONAL_INVENTORY_WINDOW_TYPE,
        &[(9, LOG, 100, 1), (10, PLANKS, 101, 1)],
    );
    let mut entries = registry().entries.to_vec();
    for entry in &mut entries {
        if entry.network_id == LOG || entry.network_id == PLANKS {
            entry.item_tags = Arc::from([Arc::from("test:wood")]);
        }
    }
    ledger.apply_registry(&ItemRegistryEvent {
        entries: entries.into(),
    });
    ledger.begin_recipe_placement(&recipe).unwrap();
    assert_eq!(grid_count(&ledger, 28), Some((PLANKS, 1)));
    assert_eq!(grid_count(&ledger, 29), Some((LOG, 1)));
}

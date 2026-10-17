use ::protocol::{
    InventoryEvent,
    test_support::recipes::{
        CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture, recipe_unlock_event,
    },
};
use inventory::InventorySession;

/// A supported wire recipe with its own exact name and fixture-owned numeric IDs.
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

/// Queues real decoded CraftingData beside unlock events in one inventory FIFO.
fn catalog(
    session: &mut InventorySession,
    sequence: u64,
    recipes: Vec<RecipeFixture>,
    clear: bool,
) {
    let update = CraftingDataFixture {
        recipes,
        clear_recipes: clear,
        ..Default::default()
    }
    .update();
    session
        .enqueue_inventory_event(1, sequence, InventoryEvent::Recipes(update))
        .unwrap();
}

/// Queues a wire-decoded, world-normalized unlock operation without bypassing the session.
fn unlock(session: &mut InventorySession, sequence: u64, operation: u32, names: &[&str]) {
    session
        .enqueue_inventory_event(1, sequence, recipe_unlock_event(operation, names))
        .unwrap();
}

/// Advances only the committed frontier and leaves all packet order in the inventory owner.
fn commit(session: &mut InventorySession, through: u64) {
    session.synchronize_crafting_frontier(1, Some((1, 0, Some(through))));
    session.drain_pending_inventory();
}

#[test]
fn receipt_time_resolution_uses_catalog_order_even_with_one_batched_drain() {
    let mut session = InventorySession::new(1);
    unlock(&mut session, 1, 2, &["test:A"]);
    catalog(
        &mut session,
        2,
        vec![recipe("test:A", 1), recipe("test:B", 2)],
        true,
    );
    commit(&mut session, 2);
    assert!(!session.recipe_unlocks().contains("test:A"));
    unlock(&mut session, 3, 1, &["test:A"]);
    unlock(&mut session, 4, 1, &["test:B"]);
    unlock(&mut session, 5, 1, &[]);
    commit(&mut session, 5);
    assert!(session.recipe_unlocks().contains("test:A"));
    assert!(session.recipe_unlocks().contains("test:B"));
    assert!(!session.recipe_unlocks().is_newly_unlocked("test:A"));
}

#[test]
fn catalog_replacement_snapshot_and_session_boundaries_preserve_exact_authority() {
    let mut session = InventorySession::new(1);
    catalog(&mut session, 1, vec![recipe("test:A", 1)], true);
    unlock(&mut session, 2, 2, &["test:A"]);
    commit(&mut session, 2);
    let snapshot = session.with_ledger(session.ledger_snapshot());
    assert!(snapshot.recipe_unlocks().is_newly_unlocked("test:A"));
    catalog(&mut session, 3, vec![], true);
    commit(&mut session, 3);
    assert!(session.recipe_unlocks().contains("test:A"));
    catalog(&mut session, 4, vec![recipe("test:A", 91)], true);
    commit(&mut session, 4);
    let current = session.crafting_catalog_snapshot().unwrap();
    let identity = current.resolve_unlock_identifier("test:A").unwrap();
    assert_eq!(identity.identity().network_id(), 91);
    assert!(session.recipe_unlocks().is_unlocked(identity.identity()));
    unlock(&mut session, 5, 3, &["test:A"]);
    commit(&mut session, 5);
    assert!(!session.recipe_unlocks().contains("test:A"));
    assert!(snapshot.recipe_unlocks().contains("test:A"));
    session.begin_session(1);
    assert!(!session.recipe_unlocks().contains("test:A"));
    session.begin_session(2);
    assert!(!session.recipe_unlocks().contains("test:A"));
    assert!(snapshot.recipe_unlocks().contains("test:A"));
}

#[test]
fn authority_waits_for_the_committed_frontier_and_bypass_retires_only_current_copy() {
    let mut session = InventorySession::new(1);
    catalog(&mut session, 1, vec![recipe("test:A", 1)], true);
    unlock(&mut session, 2, 2, &["test:A"]);
    commit(&mut session, 1);
    assert!(!session.recipe_unlocks().contains("test:A"));
    commit(&mut session, 2);
    assert!(session.recipe_unlocks().contains("test:A"));
    let snapshot = session.clone();
    unlock(&mut session, 3, 3, &["test:A"]);
    assert!(session.pop_inventory_event().is_some());
    assert!(!session.recipe_unlocks().contains("test:A"));
    assert!(snapshot.recipe_unlocks().contains("test:A"));
    let mut next = snapshot.clone();
    next.begin_session(2);
    assert!(!next.recipe_unlocks().contains("test:A"));
}

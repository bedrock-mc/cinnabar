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

#[test]
fn lost_frontiers_retire_unlocks_before_pending_removals_are_discarded() {
    for (session_id, identity) in [
        (1, None),
        (1, Some((1, 0, None))),
        (2, Some((1, 0, Some(3)))),
        (1, Some((0, 0, Some(3)))),
        (1, Some((1, 4, Some(3)))),
        (1, Some((2, 0, Some(3)))),
        (1, Some((1, 0, Some(3)))),
        (1, Some((1, 1, Some(1)))),
    ] {
        let mut session = InventorySession::new(1);
        catalog(&mut session, 1, vec![recipe("test:A", 1)], true);
        unlock(&mut session, 2, 2, &["test:A"]);
        commit(&mut session, 2);
        session.synchronize_crafting_frontier(1, Some((1, 1, Some(2))));
        session.begin_session(1);
        assert!(session.recipe_unlocks().contains("test:A"));
        let snapshot = session.clone();
        unlock(&mut session, 3, 3, &["test:A"]);
        session.synchronize_crafting_frontier(session_id, identity);
        session.drain_pending_inventory();
        assert!(
            !session.recipe_unlocks().contains("test:A"),
            "lost frontier {identity:?} for session {session_id} skipped a removal"
        );
        assert!(!session.recipe_unlocks().is_newly_unlocked("test:A"));
        assert!(snapshot.recipe_unlocks().contains("test:A"));
        assert!(snapshot.recipe_unlocks().is_newly_unlocked("test:A"));
    }
}

#[test]
fn cell_queue_loss_retires_unstaged_removals_but_preserves_unaffected_membership() {
    for pending_removal in [false, true] {
        let mut session = InventorySession::new(1);
        catalog(&mut session, 1, vec![recipe("test:A", 1)], true);
        unlock(&mut session, 2, 2, &["test:A"]);
        commit(&mut session, 2);
        let snapshot = session.clone();
        let slot = InventoryEvent::Slot(::protocol::InventorySlotEvent {
            identity: ::protocol::SlotIdentity {
                container: ::protocol::ContainerIdentity {
                    window_id: Some(::protocol::UI_INVENTORY_WINDOW_ID),
                    slot_type: Some(0),
                    dynamic_id: None,
                },
                slot: 28,
            },
            stack: ::protocol::NetworkItemStack::empty(),
            storage_item: None,
        });
        for sequence in 3..=67 {
            session
                .enqueue_inventory_event(1, sequence, slot.clone())
                .unwrap();
        }
        if pending_removal {
            unlock(&mut session, 68, 3, &["test:A"]);
        }
        commit(&mut session, if pending_removal { 68 } else { 67 });
        assert_eq!(
            session.recipe_unlocks().contains("test:A"),
            !pending_removal
        );
        assert_eq!(
            session.recipe_unlocks().is_newly_unlocked("test:A"),
            !pending_removal
        );
        assert!(snapshot.recipe_unlocks().contains("test:A"));
    }
}

#[test]
fn rejected_ingress_preserves_membership_and_a_capacity_retry_still_applies() {
    let mut session = InventorySession::new(1);
    catalog(&mut session, 1, vec![recipe("test:A", 1)], true);
    unlock(&mut session, 2, 2, &["test:A"]);
    commit(&mut session, 2);
    for (session_id, sequence) in [(2, 3), (1, 2)] {
        assert!(
            session
                .enqueue_inventory_event(session_id, sequence, recipe_unlock_event(3, &["test:A"]))
                .is_err()
        );
        assert!(session.recipe_unlocks().contains("test:A"));
    }
    let end = 2 + inventory::MAX_PENDING_INVENTORY_EVENTS as u64;
    for sequence in 3..=end {
        session
            .enqueue_inventory_event(1, sequence, InventoryEvent::Options(Default::default()))
            .unwrap();
    }
    assert!(
        session
            .enqueue_inventory_event(1, end + 1, recipe_unlock_event(3, &["test:A"]))
            .is_err()
    );
    assert!(session.recipe_unlocks().contains("test:A"));
    commit(&mut session, end);
    assert!(session.recipe_unlocks().contains("test:A"));
    unlock(&mut session, end + 1, 3, &["test:A"]);
    commit(&mut session, end + 1);
    assert!(!session.recipe_unlocks().contains("test:A"));
}

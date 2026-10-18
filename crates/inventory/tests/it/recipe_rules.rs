use std::sync::Arc;

use ::protocol::{
    CraftingRules, InventoryContentEvent, InventoryEvent, ItemRegistryEvent, NetworkItemStack,
    test_support::recipes::{
        CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture, UnlockingFixture,
        recipe_unlock_event,
    },
};
use inventory::{CraftGridMatch, InventorySession};

/// Builds a named wire recipe whose discovery context can vary independently of its output.
fn recipe(name: &str, id: u32, always: bool) -> RecipeFixture {
    RecipeFixture {
        net_id: id,
        unlocking: always.then_some(UnlockingFixture {
            context: 1,
            ..Default::default()
        }),
        ..RecipeFixture::shapeless(
            name,
            vec![IngredientFixture::named("minecraft:oak_log", 0)],
            vec![OutputFixture::new(
                ::protocol::vanilla_item_registry()
                    .iter()
                    .find(|entry| entry.identifier.as_ref() == "minecraft:oak_planks")
                    .unwrap()
                    .network_id,
                1,
            )],
        )
    }
}

/// Commits decoded inventory events through the same ordered owner as production ingress.
fn commit(session: &mut InventorySession, sequence: u64, event: InventoryEvent) {
    session.enqueue_inventory_event(1, sequence, event).unwrap();
    session.synchronize_crafting_frontier(1, Some((1, 0, Some(sequence))));
    session.drain_pending_inventory();
}

/// Bootstraps after idle login drains, then installs a recipe and a known crafting input.
fn session(always: bool) -> InventorySession {
    let registry = ItemRegistryEvent {
        entries: ::protocol::vanilla_item_registry(),
    };
    let log = registry
        .entries
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:oak_log")
        .unwrap()
        .network_id;
    let mut session = InventorySession::new(1);
    for _ in 0..3 {
        session.synchronize_crafting_frontier(1, None);
        session.drain_pending_inventory();
    }
    assert!(session.publish_bootstrap_inventory(
        Some(registry),
        InventoryEvent::Authority(::protocol::InventoryAuthority::Server),
    ));
    session.publish_crafting_rules_bootstrap(CraftingRules::default());
    commit(
        &mut session,
        2,
        InventoryEvent::Recipes(
            CraftingDataFixture::clearing(vec![recipe("test:locked", 9, always)]).update(),
        ),
    );
    let mut slots = vec![NetworkItemStack::empty(); ::protocol::UI_SLOT_COUNT];
    slots[usize::from(*::protocol::CRAFTING_INPUT_SLOTS.start())] = NetworkItemStack {
        network_id: log,
        count: 1,
        stack_network_id: 10,
        ..NetworkItemStack::empty()
    };
    commit(
        &mut session,
        3,
        InventoryEvent::Content(InventoryContentEvent {
            container: ::protocol::ContainerIdentity::window(::protocol::UI_INVENTORY_WINDOW_ID),
            slots: Arc::from(slots),
            storage_item: NetworkItemStack::empty(),
        }),
    );
    session
}

#[test]
fn manual_crafting_checks_unlocks_only_when_both_rules_require_them() {
    for (unlocking, limited) in [(false, false), (false, true), (true, false), (true, true)] {
        for always in [false, true] {
            let mut session = session(always);
            commit(
                &mut session,
                4,
                InventoryEvent::CraftingRules(CraftingRules {
                    recipes_unlock: Some(unlocking),
                    limited_crafting: Some(limited),
                }),
            );
            if unlocking && limited {
                assert!(
                    matches!(session.crafting_match(), CraftGridMatch::Unavailable),
                    "missing discovery component is not an empty component"
                );
                assert!(session.begin_crafting().is_err());
                assert!(session.begin_crafting_all().is_err());
                assert_eq!(session.ledger().pending_request_count(), 0);
                commit(&mut session, 5, recipe_unlock_event(0, &[]));
                assert_eq!(
                    matches!(session.crafting_match(), CraftGridMatch::Unique(_)),
                    always
                );
                commit(&mut session, 6, recipe_unlock_event(1, &["test:locked"]));
                assert!(matches!(
                    session.crafting_match(),
                    CraftGridMatch::Unique(_)
                ));
                commit(&mut session, 7, recipe_unlock_event(3, &["test:locked"]));
                assert_eq!(
                    matches!(session.crafting_match(), CraftGridMatch::Unique(_)),
                    always
                );
            } else {
                assert!(matches!(
                    session.crafting_match(),
                    CraftGridMatch::Unique(_)
                ));
            }
        }
    }
}

#[test]
fn recipe_book_distinguishes_absent_discovery_from_an_empty_component() {
    let mut session = session(false);
    assert_eq!(session.book_recipes(false, false, 0, usize::MAX).len(), 1);
    commit(&mut session, 4, recipe_unlock_event(0, &[]));
    assert!(session.book_recipes(false, false, 0, usize::MAX).is_empty());
}

#[test]
fn recipe_book_survives_prebootstrap_drains_without_unlock_packets() {
    let mut session = session(false);
    let mut material = session
        .ledger()
        .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(
            *::protocol::CRAFTING_INPUT_SLOTS.start(),
        ))
        .unwrap()
        .clone();
    material.stack_network_id = 12;
    let mut slots =
        vec![NetworkItemStack::empty(); inventory::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = material;
    commit(
        &mut session,
        4,
        InventoryEvent::Content(InventoryContentEvent {
            container: ::protocol::ContainerIdentity::window(0),
            slots: Arc::from(slots),
            storage_item: NetworkItemStack::empty(),
        }),
    );
    assert!(session.ledger_mut().request_personal_open(42));
    assert!(session.ledger_mut().mark_transport_enqueued(0));
    commit(
        &mut session,
        5,
        InventoryEvent::Open(::protocol::ContainerOpenEvent {
            container: ::protocol::ContainerIdentity::window(1),
            window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
            position: [0; 3],
            runtime_entity_id: -1,
        }),
    );
    for search_active in [false, true] {
        let mut session = session.clone();
        let entries = session.book_recipes(false, search_active, 0, 10);
        assert_eq!(entries.len(), 1);
        assert!(session.book_recipe_selectable(&entries[0].recipe));
        assert!(session.begin_book_craft(&entries[0].recipe).is_ok());
        assert_eq!(session.ledger().pending_request_count(), 1);
    }
}

#[test]
fn recipe_book_rule_matrix_keeps_locked_search_results_unselectable() {
    for (unlocking, limited) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut session = session(false);
        commit(&mut session, 4, recipe_unlock_event(0, &[]));
        commit(
            &mut session,
            5,
            InventoryEvent::CraftingRules(CraftingRules {
                recipes_unlock: Some(unlocking),
                limited_crafting: Some(limited),
            }),
        );
        assert_eq!(
            session.book_recipes(false, false, 0, 10).len(),
            usize::from(!unlocking)
        );
        let searched = session.book_recipes(false, true, 0, 10);
        assert_eq!(searched.len(), usize::from(!unlocking || !limited));
        if unlocking && !limited {
            assert!(!session.book_recipe_selectable(&searched[0].recipe));
            assert!(session.begin_book_craft(&searched[0].recipe).is_err());
            assert_eq!(session.ledger().pending_request_count(), 0);
        }
    }
}

#[test]
fn recipe_identity_stays_with_its_record_and_unlocks_do_not_spread_between_variants() {
    let mut session = session(false);
    let mut locked = recipe("test:other", 3, false);
    locked.priority = -1;
    let mut shaped = recipe("test:locked", 9, false);
    shaped.shape = Some((1, 1));
    commit(
        &mut session,
        4,
        InventoryEvent::Recipes(CraftingDataFixture::clearing(vec![shaped, locked]).update()),
    );
    let catalog = session.crafting_catalog_snapshot().unwrap();
    assert_eq!(
        catalog.recipe(3).unwrap().identity().unwrap().identifier(),
        "test:other"
    );
    assert_eq!(
        catalog.recipe(9).unwrap().identity().unwrap().identifier(),
        "test:locked"
    );
    let stale = catalog.recipe(9).unwrap();
    commit(
        &mut session,
        5,
        InventoryEvent::CraftingRules(CraftingRules {
            recipes_unlock: Some(true),
            limited_crafting: Some(true),
        }),
    );
    commit(&mut session, 6, recipe_unlock_event(1, &["test:locked"]));
    assert!(
        matches!(session.crafting_match(), CraftGridMatch::Unique(recipe) if recipe.network_id() == 9)
    );
    assert_eq!(
        session.book_recipes(false, false, 0, 10)[0]
            .recipe
            .network_id(),
        9
    );
    commit(&mut session, 7, recipe_unlock_event(3, &["test:locked"]));
    assert!(matches!(session.crafting_match(), CraftGridMatch::NoMatch));
    assert!(session.book_recipes(false, false, 0, 10).is_empty());
    commit(
        &mut session,
        8,
        InventoryEvent::Recipes(
            CraftingDataFixture::clearing(vec![recipe("test:other", 9, true)]).update(),
        ),
    );
    assert!(session.begin_book_craft(&stale).is_err());
}

#[test]
fn rule_observations_wait_for_commit_and_lost_authority_does_not_reuse_defaults() {
    let mut session = session(false);
    assert!(!session.crafting_rules().unwrap().requires_manual_unlock());
    session
        .enqueue_inventory_event(
            1,
            4,
            InventoryEvent::CraftingRules(CraftingRules {
                recipes_unlock: None,
                limited_crafting: Some(true),
            }),
        )
        .unwrap();
    session.drain_pending_inventory();
    assert!(!session.crafting_rules().unwrap().requires_manual_unlock());
    session.synchronize_crafting_frontier(1, Some((1, 0, Some(4))));
    session.finish_drain();
    assert!(session.crafting_rules().unwrap().requires_manual_unlock());
    session.synchronize_crafting_frontier(1, None);
    assert!(session.crafting_rules().is_none());
    let next = session.clone();
    session.begin_session(2);
    assert!(!session.crafting_rules().unwrap().requires_manual_unlock());
    assert!(next.crafting_rules().is_none());
}

#[test]
fn rule_updates_in_the_outer_fifo_are_retired_when_cell_pressure_discards_the_frontier() {
    let mut session = session(false);
    let snapshot = session.clone();
    let end = 3 + inventory::MAX_PENDING_INVENTORY_EVENTS as u64;
    for sequence in 4..end {
        session
            .enqueue_inventory_event(
                1,
                sequence,
                InventoryEvent::Slot(::protocol::InventorySlotEvent {
                    identity: ::protocol::SlotIdentity {
                        container: ::protocol::ContainerIdentity::window(
                            ::protocol::UI_INVENTORY_WINDOW_ID,
                        ),
                        slot: u16::from(*::protocol::CRAFTING_INPUT_SLOTS.start()),
                    },
                    stack: NetworkItemStack::empty(),
                    storage_item: None,
                }),
            )
            .unwrap();
    }
    commit(
        &mut session,
        end,
        InventoryEvent::CraftingRules(CraftingRules {
            recipes_unlock: None,
            limited_crafting: Some(true),
        }),
    );
    assert!(session.crafting_rules().is_none());
    assert!(!snapshot.crafting_rules().unwrap().requires_manual_unlock());
}

#[test]
fn recipe_book_keeps_materials_from_locked_siblings_without_granting_selection() {
    let mut session = session(false);
    let locked = recipe("test:locked", 9, false);
    let mut granted = recipe("test:granted", 10, false);
    granted.ingredients = vec![IngredientFixture::named("minecraft:crimson_stem", 0)];
    commit(
        &mut session,
        4,
        InventoryEvent::Recipes(CraftingDataFixture::clearing(vec![locked, granted]).update()),
    );
    commit(&mut session, 5, recipe_unlock_event(1, &["test:granted"]));
    let mut material = session
        .ledger()
        .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(
            *::protocol::CRAFTING_INPUT_SLOTS.start(),
        ))
        .unwrap()
        .clone();
    material.count = 8;
    material.stack_network_id = 12;
    commit(
        &mut session,
        6,
        InventoryEvent::Content(InventoryContentEvent {
            container: ::protocol::ContainerIdentity::window(0),
            slots: Arc::from([material]),
            storage_item: NetworkItemStack::empty(),
        }),
    );
    for filtering in [false, true] {
        for searched in [false, true] {
            let entries = session.book_recipes(filtering, searched, 0, 10);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].recipe.network_id(), 10);
            assert!(entries[0].craftable);
            assert!(session.book_recipe_selectable(&entries[0].recipe));
            assert!(session.begin_book_craft(&entries[0].recipe).is_err());
        }
    }
    let locked = session.screen_catalog().unwrap().recipe(9).unwrap();
    assert!(!session.book_recipe_selectable(&locked));
    assert!(session.begin_book_craft(&locked).is_err());
    assert_eq!(session.ledger().pending_request_count(), 0);
    commit(
        &mut session,
        7,
        InventoryEvent::Content(InventoryContentEvent {
            container: ::protocol::ContainerIdentity::window(0),
            slots: Arc::from([NetworkItemStack::empty()]),
            storage_item: NetworkItemStack::empty(),
        }),
    );
    assert!(session.book_recipes(true, false, 0, 10).is_empty());
    assert!(!session.book_recipes(false, false, 0, 10)[0].craftable);
}

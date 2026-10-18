use inventory::inventory_ledger::InventoryTarget;
use inventory::{CraftingPreview, InventorySession};
use protocol::test_support::recipes::CraftingDataFixture;
use protocol::*;
use std::sync::Arc;

/// Builds a named packet address without inventing a response address.
fn address(window: i32, name: Option<u8>) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(window),
        slot_type: name,
        dynamic_id: None,
    }
}

/// Creates one original stack with a valid normalized empty-data digest.
fn item() -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        stack_network_id: 101,
        count: 1,
        ..NetworkItemStack::empty()
    }
}

/// Builds a complete personal UI snapshot with the specified raw cursor cell.
fn full_ui(name: Option<u8>, cursor: NetworkItemStack) -> InventoryEvent {
    let mut slots = vec![NetworkItemStack::empty(); UI_SLOT_COUNT];
    slots[0] = cursor;
    InventoryEvent::Content(InventoryContentEvent {
        container: address(UI_INVENTORY_WINDOW_ID, name),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    })
}

/// Builds a single raw Slot event at the given packet address.
fn slot(window: i32, name: Option<u8>, position: u16, stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: address(window, name),
            slot: position,
        },
        stack,
        storage_item: None,
    })
}

/// Commits one event through the public session FIFO and world frontier.
fn commit(session: &mut InventorySession, sequence: u64, event: InventoryEvent) {
    session.enqueue_inventory_event(1, sequence, event).unwrap();
    session.synchronize_crafting_frontier(1, Some((1, 0, Some(sequence))));
    session.drain_pending_inventory();
}

/// Establishes server authority, a registry, and an available empty recipe catalog.
fn session() -> InventorySession {
    let mut session = InventorySession::new(1);
    let registry = ItemRegistryEvent {
        entries: Arc::from([ItemRegistryEntry {
            identifier: Arc::from("minecraft:oak_log"),
            network_id: 6,
            component_based: false,
            version: ItemRegistryVersion::None,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    };
    session.publish_bootstrap_inventory(
        Some(registry),
        InventoryEvent::Authority(InventoryAuthority::Server),
    );
    commit(
        &mut session,
        1,
        InventoryEvent::Recipes(CraftingDataFixture::clearing(vec![]).update()),
    );
    session
}

/// Establishes known empty grid and cursor using the already-supported named Slot control.
fn known_empty() -> InventorySession {
    let mut session = session();
    commit(&mut session, 2, full_ui(None, NetworkItemStack::empty()));
    commit(
        &mut session,
        3,
        slot(
            UI_INVENTORY_WINDOW_ID,
            Some(CONTAINER_NAME_CURSOR),
            0,
            NetworkItemStack::empty(),
        ),
    );
    assert_eq!(session.crafting_preview(), Some(CraftingPreview::NoMatch));
    session
}

#[test]
fn named_cursor_positive_control() {
    let mut session = known_empty();
    commit(
        &mut session,
        4,
        slot(
            UI_INVENTORY_WINDOW_ID,
            Some(CONTAINER_NAME_CURSOR),
            0,
            item(),
        ),
    );
    assert!(session.crafting_preview().is_none());
    commit(
        &mut session,
        5,
        slot(
            UI_INVENTORY_WINDOW_ID,
            Some(CONTAINER_NAME_CURSOR),
            0,
            NetworkItemStack::empty(),
        ),
    );
    assert_eq!(session.crafting_preview(), Some(CraftingPreview::NoMatch));
}

#[test]
fn full_ui_snapshot_establishes_empty_cursor() {
    let mut session = session();
    commit(&mut session, 2, full_ui(None, NetworkItemStack::empty()));
    assert!(session.ledger().cursor_stack().is_none());
    assert_eq!(
        session.crafting_preview(),
        Some(CraftingPreview::NoMatch),
        "full UI snapshot establishes all five passive crafting cells"
    );
}

#[test]
fn full_ui_occupied_cursor_suppresses_preview() {
    let mut session = known_empty();
    commit(&mut session, 4, full_ui(None, item()));
    assert!(session.ledger().cursor_stack().is_some());
    assert!(
        session.crafting_preview().is_none(),
        "passive cursor must follow the admitted full-content cursor"
    );
}

#[test]
fn raw_ui_cursor_clear_recovers_preview() {
    let mut session = known_empty();
    commit(
        &mut session,
        4,
        slot(
            UI_INVENTORY_WINDOW_ID,
            Some(CONTAINER_NAME_CURSOR),
            0,
            item(),
        ),
    );
    commit(
        &mut session,
        5,
        slot(UI_INVENTORY_WINDOW_ID, None, 0, NetworkItemStack::empty()),
    );
    assert!(session.ledger().cursor_stack().is_none());
    assert_eq!(
        session.crafting_preview(),
        Some(CraftingPreview::NoMatch),
        "raw UI slot zero clears the same cursor"
    );
}

#[test]
fn named_furnace_slot_cannot_mutate_passive_cursor() {
    let mut session = known_empty();
    commit(
        &mut session,
        4,
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(7),
            window_type: WINDOW_TYPE_FURNACE,
            position: [0; 3],
            runtime_entity_id: -1,
        }),
    );
    commit(
        &mut session,
        5,
        slot(7, Some(CONTAINER_NAME_CURSOR), 0, item()),
    );
    assert_eq!(
        session
            .ledger()
            .target_stack(InventoryTarget::Storage(0))
            .unwrap()
            .stack_network_id,
        101
    );
    assert!(session.ledger().cursor_stack().is_none());
    assert_eq!(
        session.crafting_preview(),
        Some(CraftingPreview::NoMatch),
        "ordinary furnace slot updates its storage, irrespective of its packet name"
    );
}

#[test]
fn wrong_length_named_content_cannot_invent_cursor() {
    let mut session = known_empty();
    commit(
        &mut session,
        4,
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(7),
            window_type: WINDOW_TYPE_FURNACE,
            position: [0; 3],
            runtime_entity_id: -1,
        }),
    );
    commit(
        &mut session,
        5,
        InventoryEvent::Content(InventoryContentEvent {
            container: address(7, Some(CONTAINER_NAME_CURSOR)),
            slots: Arc::from([item()]),
            storage_item: NetworkItemStack::empty(),
        }),
    );
    assert!(session.ledger().cursor_stack().is_none());
    assert_eq!(
        session.crafting_preview(),
        Some(CraftingPreview::NoMatch),
        "rejected short furnace Content cannot establish passive cursor authority"
    );
}

#[test]
fn raw_ui_offhand_name_still_addresses_cursor_zero() {
    let mut session = known_empty();
    commit(
        &mut session,
        4,
        slot(
            UI_INVENTORY_WINDOW_ID,
            Some(CONTAINER_NAME_OFFHAND),
            0,
            item(),
        ),
    );
    assert!(
        session.crafting_preview().is_none(),
        "raw UI slot zero is cursor, even when its descriptor names Offhand"
    );
}

#[test]
fn remaining_full_ui_cursor_name_still_observes_cursor_zero() {
    let mut session = known_empty();
    commit(
        &mut session,
        4,
        full_ui(Some(CONTAINER_NAME_CURSOR), item()),
    );
    assert!(
        session.crafting_preview().is_none(),
        "complete UI Content updates cursor zero regardless of packet descriptor"
    );
}

#[test]
fn remaining_control_equipment_left_does_not_change_selection_or_admit_cells() {
    let mut session = InventorySession::new(1);
    session.publish_local_runtime_id(1, 42).unwrap();
    session.set_local_selected_slot(2);
    let event = EquipmentEvent {
        actor_runtime_id: 42,
        stack: item(),
        inventory_slot: 0,
        selected_slot: 8,
        window_id: u8::try_from(OFFHAND_WINDOW_ID).unwrap(),
        handedness: Some(ActorHandedness::Left),
    };
    let routed = session.route_equipment(1, 1, event).unwrap();
    let inventory::EquipmentRouteResult::Routed(inventory::EquipmentRoute::LocalSelected {
        fifo_sequence,
        event,
    }) = routed
    else {
        panic!("local route")
    };
    session.retain_local_selected_equipment(fifo_sequence, event);
    assert_eq!(session.selected_hotbar_slot(None), Some(2));
    assert!(session.local_selected_equipment().is_none());
    assert!(
        session
            .ledger()
            .target_stack(InventoryTarget::Offhand)
            .is_none()
    );
    assert!(session.ledger().displayed_stack(8).is_none());
}

#[test]
fn remaining_control_normal_transaction_establishes_five_cells() {
    let mut session = session();
    let mut updates = Vec::new();
    for index in [28, 29, 30, 31] {
        updates.push(InventorySlotEvent {
            identity: SlotIdentity {
                container: address(UI_INVENTORY_WINDOW_ID, Some(0)),
                slot: index,
            },
            stack: NetworkItemStack::empty(),
            storage_item: None,
        });
    }
    updates.push(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(CONTAINER_NAME_CURSOR),
                dynamic_id: None,
            },
            slot: 0,
        },
        stack: NetworkItemStack::empty(),
        storage_item: None,
    });
    commit(
        &mut session,
        2,
        InventoryEvent::Transaction(InventoryTransactionEvent {
            slots: updates.into(),
            skipped_actions: 0,
        }),
    );
    assert_eq!(session.crafting_preview(), Some(CraftingPreview::NoMatch));
}

#[test]
fn remaining_control_malformed_ui_content_does_not_establish_cursor() {
    let mut session = known_empty();
    commit(
        &mut session,
        4,
        InventoryEvent::Content(InventoryContentEvent {
            container: address(UI_INVENTORY_WINDOW_ID, None),
            slots: Arc::from([item()]),
            storage_item: NetworkItemStack::empty(),
        }),
    );
    assert!(session.ledger().cursor_stack().is_none());
    assert_eq!(session.crafting_preview(), Some(CraftingPreview::NoMatch));
}

#[test]
fn hotbar_flag_controls_numeric_selection_at_fifo_receipt() {
    for window in [PLAYER_INVENTORY_WINDOW_ID, 7] {
        let mut session = InventorySession::new(1);
        session.queue_local_hotbar_selection(3, None);
        commit(
            &mut session,
            1,
            InventoryEvent::SelectedSlot(SelectedSlotEvent {
                container: ContainerIdentity::window(window),
                slot: 8,
                select_slot: false,
            }),
        );
        assert_eq!(session.selected_hotbar_slot(None), Some(3));
        assert_eq!(session.pending_hotbar_selection(), Some(3));
        commit(
            &mut session,
            2,
            InventoryEvent::SelectedSlot(SelectedSlotEvent {
                container: ContainerIdentity::window(window),
                slot: 8,
                select_slot: true,
            }),
        );
        assert_eq!(session.selected_hotbar_slot(None), Some(8));
        assert_eq!(session.pending_hotbar_selection(), None);
    }
}

/// Crafting observes exactly the fixed registry writes admitted by the current screen.
#[test]
fn review_registry_observations_follow_ledger_admission() {
    for screen in [false, true] {
        for dynamic_id in [None, Some(9)] {
            for full in [false, true] {
                for occupied in [false, true] {
                    let mut session = known_empty();
                    if screen {
                        assert!(session.ledger_mut().request_personal_open(42));
                        assert!(session.ledger_mut().mark_transport_enqueued(0));
                        commit(
                            &mut session,
                            4,
                            InventoryEvent::Open(ContainerOpenEvent {
                                container: ContainerIdentity::window(2),
                                window_type:
                                    inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
                                position: [0, 64, 0],
                                runtime_entity_id: -1,
                            }),
                        );
                    }
                    let before = if occupied {
                        item()
                    } else {
                        NetworkItemStack::empty()
                    };
                    commit(
                        &mut session,
                        5,
                        slot(UI_INVENTORY_WINDOW_ID, None, 0, before),
                    );
                    let incoming = if occupied {
                        NetworkItemStack::empty()
                    } else {
                        item()
                    };
                    let container = ContainerIdentity {
                        window_id: Some(DYNAMIC_STORAGE_WINDOW_ID),
                        slot_type: Some(CONTAINER_NAME_CURSOR),
                        dynamic_id,
                    };
                    let event = if full {
                        InventoryEvent::Content(InventoryContentEvent {
                            container,
                            slots: vec![incoming].into(),
                            storage_item: NetworkItemStack::empty(),
                        })
                    } else {
                        InventoryEvent::Slot(InventorySlotEvent {
                            identity: SlotIdentity { container, slot: 0 },
                            stack: incoming,
                            storage_item: None,
                        })
                    };
                    commit(&mut session, 6, event);
                    let empty = if screen && dynamic_id.is_none() {
                        occupied
                    } else {
                        !occupied
                    };
                    assert_eq!(session.ledger().cursor_stack().is_none(), empty);
                    assert_eq!(
                        session.crafting_preview().is_some(),
                        empty,
                        "screen={screen} dynamic={dynamic_id:?} full={full} occupied={occupied}"
                    );
                }
            }
        }
    }
}

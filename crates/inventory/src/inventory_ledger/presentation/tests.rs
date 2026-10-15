use super::*;
use protocol::{ContainerIdentity, InventoryEvent, InventorySlotEvent, SlotIdentity};

/// Restates a player cell, including known-empty writes.
fn write(ledger: &mut PlayerInventoryLedger, stack: NetworkItemStack) {
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 0,
        },
        stack,
        storage_item: None,
    }));
}

#[test]
fn snapshots_detect_unknown_to_known_and_identical_server_restatements() {
    let mut ledger = PlayerInventoryLedger::default();
    let unknown = ledger.presentation_snapshot();
    assert!(unknown.matches(&ledger));
    write(&mut ledger, NetworkItemStack::empty());
    assert!(!unknown.matches(&ledger));
    let empty = ledger.presentation_snapshot();
    assert!(empty.matches(&ledger));
    write(&mut ledger, NetworkItemStack::empty());
    assert!(!empty.matches(&ledger));
    let old_session = ledger.presentation_snapshot();
    ledger.begin_session(1);
    assert!(!old_session.matches(&ledger));
}

#[test]
fn snapshots_detect_in_place_overlay_and_ui_mutations_through_shared_cells() {
    use crate::inventory_ledger::cells::{Cell, Held};
    let mut ledger = PlayerInventoryLedger::default();
    let stack = NetworkItemStack {
        network_id: 1,
        count: 1,
        stack_network_id: 9,
        ..NetworkItemStack::empty()
    };
    write(&mut ledger, stack.clone());
    let before = ledger.presentation_snapshot();
    let held = ledger.confirmed.get_mut(Cell::Inventory(0)).unwrap();
    held.overlay = Some(crate::inventory_ledger::StackResponseOverlay {
        durability_correction: Some(7),
        ..Default::default()
    });
    assert!(!before.matches(&ledger));
    let before = ledger.presentation_snapshot();
    ledger.confirmed.set(
        Cell::Craft(*protocol::CRAFTING_INPUT_SLOTS.start()),
        Held::new(&stack),
    );
    assert!(!before.matches(&ledger));
    let before = ledger.presentation_snapshot();
    ledger.confirmed.set(Cell::CreatedOutput, Held::new(&stack));
    assert!(!before.matches(&ledger));
    let before = ledger.presentation_snapshot();
    ledger
        .confirmed
        .ensure_storage(crate::inventory_ledger::LARGE_STORAGE_SLOT_COUNT);
    assert!(!before.matches(&ledger));
    let before = ledger.presentation_snapshot();
    ledger
        .confirmed
        .get_mut(Cell::CreatedOutput)
        .unwrap()
        .stack
        .count = 2;
    assert!(!before.matches(&ledger));
}

#[test]
fn rejected_predictions_restore_a_different_retained_display_source() {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.apply(&InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    write(
        &mut ledger,
        NetworkItemStack {
            network_id: 1,
            count: 5,
            stack_network_id: 9,
            ..NetworkItemStack::empty()
        },
    );
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: crate::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let before = ledger.presentation_snapshot();
    let request = ledger.begin_take_count(0, 2).unwrap();
    assert!(!before.matches(&ledger));
    let predicted = ledger.presentation_snapshot();
    assert!(predicted.matches(&ledger));
    assert!(ledger.mark_transport_enqueued(1));
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                status: protocol::StackResponseStatus::Rejected,
                request_id: request,
                containers: Arc::from([]),
            }]),
        },
    ));
    assert!(!predicted.matches(&ledger));
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 5);
}

#[test]
fn session_clone_snapshots_track_display_changes_without_journal_ownership() {
    let mut session = crate::InventorySession::new(1);
    write(
        session.ledger_mut(),
        NetworkItemStack {
            network_id: 1,
            count: 1,
            ..NetworkItemStack::empty()
        },
    );
    let snapshot = session.ledger().presentation_snapshot();
    let mut copy = session.clone();
    write(copy.ledger_mut(), NetworkItemStack::empty());
    assert!(!snapshot.matches(copy.ledger()));
    assert!(snapshot.matches(session.ledger()));
}

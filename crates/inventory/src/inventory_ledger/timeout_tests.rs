use super::*;

#[test]
fn an_unanswered_personal_open_keeps_the_requested_screen_usable() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));

    ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);

    assert!(ledger.personal_inventory_desired_open());
    assert!(!ledger.resync_required());
    assert!(ledger.pending_batch().unwrap().is_none());
    assert!(ledger.begin_click(0).is_ok());
}

#[test]
fn a_late_personal_reply_preserves_gestures_from_the_requested_screen() {
    for answered in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        let take = ledger.begin_click(0).unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        if answered {
            accept_cursor_move(&mut ledger, take, false);
        }
        ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        ledger.apply(&InventoryEvent::Open(personal_open(7)));
        if !answered {
            assert_eq!(ledger.pending_request_id(), Some(take));
            accept_cursor_move(&mut ledger, take, false);
        }
        assert!(ledger.personal_inventory_desired_open());
        assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(32));
        assert!(!ledger.resync_required());
    }
}

#[test]
fn timeout_requires_refresh_for_inputs_before_releasing_personal_ownership() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    let slot = *protocol::CRAFTING_INPUT_SLOTS.start();
    let ingredient = stack(32, 2);
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(protocol::CONTAINER_NAME_CRAFT_INPUT),
                dynamic_id: None,
            },
            slot: u16::from(slot),
        },
        stack: ingredient.clone(),
        storage_item: None,
    }));
    assert!(!ledger.resync_required());
    ledger.request_personal_close();
    assert!(ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert!(ledger.resync_required());
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(slot)),
        Some(&ingredient)
    );
}

#[test]
fn missing_close_reply_releases_its_slot_and_reconciles_latest_intent() {
    for desired_open in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        acknowledge_personal_open(&mut ledger, 2);
        ledger.request_personal_close();
        assert!(ledger.mark_transport_enqueued(20));
        if desired_open {
            assert!(ledger.request_personal_open(42));
        }
        let now = 20 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
        assert!(ledger.poll_timeout(now));
        assert!(!ledger.poll_timeout(now));
        assert_eq!(
            ledger.pending_control_packet().unwrap().is_some(),
            desired_open
        );
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(now + 1));
        ledger.apply(&InventoryEvent::Open(personal_open(2)));
        assert!(ledger.personal_inventory_desired_open());
    }
}

#[test]
fn missing_open_reply_releases_admission_without_closing_a_guessed_window() {
    for target in [42, 900] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(target));
        assert!(ledger.mark_transport_enqueued(10));
        let now = 10 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
        assert!(ledger.poll_timeout(now));
        assert!(!ledger.poll_timeout(now));
        assert!(ledger.personal_inventory_desired_open());
        assert!(ledger.pending_batch().unwrap().is_none());
        ledger.request_personal_close();
        assert!(ledger.request_personal_open(target + 1));
        assert_eq!(
            ledger.pending_batch().unwrap().unwrap().0,
            protocol::open_inventory_packet(target + 1).unwrap()
        );
        assert!(ledger.mark_transport_enqueued(now + 1));
        ledger.apply(&InventoryEvent::Open(personal_open(7)));
        assert!(ledger.begin_click(0).is_ok());
    }
}

#[test]
fn a_late_server_open_after_timeout_uses_its_actual_window_id() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    let now = 10 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
    assert!(ledger.poll_timeout(now));
    ledger.apply(&InventoryEvent::Open(personal_open(7)));
    assert!(ledger.personal_inventory_desired_open());
    assert!(ledger.pending_batch().unwrap().is_none());
    ledger.request_personal_close();
    assert_eq!(
        ledger.pending_batch().unwrap().unwrap().0,
        protocol::container_close_packet(7).unwrap()
    );
}

#[test]
fn an_open_timeout_preserves_an_unsent_world_drop() {
    let mut ledger = ledger_with_slot_zero();
    ledger.begin_world_drop(0, Some(1)).unwrap();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    let now = 10 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
    assert!(ledger.poll_timeout(now));
    assert!(ledger.pending_control_packet().unwrap().is_none());
    assert!(ledger.mark_transport_enqueued(now));
    assert!(ledger.pending_batch().unwrap().is_none());
}

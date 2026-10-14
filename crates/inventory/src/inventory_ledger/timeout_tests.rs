use super::*;

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
    assert!(ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert!(ledger.resync_required());
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(slot)),
        Some(&ingredient)
    );
}

#[test]
fn missing_open_reply_resets_the_server_before_retrying_the_latest_intent() {
    for desired_open in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        if !desired_open {
            ledger.request_personal_close();
        }
        let now = 10 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
        assert!(ledger.poll_timeout(now));
        assert!(!ledger.poll_timeout(now));
        assert_eq!(ledger.personal_inventory_desired_open(), desired_open);
        assert!(ledger.personal.is_none());
        assert_eq!(
            protocol::test_support::packets::container_close(
                &ledger.pending_control_packet().unwrap().unwrap()
            )
            .unwrap()
            .container_id,
            u8::try_from(protocol::PLAYER_INVENTORY_WINDOW_ID).unwrap()
        );
        assert!(ledger.mark_transport_enqueued(now));
        assert!(!ledger.mark_transport_enqueued(now));
        ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
            window_type: NO_CONTAINER_WINDOW_TYPE,
            server_initiated: false,
        }));
        assert_eq!(
            ledger.pending_control_packet().unwrap().is_some(),
            desired_open
        );
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(now + 1));
        ledger.apply(&InventoryEvent::Open(personal_open(0)));
        assert!(matches!(ledger.personal, Some(PersonalWindow::Open { .. })));
    }
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
fn a_late_open_supplies_the_id_for_timeout_recovery() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    let now = 10 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
    assert!(ledger.poll_timeout(now));
    ledger.apply(&InventoryEvent::Open(personal_open(7)));
    assert_eq!(
        protocol::test_support::packets::container_close(
            &ledger.pending_control_packet().unwrap().unwrap()
        )
        .unwrap()
        .container_id,
        7
    );
    assert!(ledger.mark_transport_enqueued(now));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(7),
        window_type: NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert!(ledger.mark_transport_enqueued(now + 1));
    ledger.apply(&InventoryEvent::Open(personal_open(8)));
    assert!(matches!(ledger.personal, Some(PersonalWindow::Open { .. })));
    assert_eq!(ledger.controls.personal.confirmed.unwrap().window_id, 8);
}

#[test]
fn timeout_recovery_waits_for_an_unsent_world_drop() {
    let mut ledger = ledger_with_slot_zero();
    ledger.begin_world_drop(0, Some(1)).unwrap();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    let now = 10 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
    assert!(ledger.poll_timeout(now));
    assert!(ledger.pending_control_packet().unwrap().is_none());
    assert!(ledger.mark_transport_enqueued(now));
    assert!(ledger.pending_control_packet().unwrap().is_some());
    assert!(ledger.mark_transport_enqueued(now + 1));
}

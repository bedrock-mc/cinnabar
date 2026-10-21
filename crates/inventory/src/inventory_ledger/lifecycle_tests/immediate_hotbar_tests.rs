use super::*;

#[test]
fn first_frame_hotbar_swap_waits_behind_open_and_survives_transport_pressure() {
    for authority in [InventoryAuthority::Server, InventoryAuthority::Client] {
        let mut session = InventorySession::new(1);
        *session.ledger_mut() = ledger_with_slot_zero();
        let ledger = session.ledger_mut();
        ledger.apply(&InventoryEvent::Authority(authority));
        assert!(ledger.request_personal_open(42));
        let open = ledger.pending_batch().unwrap().unwrap().0;
        let id = ledger
            .begin_hotbar_swap(InventoryTarget::Player(0), 1)
            .unwrap();
        assert!(ledger.displayed_stack(0).is_none());
        assert_eq!(ledger.displayed_stack(1).unwrap().count, 32);
        assert_eq!(ledger.pending_batch().unwrap().unwrap().0, open);
        assert_eq!(
            session.flush_inventory_send(10, |packet| {
                assert_eq!(packet, open);
                Err("full")
            }),
            Err("full")
        );
        assert_eq!(session.ledger().pending_request_id(), Some(id));
        let mut sent = Vec::new();
        session
            .flush_inventory_send(20, |packet| {
                sent.push(packet);
                Ok::<_, &str>(())
            })
            .unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0], open);
        assert_ne!(sent[1], open);
        assert!(session.ledger().pending_batch().unwrap().is_none());
    }
}

#[test]
fn cancelling_an_unsent_open_rolls_back_first_frame_hotbar_swaps() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    ledger
        .begin_hotbar_swap(InventoryTarget::Player(0), 1)
        .unwrap();
    ledger.request_personal_close();
    assert!(ledger.pending_batch().unwrap().is_none());
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, 9);
    assert!(ledger.displayed_stack(1).is_none());
    assert_eq!(
        ledger.begin_hotbar_swap(InventoryTarget::Player(0), 1),
        Err(InventoryGestureError::PersonalInventoryUnavailable)
    );
    assert!(ledger.request_personal_open(42));
    ledger
        .begin_hotbar_swap(InventoryTarget::Player(0), 2)
        .unwrap();
    ledger.begin_session(2);
    assert!(ledger.pending_batch().unwrap().is_none());
}

/// A client part's modal shows the player's cells with no window open: its hotbar keys send the
/// swap as the in-world drop sends its request, while a container screen's still need the window.
#[test]
fn windowless_hotbar_swaps_send_without_an_open_inventory() {
    let mut ledger = ledger_with_slot_zero();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    assert_eq!(
        ledger.begin_hotbar_swap(InventoryTarget::Player(0), 1),
        Err(InventoryGestureError::PersonalInventoryUnavailable)
    );
    assert_eq!(
        ledger.begin_windowless_hotbar_swap(InventoryTarget::Armor(0), 1),
        Err(InventoryGestureError::InvalidRequest)
    );
    let id = ledger
        .begin_windowless_hotbar_swap(InventoryTarget::Player(0), 1)
        .unwrap();
    assert_eq!(ledger.pending_request_id(), Some(id));
    assert!(ledger.displayed_stack(0).is_none());
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 32);
    assert!(ledger.pending_batch().unwrap().is_some());
}

#[test]
fn a_server_window_supersedes_unsent_personal_hotbar_swaps() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    ledger
        .begin_hotbar_swap(InventoryTarget::Player(0), 1)
        .unwrap();
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(4),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, 9);
    assert!(ledger.displayed_stack(1).is_none());
    assert!(ledger.pending_batch().unwrap().is_none());
}

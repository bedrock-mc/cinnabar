use super::*;

/// A historical response must use the same effective maximum as a current response.
#[test]
fn historic_correction_respects_capacity_with_a_newer_owner() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 8))]);
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        16,
        0,
    ));
    let first = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    let second = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    let third = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    send_all(&mut ledger);
    respond(
        &mut ledger,
        second,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            6,
            92,
        )],
    );
    assert!(
        !ledger.queue[0].predicted[0].active,
        "first owner is historic"
    );
    assert!(
        !ledger.queue[0].predicted[0].current,
        "newer settlement invalidated older rollback"
    );
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count, 6);
    respond(
        &mut ledger,
        first,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            17,
            91,
        )],
    );
    assert_eq!(ledger.pending_request_id(), Some(third));
    assert_eq!(
        ledger.displayed_stack(0).unwrap().count,
        5,
        "newer prediction remains visible"
    );
    assert_eq!(
        ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count,
        6,
        "historic above-capacity correction must not publish"
    );
}

/// A mining request's selected source must not authorize an unrelated requested slot.
#[test]
fn mining_response_ignores_unowned_requested_cells() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 8)), (1, stack(7, 11, 3))]);
    let mining = ledger.begin_mining_request(0, 1).unwrap();
    respond(
        &mut ledger,
        mining,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            1,
            2,
            91,
        )],
    );
    assert_eq!(
        ledger.confirmed_stack(Cell::Inventory(1)).unwrap().count,
        3,
        "unowned mining correction must skip the unrelated source"
    );
}

/// A selected mining snapshot supplies a response that remaps into an empty player cell.
#[test]
fn mining_response_remaps_the_requested_snapshot() {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&player_content(&[(0, stack(6, 10, 1))]));
    let mining = ledger.begin_mining_request(0, 1).unwrap();
    let mut remapped = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 1, 1, 91);
    remapped.slots = Arc::from([StackResponseSlot {
        hotbar_slot: 0,
        durability_correction: 1,
        ..remapped.slots[0].clone()
    }]);
    respond(
        &mut ledger,
        mining,
        StackResponseStatus::Accepted,
        vec![remapped],
    );
    assert_eq!(
        ledger
            .confirmed_stack(Cell::Inventory(1))
            .map(|held| (held.network_id, held.stack_network_id)),
        Some((6, 91)),
        "requested mining source supplies the empty remap destination"
    );
}

/// An ordinary screen response cannot borrow an untouched backing item as a sparse owner.
#[test]
fn unowned_screen_correction_preserves_backing() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 8)), (1, stack(7, 11, 3))]);
    let request = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    send_all(&mut ledger);
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            1,
            2,
            91,
        )],
    );
    assert_eq!(
        ledger
            .confirmed_stack(Cell::Inventory(1))
            .map(|held| (held.count, held.stack_network_id)),
        Some((3, 11)),
        "an untouched requested cell has no correction owner"
    );
}

/// A current prediction must reject a correction beyond the effective item maximum.
#[test]
fn current_correction_respects_item_capacity() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 8))]);
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        16,
        0,
    ));
    let request = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    send_all(&mut ledger);
    assert!(ledger.queue[0].predicted[0].active);
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            17,
            91,
        )],
    );
    assert_eq!(
        ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count,
        8,
        "current above-capacity correction must not publish"
    );
}

/// A later front-screen prediction masks, but does not reject, a HUD correction to backing.
#[test]
fn hud_mining_corrects_backing_under_later_front_prediction() {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:iron_pickaxe",
        6,
        1,
        0,
    ));
    ledger.apply(&player_content(&[(0, stack(6, 10, 1))]));
    ledger.apply(&cursor_content(NetworkItemStack::empty()));
    let mining = ledger.begin_mining_request(0, 1).unwrap();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let screen = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    send_all(&mut ledger);
    assert!(ledger.displayed_stack(0).is_none());
    let mut corrected = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 1, 91);
    corrected.slots = Arc::from([StackResponseSlot {
        durability_correction: 1,
        ..corrected.slots[0].clone()
    }]);
    respond(
        &mut ledger,
        mining,
        StackResponseStatus::Accepted,
        vec![corrected],
    );
    assert_eq!(ledger.pending_request_id(), Some(screen));
    assert!(ledger.displayed_stack(0).is_none());
    assert_eq!(
        ledger
            .confirmed_stack(Cell::Inventory(0))
            .unwrap()
            .stack_network_id,
        91,
        "the HUD correction must update backing beneath the front-screen prediction"
    );
}

/// Closing an unrelated front screen leaves the retained HUD request able to settle.
#[test]
fn screen_close_keeps_hud_mining_response() {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&player_content(&[(0, stack(6, 10, 1))]));
    ledger.apply(&cursor_content(NetworkItemStack::empty()));
    let mining = ledger.begin_mining_request(0, 1).unwrap();
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger.request_storage_close();
    assert!(ledger.mark_transport_enqueued(1));
    ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: protocol::NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert!(ledger.storage_generation().is_none());
    let mut corrected = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 1, 91);
    corrected.slots = Arc::from([StackResponseSlot {
        durability_correction: 1,
        ..corrected.slots[0].clone()
    }]);
    respond(
        &mut ledger,
        mining,
        StackResponseStatus::Accepted,
        vec![corrected],
    );
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, 91);
    assert!(!ledger.resync_required());
}

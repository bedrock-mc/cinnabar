//! Cursor-free moves: hotbar swaps, drops and quick moves.

use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_LEVEL_ENTITY, ContainerIdentity, ContainerOpenEvent, InventoryContentEvent,
    InventoryEvent, InventorySlotEvent, NetworkItemStack, NormalInventoryChange,
    NormalInventorySource, SlotIdentity, StackRequestAction, StackRequestContainer,
};

use super::*;

#[path = "storage_quick_move_tests.rs"]
mod storage_tests;

mod world_drop;

/// Reads the reserved wire identity of a drop still awaiting transport.
fn drop_legacy_id(ledger: &PlayerInventoryLedger, request_id: i32) -> i32 {
    ledger
        .queue
        .iter()
        .find(|request| request.request_id == request_id)
        .expect("queued drop")
        .world_drop
        .expect("world drop identity")
        .legacy_request_id
}

fn stack(network_id: i32, stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        stack_network_id,
        count,
        ..NetworkItemStack::default()
    }
}

fn open_ledger(slots: &[(usize, NetworkItemStack)]) -> PlayerInventoryLedger {
    let mut ledger = known_ledger(slots);
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
}

/// Establishes server-backed player slots and a parsed apple capacity declaration.
fn known_ledger(slots: &[(usize, NetworkItemStack)]) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        protocol::ITEM_DEFAULT_MAX_STACK_SIZE,
        0,
    ));
    let mut content = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    for (slot, stack) in slots {
        content[*slot] = stack.clone();
    }
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(content),
        storage_item: NetworkItemStack::default(),
    }));
    ledger
}

/// Two occupied cells Swap; an empty hotbar cell receives one Place.
#[test]
fn hotbar_swap_uses_swap_or_place() {
    let mut ledger = open_ledger(&[(20, stack(8, 100, 3)), (2, stack(9, 55, 1))]);
    ledger
        .begin_hotbar_swap(InventoryTarget::Player(20), 2)
        .unwrap();
    assert!(matches!(
        ledger.newest_action(),
        Some(StackRequestAction::Swap { source, destination })
            if source.slot == 20 && destination.slot == 2
    ));
    assert_eq!(ledger.displayed_stack(2).unwrap().stack_network_id, -3);
    assert_eq!(ledger.displayed_stack(20).unwrap().stack_network_id, -3);
    assert_eq!(ledger.displayed_stack(2).unwrap().network_id, 8);
    assert_eq!(ledger.displayed_stack(20).unwrap().network_id, 9);

    ledger
        .begin_hotbar_swap(InventoryTarget::Player(21), 4)
        .map(|_| ())
        .unwrap_err();
    ledger
        .begin_hotbar_swap(InventoryTarget::Player(20), 5)
        .unwrap();
    let Some(StackRequestAction::Place {
        amount,
        source,
        destination,
    }) = ledger.newest_action()
    else {
        panic!("placing into the empty hotbar cell");
    };
    assert_eq!((amount, source.slot, destination.slot), (1, 20, 5));
    assert_eq!(
        destination.container,
        StackRequestContainer::PlayerInventory
    );
    assert_eq!(destination.stack_network_id, 0);
    assert_eq!(
        ledger.begin_hotbar_swap(InventoryTarget::Player(3), 3),
        Err(InventoryGestureError::InvalidRequest)
    );
}

/// Drops send one Drop action and shrink the source; rejection restores it.
#[test]
fn drop_shrinks_the_source_until_rejected() {
    let original = stack(8, 100, 5);
    let mut ledger = open_ledger(&[(20, original.clone())]);
    let request = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(20)), Some(1))
        .unwrap();
    assert!(matches!(
        ledger.newest_action(),
        Some(StackRequestAction::Drop { amount: 1, source, randomly: false })
            if source.slot == 20 && source.stack_network_id == 100
    ));
    assert_eq!(ledger.displayed_stack(20).unwrap().count, 4);
    assert_eq!(
        ledger.begin_drop(DropSource::Target(InventoryTarget::Player(20)), Some(5)),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                status: protocol::StackResponseStatus::Rejected,
                request_id: request,
                containers: Arc::from([]),
            }]),
        },
    ));
    assert_eq!(ledger.displayed_stack(20), Some(&original));
    assert_eq!(
        ledger.begin_drop(DropSource::Cursor, None),
        Err(InventoryGestureError::EmptyGesture)
    );
}

/// Quick moves go hotbar to main inventory and back, merging into a
/// compatible partial stack first and spilling the rest into empty cells.
#[test]
fn quick_move_prefers_compatible_partial_stacks() {
    let mut ledger = open_ledger(&[
        (0, stack(6, 10, 10)),
        (30, stack(6, 11, 60)),
        (4, stack(8, 12, 1)),
    ]);
    ledger.begin_quick_move(InventoryTarget::Player(0)).unwrap();
    let Some(StackRequestAction::Place {
        amount,
        destination,
        ..
    }) = ledger.newest_action()
    else {
        panic!("a merge place first");
    };
    assert_eq!(
        (amount, destination.slot, destination.stack_network_id),
        (4, 30, 11)
    );
    assert_eq!(ledger.displayed_stack(30).unwrap().count, 64);
    assert_eq!(ledger.displayed_stack(0), None);
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 6);

    ledger.begin_quick_move(InventoryTarget::Player(4)).unwrap();
    let Some(StackRequestAction::Place { destination, .. }) = ledger.newest_action() else {
        panic!("a single place");
    };
    assert_eq!((destination.slot, destination.stack_network_id), (10, 0));
    assert_eq!(ledger.displayed_stack(10).unwrap().stack_network_id, -5);
}

/// With storage open, player stacks quick-move into the storage window.
#[test]
fn quick_move_from_player_targets_open_storage() {
    let mut ledger = known_ledger(&[(20, stack(8, 100, 3))]);
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(7),
            slot_type: Some(CONTAINER_NAME_LEVEL_ENTITY),
            dynamic_id: Some(91),
        },
        slots: Arc::from(vec![NetworkItemStack::default(); SMALL_STORAGE_SLOT_COUNT]),
        storage_item: NetworkItemStack::default(),
    }));
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    let Some(StackRequestAction::Place { destination, .. }) = ledger.newest_action() else {
        panic!("a single place");
    };
    assert_eq!(
        destination.container,
        StackRequestContainer::LevelEntity { dynamic_id: None }
    );
    assert_eq!(ledger.storage_stack(0).unwrap().stack_network_id, -3);
}

/// The in-world drop key needs no open window, unlike a screen drop.
#[test]
fn world_drop_works_without_an_open_window() {
    let mut ledger = known_ledger(&[(2, stack(8, 100, 3))]);
    assert_eq!(
        ledger.begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1)),
        Err(InventoryGestureError::PersonalInventoryUnavailable)
    );
    ledger.begin_world_drop(2, Some(1)).unwrap();
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 2);
}

#[test]
fn world_drop_feedback_is_once_per_admitted_gesture_not_per_item() {
    let mut ledger = known_ledger(&[(2, stack(8, 100, 3))]);
    ledger.begin_world_drop(2, Some(1)).unwrap();
    ledger.begin_world_drop(2, None).unwrap();
    assert_eq!(ledger.take_world_drops(), 2);
    assert_eq!(ledger.take_world_drops(), 0);
    assert!(ledger.mark_transport_enqueued(10));
    assert_eq!(ledger.take_world_drops(), 0);
}

#[test]
fn failed_world_drops_do_not_emit_feedback() {
    let mut ledger = known_ledger(&[(2, stack(8, 100, 3))]);
    for (slot, amount) in [(1, Some(1)), (2, Some(0)), (2, Some(4)), (36, None)] {
        assert!(ledger.begin_world_drop(slot, amount).is_err());
        assert_eq!(ledger.take_world_drops(), 0);
    }
    ledger.begin_world_drop(2, Some(1)).unwrap();
    ledger.begin_session(2);
    assert_eq!(ledger.take_world_drops(), 0);
}

#[test]
fn screen_drops_do_not_emit_world_drop_feedback() {
    let mut ledger = open_ledger(&[(2, stack(8, 100, 3))]);
    ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
        .unwrap();
    assert_eq!(ledger.take_world_drops(), 0);
}

/// Like the native client, a drop key press leaves as a normal transaction rather than an item
/// stack request, so servers see no inventory request from a player walking with no screen open.
#[test]
fn world_drop_leaves_as_the_native_drop_transaction() {
    let held = digested(stack(8, 100, 3));
    let mut ledger = known_ledger(&[(2, held.clone())]);
    let request = ledger.begin_world_drop(2, Some(1)).unwrap();
    let legacy_id = drop_legacy_id(&ledger, request);
    let (packet, entries) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(entries, 1);
    let verify = |stack: NetworkItemStack| {
        protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap()
    };
    let mut remainder = held.clone();
    remainder.count = 2;
    let mut dropped = held.clone();
    dropped.count = 1;
    let expected = protocol::inventory_drop_transaction_packet(
        2,
        Some(legacy_id),
        vec![
            NormalInventoryChange {
                source: NormalInventorySource::Container(0),
                slot: 2,
                from: verify(held),
                to: verify(remainder).renamed(legacy_id),
            },
            NormalInventoryChange {
                source: NormalInventorySource::World { randomly: false },
                slot: 0,
                from: verify(NetworkItemStack::empty()),
                to: verify(dropped),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        protocol::encode(&packet).unwrap(),
        protocol::encode(&expected).unwrap()
    );

    // The server restates the slot only when it disagrees, so the sent drop is local truth.
    assert!(ledger.mark_transport_enqueued(10));
    assert_eq!(ledger.pending_state(), None);
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 2);
    assert_eq!(
        ledger.displayed_stack(2).unwrap().stack_network_id,
        legacy_id
    );
    assert!(ledger.pending_batch().unwrap().is_none());
}

/// A later request names a drop's remainder by its legacy id, which servers accept for it.
#[test]
fn requests_after_a_world_drop_name_the_remainder_by_its_legacy_id() {
    let mut ledger = known_ledger(&[(2, digested(stack(8, 100, 3)))]);
    let request = ledger.begin_world_drop(2, Some(1)).unwrap();
    let legacy_id = drop_legacy_id(&ledger, request);
    assert!(ledger.mark_transport_enqueued(10));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
        .unwrap();
    let Some(StackRequestAction::Drop { source, .. }) = ledger.newest_action() else {
        panic!("a screen drop");
    };
    assert_eq!(source.stack_network_id, legacy_id);
    assert!(ledger.pending_batch().unwrap().is_some());
}

#[test]
fn server_correction_does_not_repeat_world_drop_feedback() {
    let original = digested(stack(8, 100, 3));
    let mut ledger = known_ledger(&[(2, original.clone())]);
    ledger.begin_world_drop(2, Some(1)).unwrap();
    assert_eq!(ledger.take_world_drops(), 1);
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 2,
        },
        stack: original.clone(),
        storage_item: None,
    }));
    assert_eq!(ledger.displayed_stack(2), Some(&original));
    assert_eq!(ledger.take_world_drops(), 0);
}

/// `stack` with the digest of its extra data, as stacks decoded from the wire carry.
fn digested(mut stack: NetworkItemStack) -> NetworkItemStack {
    use sha2::{Digest, Sha256};
    stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
    stack
}

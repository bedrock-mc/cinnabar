//! Cursor-free moves: hotbar swaps, drops and quick moves.

use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_LEVEL_ENTITY, ContainerIdentity, ContainerOpenEvent, InventoryContentEvent,
    InventoryEvent, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, NetworkItemStack,
    StackRequestAction, StackRequestContainer,
};

use super::*;

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

fn known_ledger(slots: &[(usize, NetworkItemStack)]) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: vec![ItemRegistryEntry {
            identifier: Arc::from("minecraft:apple"),
            network_id: 6,
            component_based: true,
            version: ItemRegistryVersion::DataDriven,
            component_digest: [6; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: false,
            item_tags: std::sync::Arc::from([]),
        }]
        .into(),
    });
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
        StackRequestContainer::LevelEntity {
            dynamic_id: Some(91)
        }
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

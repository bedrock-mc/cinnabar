use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CURSOR,
    CONTAINER_NAME_LEVEL_ENTITY, ContainerCloseEvent, ContainerIdentity, ContainerOpenEvent,
    InventoryContentEvent, InventoryEvent, InventorySlotEvent, ItemStackResponseEvent,
    NetworkItemStack, SlotIdentity, StackRequestAction, StackResponse, StackResponseContainer,
    StackResponseSlot, StackResponseStatus,
};

use super::*;

fn stack(stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        count,
        stack_network_id,
        ..NetworkItemStack::default()
    }
}

fn cursor_content(value: Option<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(-1),
            slot_type: Some(CONTAINER_NAME_CURSOR),
            dynamic_id: None,
        },
        slots: Arc::from([value.unwrap_or_default()]),
        storage_item: NetworkItemStack::default(),
    })
}

fn player_ledger(
    source: Option<NetworkItemStack>,
    cursor: Option<NetworkItemStack>,
) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    if let Some(source) = source {
        slots[0] = source;
    }
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    }));
    ledger.apply(&cursor_content(cursor));
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

fn storage_ledger(
    source: Option<NetworkItemStack>,
    cursor: Option<NetworkItemStack>,
) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    let mut slots = vec![NetworkItemStack::default(); SMALL_STORAGE_SLOT_COUNT];
    if let Some(source) = source {
        slots[2] = source;
    }
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(7),
            slot_type: Some(CONTAINER_NAME_LEVEL_ENTITY),
            dynamic_id: Some(91),
        },
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    }));
    ledger.apply(&cursor_content(cursor));
    ledger
}

fn correction(
    container: ContainerIdentity,
    slot: u8,
    count: u8,
    stack_id: i32,
) -> StackResponseContainer {
    StackResponseContainer {
        container,
        slots: Arc::from([StackResponseSlot {
            slot,
            hotbar_slot: slot,
            count,
            item_stack_id: stack_id,
            custom_name: Arc::from(""),
            filtered_custom_name: Arc::from(""),
            durability_correction: 0,
        }]),
    }
}

fn response(
    request_id: i32,
    status: StackResponseStatus,
    containers: Vec<StackResponseContainer>,
) -> InventoryEvent {
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status,
            request_id,
            containers: Arc::from(containers),
        }]),
    })
}

fn overlay(name: &str) -> StackResponseOverlay {
    StackResponseOverlay {
        custom_name: Some(Arc::from(name)),
        filtered_custom_name: None,
        durability_correction: Some(3),
    }
}

#[test]
fn partial_take_and_place_split_counts_ids_and_overlays() {
    let original = stack(44, 12);
    let original_overlay = overlay("split");
    let mut take = player_ledger(Some(original.clone()), None);
    take.slot_overlays[0] = Some(original_overlay.clone());

    assert_eq!(take.begin_take_count(0, 5).unwrap(), -3);
    let pending = take.pending.as_ref().unwrap();
    assert!(matches!(
        pending.action,
        StackRequestAction::Take {
            amount: 5,
            source: protocol::StackRequestSlot {
                stack_network_id: 44,
                ..
            },
            destination: protocol::StackRequestSlot {
                stack_network_id: 0,
                ..
            }
        }
    ));
    assert_eq!(take.displayed_stack(0).unwrap().count, 7);
    assert_eq!(take.displayed_stack(0).unwrap().stack_network_id, 44);
    assert_eq!(take.cursor_stack().unwrap().count, 5);
    assert_eq!(take.cursor_stack().unwrap().stack_network_id, 44);
    assert_eq!(
        pending.prediction.source_overlay,
        Some(original_overlay.clone())
    );
    assert_eq!(
        pending.prediction.destination_overlay,
        Some(original_overlay.clone())
    );

    let mut place = player_ledger(None, Some(original));
    place.cursor_overlay = Some(original_overlay.clone());
    assert_eq!(place.begin_place_count(0, 5).unwrap(), -3);
    let pending = place.pending.as_ref().unwrap();
    assert!(matches!(
        pending.action,
        StackRequestAction::Place {
            amount: 5,
            source: protocol::StackRequestSlot {
                stack_network_id: 44,
                ..
            },
            destination: protocol::StackRequestSlot {
                stack_network_id: 0,
                ..
            }
        }
    ));
    assert_eq!(place.cursor_stack().unwrap().count, 7);
    assert_eq!(place.cursor_stack().unwrap().stack_network_id, 44);
    assert_eq!(place.displayed_stack(0).unwrap().count, 5);
    assert_eq!(place.displayed_stack(0).unwrap().stack_network_id, 44);
    assert_eq!(
        pending.prediction.source_overlay,
        Some(original_overlay.clone())
    );
    assert_eq!(
        pending.prediction.destination_overlay,
        Some(original_overlay)
    );
}

#[test]
fn full_count_operations_match_existing_take_and_place_predictions() {
    let original = stack(44, 12);
    let mut explicit_take = player_ledger(Some(original.clone()), None);
    let mut click_take = player_ledger(Some(original.clone()), None);
    explicit_take.begin_take_count(0, 12).unwrap();
    click_take.begin_click(0).unwrap();
    assert_eq!(
        explicit_take.pending.as_ref().unwrap().action,
        click_take.pending.as_ref().unwrap().action
    );
    assert_eq!(
        explicit_take.displayed_stack(0),
        click_take.displayed_stack(0)
    );
    assert_eq!(explicit_take.cursor_stack(), click_take.cursor_stack());

    let mut explicit_place = player_ledger(None, Some(original.clone()));
    let mut click_place = player_ledger(None, Some(original));
    explicit_place.begin_place_count(0, 12).unwrap();
    click_place.begin_click(0).unwrap();
    assert_eq!(
        explicit_place.pending.as_ref().unwrap().action,
        click_place.pending.as_ref().unwrap().action
    );
    assert_eq!(
        explicit_place.displayed_stack(0),
        click_place.displayed_stack(0)
    );
    assert_eq!(explicit_place.cursor_stack(), click_place.cursor_stack());
}

#[test]
fn invalid_amounts_and_occupied_destinations_are_atomic() {
    let original = stack(44, 12);
    let mut take = player_ledger(Some(original.clone()), None);
    for amount in [0, 13, 256] {
        assert_eq!(
            take.begin_take_count(0, amount),
            Err(InventoryGestureError::InvalidRequest)
        );
        assert_eq!(take.next_request_id, -3);
        assert!(take.pending.is_none());
        assert_eq!(take.displayed_stack(0), Some(&original));
        assert!(take.cursor_stack().is_none());
    }

    let occupied = stack(55, 2);
    let mut take_occupied = player_ledger(Some(original.clone()), Some(occupied.clone()));
    assert_eq!(
        take_occupied.begin_take_count(0, 1),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(take_occupied.next_request_id, -3);
    assert!(take_occupied.pending.is_none());

    let mut place_occupied = player_ledger(Some(occupied), Some(original.clone()));
    assert_eq!(
        place_occupied.begin_place_count(0, 1),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(place_occupied.next_request_id, -3);
    assert!(place_occupied.pending.is_none());

    let mut empty = player_ledger(None, None);
    assert_eq!(
        empty.begin_take_count(0, 1),
        Err(InventoryGestureError::EmptyGesture)
    );
    assert_eq!(
        empty.begin_place_count(0, 1),
        Err(InventoryGestureError::EmptyGesture)
    );
    assert_eq!(empty.next_request_id, -3);

    let mut place = player_ledger(None, Some(original.clone()));
    for amount in [0, 13, 256] {
        assert_eq!(
            place.begin_place_count(0, amount),
            Err(InventoryGestureError::InvalidRequest)
        );
        assert_eq!(place.next_request_id, -3);
        assert!(place.pending.is_none());
        assert_eq!(place.cursor_stack(), Some(&original));
        assert!(place.displayed_stack(0).is_none());
    }
}

#[test]
fn accepted_partial_take_reconciles_distinct_server_ids_and_newer_authority_wins() {
    let original = stack(44, 12);
    let mut accepted = player_ledger(Some(original.clone()), None);
    let request = accepted.begin_take_count(0, 5).unwrap();
    assert!(accepted.mark_transport_enqueued(10));
    accepted.apply(&response(
        request,
        StackResponseStatus::Accepted,
        vec![
            correction(
                ContainerIdentity {
                    window_id: None,
                    slot_type: Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                    dynamic_id: None,
                },
                0,
                7,
                101,
            ),
            correction(
                ContainerIdentity {
                    window_id: None,
                    slot_type: Some(CONTAINER_NAME_CURSOR),
                    dynamic_id: None,
                },
                0,
                5,
                102,
            ),
        ],
    ));
    assert_eq!(
        (
            accepted.displayed_stack(0).unwrap().count,
            accepted.displayed_stack(0).unwrap().stack_network_id
        ),
        (7, 101)
    );
    assert_eq!(
        (
            accepted.cursor_stack().unwrap().count,
            accepted.cursor_stack().unwrap().stack_network_id
        ),
        (5, 102)
    );

    let newer = stack(77, 20);
    let mut raced = player_ledger(Some(original), None);
    let request = raced.begin_take_count(0, 5).unwrap();
    assert!(raced.mark_transport_enqueued(10));
    raced.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 0,
        },
        stack: newer.clone(),
        storage_item: None,
    }));
    raced.apply(&response(
        request,
        StackResponseStatus::Accepted,
        Vec::new(),
    ));
    assert_eq!(raced.displayed_stack(0), Some(&newer));
    assert!(raced.cursor_stack().is_none());
    assert!(raced.resync_required());
}

#[test]
fn accepted_partial_split_with_missing_or_invalid_new_identity_requires_recovery() {
    let original = stack(44, 12);
    let mut ledger = player_ledger(Some(original), None);
    let request = ledger.begin_take_count(0, 5).unwrap();
    assert!(ledger.mark_transport_enqueued(10));

    ledger.apply(&response(
        request,
        StackResponseStatus::Accepted,
        vec![
            correction(
                ContainerIdentity {
                    window_id: None,
                    slot_type: Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                    dynamic_id: None,
                },
                0,
                7,
                0,
            ),
            correction(
                ContainerIdentity {
                    window_id: None,
                    slot_type: Some(CONTAINER_NAME_CURSOR),
                    dynamic_id: None,
                },
                0,
                5,
                -1,
            ),
        ],
    ));

    assert!(ledger.resync_required());
    assert_eq!(
        ledger.begin_place_count(1, 1),
        Err(InventoryGestureError::ResyncRequired)
    );
}

#[test]
fn accepted_partial_split_can_empty_one_half_and_reuse_the_other() {
    let original = stack(44, 12);
    let mut ledger = player_ledger(Some(original), None);
    let request = ledger.begin_take_count(0, 5).unwrap();
    assert!(ledger.mark_transport_enqueued(10));

    ledger.apply(&response(
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            ContainerIdentity {
                window_id: None,
                slot_type: Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                dynamic_id: None,
            },
            0,
            0,
            -1,
        )],
    ));

    assert!(!ledger.resync_required());
    assert!(ledger.displayed_stack(0).is_none());
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 44);
    assert_eq!(ledger.begin_place_count(1, 2).unwrap(), -5);
}

#[test]
fn accepted_partial_split_can_retain_source_identity_when_destination_is_reissued() {
    let original = stack(44, 12);
    let mut ledger = player_ledger(Some(original), None);
    let request = ledger.begin_take_count(0, 5).unwrap();
    assert!(ledger.mark_transport_enqueued(10));

    ledger.apply(&response(
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            ContainerIdentity {
                window_id: None,
                slot_type: Some(CONTAINER_NAME_CURSOR),
                dynamic_id: None,
            },
            0,
            5,
            102,
        )],
    ));

    assert!(!ledger.resync_required());
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, 44);
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 102);
}

#[test]
fn rejection_timeout_and_busy_paths_keep_the_single_request_contract() {
    let original = stack(44, 12);
    let mut rejected = player_ledger(Some(original.clone()), None);
    let request = rejected.begin_take_count(0, 5).unwrap();
    assert_eq!(
        rejected.begin_take_count(0, 1),
        Err(InventoryGestureError::Busy)
    );
    assert!(rejected.mark_transport_enqueued(10));
    rejected.apply(&response(
        request,
        StackResponseStatus::Rejected,
        Vec::new(),
    ));
    assert_eq!(rejected.displayed_stack(0), Some(&original));
    assert!(rejected.cursor_stack().is_none());
    assert!(!rejected.resync_required());

    let mut timed_out = player_ledger(None, Some(original.clone()));
    timed_out.begin_place_count(0, 5).unwrap();
    assert!(timed_out.mark_transport_enqueued(10));
    timed_out.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert_eq!(timed_out.cursor_stack(), Some(&original));
    assert!(timed_out.displayed_stack(0).is_none());
    assert!(timed_out.resync_required());
}

#[test]
fn storage_count_operations_bind_generation_and_close_or_reset_safely() {
    let original = stack(91, 8);
    let mut take = storage_ledger(Some(original.clone()), None);
    assert_eq!(take.begin_storage_take_count(2, 3).unwrap(), -3);
    assert_eq!(take.storage_stack(2).unwrap().count, 5);
    assert_eq!(take.cursor_stack().unwrap().count, 3);
    assert_eq!(
        take.pending.as_ref().unwrap().storage_generation,
        take.storage_generation()
    );
    take.request_storage_close();
    assert!(take.pending.is_none());
    assert!(take.storage_generation().is_none());
    assert!(take.cursor_stack().is_none());

    let mut place = storage_ledger(None, Some(original.clone()));
    assert_eq!(place.begin_storage_place_count(2, 3).unwrap(), -3);
    assert_eq!(place.cursor_stack().unwrap().count, 5);
    assert_eq!(place.storage_stack(2).unwrap().count, 3);
    place.begin_session(2);
    assert!(place.pending.is_none());
    assert!(place.storage_generation().is_none());
    assert!(place.cursor_stack().is_none());

    let mut admitted = storage_ledger(Some(original), None);
    admitted.begin_storage_take_count(2, 3).unwrap();
    assert!(admitted.mark_transport_enqueued(10));
    admitted.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(7),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        server_initiated: true,
    }));
    assert!(admitted.pending.is_none());
    assert!(admitted.storage_generation().is_none());
    assert!(admitted.resync_required());
}

#[test]
fn unknown_closed_and_recovery_states_reject_count_operations() {
    let mut unauthorized = PlayerInventoryLedger::default();
    unauthorized.begin_session(1);
    assert_eq!(
        unauthorized.begin_take_count(0, 1),
        Err(InventoryGestureError::AuthorityUnavailable)
    );

    let mut unknown = PlayerInventoryLedger::default();
    unknown.begin_session(1);
    unknown.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    assert!(unknown.request_personal_open(42));
    assert!(unknown.mark_transport_enqueued(0));
    unknown.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    assert_eq!(
        unknown.begin_take_count(0, 1),
        Err(InventoryGestureError::UnknownSlot(0))
    );

    let mut closed = player_ledger(Some(stack(44, 12)), None);
    closed.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: true,
    }));
    assert_eq!(
        closed.begin_take_count(0, 1),
        Err(InventoryGestureError::PersonalInventoryUnavailable)
    );

    let mut recovering = player_ledger(Some(stack(44, 12)), None);
    recovering.begin_take_count(0, 1).unwrap();
    assert!(recovering.mark_transport_enqueued(10));
    recovering.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert_eq!(
        recovering.begin_take_count(0, 1),
        Err(InventoryGestureError::ResyncRequired)
    );
}

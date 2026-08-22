use std::sync::Arc;

use bedrock_client::ui_runtime::inventory_ledger::{
    INVENTORY_REQUEST_TIMEOUT_MILLIS, InventoryGestureError, InventoryPendingState,
    PlayerInventoryLedger,
};
use protocol::{
    ContainerCloseEvent, ContainerIdentity, ContainerOpenEvent, InventoryAuthority,
    InventoryContentEvent, InventoryEvent, InventorySlotEvent, ItemStackResponseEvent,
    NetworkItemStack, SlotIdentity, StackResponse, StackResponseContainer, StackResponseStatus,
};

fn stack(network_id: i32, count: u16, stack_network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        count,
        stack_network_id,
        ..NetworkItemStack::default()
    }
}

fn open(window_id: i32, window_type: i8) -> InventoryEvent {
    InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(window_id),
        window_type,
        position: [1, 2, 3],
        runtime_entity_id: -1,
    })
}

fn content(window_id: i32, dynamic_id: u32, count: usize) -> InventoryEvent {
    let mut slots = vec![NetworkItemStack::default(); count];
    if let Some(slot) = slots.get_mut(2) {
        *slot = stack(5, 3, 91);
    }
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(window_id),
            slot_type: Some(7),
            dynamic_id: Some(dynamic_id),
        },
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    })
}

fn response(request_id: i32, status: StackResponseStatus) -> InventoryEvent {
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status,
            request_id,
            containers: Arc::from([]),
        }]),
    })
}

fn response_with_storage_identity(request_id: i32, identity: ContainerIdentity) -> InventoryEvent {
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([StackResponseContainer {
                container: identity,
                slots: Arc::from([]),
            }]),
        }]),
    })
}

fn player_content() -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(vec![NetworkItemStack::default(); 36]),
        storage_item: NetworkItemStack::default(),
    })
}

fn player_content_with_first(stack: NetworkItemStack) -> InventoryEvent {
    let mut slots = vec![NetworkItemStack::default(); 36];
    slots[0] = stack;
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    })
}

fn cursor_content() -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(0),
            slot_type: Some(59),
            dynamic_id: None,
        },
        slots: Arc::from([NetworkItemStack::default()]),
        storage_item: NetworkItemStack::default(),
    })
}

fn ready(count: usize, dynamic_id: u32) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(41);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&open(1, 0));
    ledger.apply(&content(1, dynamic_id, count));
    ledger
}

#[test]
fn only_exact_27_and_54_slot_level_entity_windows_become_authoritative() {
    for count in [27, 54] {
        let ledger = ready(count, 700 + count as u32);
        assert_eq!(ledger.storage_slot_count(), Some(count));
        assert_eq!(ledger.storage_stack(2).unwrap().stack_network_id, 91);
    }

    for count in [0, 26, 28, 53, 55] {
        let ledger = ready(count, 9);
        assert_eq!(ledger.storage_slot_count(), None);
        assert!(ledger.pending_packet().unwrap().is_some());
    }
}

#[test]
fn normalized_signed_window_id_remains_a_supported_storage_identity() {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(41);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&open(-1, 0));
    ledger.apply(&content(-1, 701, 27));

    assert_eq!(ledger.storage_slot_count(), Some(27));
    assert_eq!(ledger.storage_identity().unwrap().window_id, Some(-1));
    ledger.request_storage_close();
    assert!(ledger.pending_packet().unwrap().is_some());
}

#[test]
fn storage_gestures_share_the_single_pending_cursor_ledger() {
    let mut ledger = ready(27, 777);
    let request = ledger.begin_storage_click(2).unwrap();
    assert_eq!(
        ledger.pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
    assert_eq!(ledger.begin_click(0), Err(InventoryGestureError::Busy));
    assert_eq!(ledger.cursor_stack().unwrap().count, 3);
    assert!(ledger.storage_stack(2).is_none());

    ledger.mark_transport_enqueued(10);
    ledger.apply(&response(request, StackResponseStatus::Rejected));
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.storage_stack(2).unwrap().count, 3);
}

#[test]
fn reused_window_id_has_a_new_generation_and_late_response_cannot_mutate_it() {
    let mut ledger = ready(27, 100);
    let first_generation = ledger.storage_generation().unwrap();
    let request = ledger.begin_storage_click(2).unwrap();
    ledger.mark_transport_enqueued(10);
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        server_initiated: true,
    }));
    ledger.apply(&open(1, 0));
    ledger.apply(&content(1, 200, 27));
    assert!(ledger.storage_generation().unwrap() > first_generation);

    ledger.apply(&response(request, StackResponseStatus::Accepted));
    assert_eq!(ledger.storage_identity().unwrap().dynamic_id, Some(200));
    assert_eq!(ledger.storage_stack(2).unwrap().stack_network_id, 91);
}

#[test]
fn player_cell_request_made_in_storage_ui_is_bound_to_that_open_generation() {
    let original = stack(8, 2, 44);
    let mut ledger = ready(27, 100);
    ledger.apply(&player_content_with_first(original.clone()));
    let request = ledger.begin_click(0).unwrap();
    ledger.mark_transport_enqueued(10);
    ledger.apply(&open(1, 0));
    ledger.apply(&content(1, 200, 27));
    ledger.apply(&response(request, StackResponseStatus::Accepted));

    assert_eq!(ledger.displayed_stack(0), Some(&original));
    assert_eq!(ledger.storage_identity().unwrap().dynamic_id, Some(200));
}

#[test]
fn identity_and_revision_conflicts_fail_closed_until_full_authority_recovers() {
    let mut ledger = ready(27, 300);
    let request = ledger.begin_storage_click(2).unwrap();
    ledger.mark_transport_enqueued(10);
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(1),
                slot_type: Some(7),
                dynamic_id: Some(300),
            },
            slot: 2,
        },
        stack: stack(6, 1, 92),
        storage_item: None,
    }));
    ledger.apply(&response(request, StackResponseStatus::Accepted));
    assert!(ledger.resync_required());

    ledger.apply(&content(1, 300, 27));
    assert!(
        ledger.resync_required(),
        "cursor authority is still missing"
    );
    ledger.apply(&cursor_content());
    assert!(!ledger.resync_required());
}

#[test]
fn mismatched_full_container_identity_cannot_confirm_a_storage_request() {
    let mut ledger = ready(27, 300);
    let request = ledger.begin_storage_click(2).unwrap();
    ledger.mark_transport_enqueued(10);
    ledger.apply(&response_with_storage_identity(
        request,
        ContainerIdentity {
            window_id: None,
            slot_type: Some(7),
            dynamic_id: Some(301),
        },
    ));
    assert!(ledger.resync_required());
    assert_eq!(ledger.storage_stack(2).unwrap().stack_network_id, 91);
}

#[test]
fn matching_response_full_identity_commits_without_a_window_field() {
    let mut ledger = ready(27, 300);
    let request = ledger.begin_storage_click(2).unwrap();
    ledger.mark_transport_enqueued(10);
    ledger.apply(&response_with_storage_identity(
        request,
        ContainerIdentity {
            window_id: None,
            slot_type: Some(7),
            dynamic_id: Some(300),
        },
    ));
    assert!(!ledger.resync_required());
    assert!(ledger.storage_stack(2).is_none());
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 91);
}

#[test]
fn admitted_timeout_recovers_after_all_full_authority_in_every_order() {
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut ledger = ready(27, 500);
        ledger.begin_storage_click(2).unwrap();
        ledger.mark_transport_enqueued(10);
        ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        assert!(ledger.resync_required());
        let mut storage_seen = false;
        let mut cursor_seen = false;
        for authority in order {
            let event = match authority {
                0 => {
                    storage_seen = true;
                    content(1, 500, 27)
                }
                1 => player_content(),
                2 => {
                    cursor_seen = true;
                    cursor_content()
                }
                _ => unreachable!(),
            };
            ledger.apply(&event);
            assert_eq!(
                ledger.resync_required(),
                !(storage_seen && cursor_seen),
                "order {order:?} recovered at the wrong boundary"
            );
        }
    }
}

#[test]
fn close_and_channel_pressure_are_bounded() {
    let mut ledger = ready(54, 400);
    ledger.request_storage_close();
    assert_eq!(ledger.storage_slot_count(), None);
    assert!(ledger.pending_packet().unwrap().is_some());
    ledger.note_transport_pressure(10);
    ledger.note_transport_pressure(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.pending_packet().unwrap().is_some());
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        server_initiated: true,
    }));
    assert!(ledger.pending_packet().unwrap().is_none());

    let mut unsupported = PlayerInventoryLedger::default();
    unsupported.apply(&open(9, 5));
    assert!(unsupported.pending_packet().unwrap().is_some());
    assert_eq!(unsupported.storage_slot_count(), None);
}

#[test]
fn full_content_cancellation_recovers_only_the_touched_surfaces() {
    let mut player_request = ready(27, 600);
    player_request.apply(&player_content_with_first(stack(8, 2, 44)));
    player_request.begin_click(0).unwrap();
    player_request.mark_transport_enqueued(10);
    player_request.apply(&content(1, 600, 27));
    assert!(player_request.resync_required());
    player_request.apply(&player_content());
    assert!(player_request.resync_required(), "cursor is still touched");
    player_request.apply(&cursor_content());
    assert!(!player_request.resync_required());

    let mut storage_request = ready(27, 601);
    storage_request.begin_storage_click(2).unwrap();
    storage_request.mark_transport_enqueued(10);
    storage_request.apply(&player_content());
    assert!(storage_request.resync_required());
    storage_request.apply(&cursor_content());
    assert!(
        storage_request.resync_required(),
        "storage is still touched"
    );
    storage_request.apply(&content(1, 601, 27));
    assert!(!storage_request.resync_required());
}

#[test]
fn foreign_storage_content_and_mismatched_slot_updates_are_fenced() {
    let mut ledger = ready(27, 700);
    ledger.apply(&content(2, 999, 27));
    ledger.apply(&content(1, 701, 27));
    assert_eq!(ledger.storage_identity().unwrap().dynamic_id, Some(700));
    assert_eq!(ledger.storage_stack(2).unwrap().stack_network_id, 91);
    assert!(ledger.pending_packet().unwrap().is_none());

    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(1),
            slot: 2,
        },
        stack: stack(9, 4, 99),
        storage_item: None,
    }));
    assert_eq!(ledger.storage_stack(2).unwrap().stack_network_id, 99);
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(1),
                slot_type: Some(7),
                dynamic_id: Some(999),
            },
            slot: 2,
        },
        stack: stack(10, 5, 100),
        storage_item: None,
    }));
    assert_eq!(ledger.storage_stack(2).unwrap().stack_network_id, 99);
}

#[test]
fn local_close_with_held_cursor_requires_player_and_cursor_authority() {
    let mut ledger = ready(27, 800);
    let request = ledger.begin_storage_click(2).unwrap();
    ledger.mark_transport_enqueued(10);
    ledger.apply(&response(request, StackResponseStatus::Accepted));
    assert!(ledger.cursor_stack().is_some());
    ledger.request_storage_close();
    assert!(ledger.resync_required());
    ledger.apply(&player_content());
    assert!(ledger.resync_required());
    ledger.apply(&cursor_content());
    assert!(!ledger.resync_required());
}

/// A storage gesture whose admitted prediction awaits its response, followed
/// by a local close request: the exact window that must retain its
/// generation and journal until authority settles.
fn closing_with_pending(dynamic_id: u32) -> (PlayerInventoryLedger, i32) {
    let mut ledger = ready(27, dynamic_id);
    ledger.apply(&player_content_with_first(stack(8, 2, 44)));
    let request = ledger.begin_storage_click(2).unwrap();
    ledger.mark_transport_enqueued(10);
    ledger.request_storage_close();
    (ledger, request)
}

#[test]
fn local_close_with_pending_prediction_retains_the_window_and_blocks_gestures() {
    let (mut ledger, request) = closing_with_pending(900);
    let generation = ledger
        .storage_generation()
        .expect("a pending prediction retains the closing window");
    assert_eq!(
        ledger.storage_identity().unwrap().dynamic_id,
        Some(900),
        "the container identity stays retained"
    );
    assert_eq!(
        ledger.storage_stack(2).map(|stack| stack.stack_network_id),
        None,
        "the response journal keeps the predicted-away source cell"
    );
    assert_eq!(
        ledger.cursor_stack().map(|stack| stack.stack_network_id),
        Some(91)
    );
    assert_eq!(ledger.pending_request_id(), Some(request));
    assert!(
        ledger.pending_packet().unwrap().is_some(),
        "the local ContainerClose still transmits"
    );

    // Every new gesture is blocked while the prediction awaits authority.
    assert_eq!(
        ledger.begin_storage_click(3),
        Err(InventoryGestureError::Busy)
    );
    assert_eq!(ledger.begin_click(0), Err(InventoryGestureError::Busy));

    // A duplicate close gesture cannot restart or requeue the close.
    ledger.request_storage_close();
    assert_eq!(ledger.storage_generation(), Some(generation));
    assert_eq!(ledger.storage_identity().unwrap().dynamic_id, Some(900));
}

#[test]
fn accepted_response_reconciles_then_finishes_the_deferred_close() {
    let (mut ledger, request) = closing_with_pending(910);
    ledger.apply(&response(request, StackResponseStatus::Accepted));

    assert_eq!(
        ledger.cursor_stack().map(|stack| stack.stack_network_id),
        Some(91),
        "the retained prediction reconciled instead of being dropped"
    );
    assert_eq!(
        ledger.storage_generation(),
        None,
        "the last pending resolution completed the close"
    );
    assert!(
        ledger.resync_required(),
        "a stack held out of a closed window needs authoritative restatement"
    );
    ledger.apply(&player_content());
    ledger.apply(&cursor_content());
    assert!(!ledger.resync_required());
}

#[test]
fn rejected_response_rolls_back_then_finishes_the_deferred_close() {
    let (mut ledger, request) = closing_with_pending(911);
    ledger.apply(&response(request, StackResponseStatus::Rejected));

    assert_eq!(
        ledger.cursor_stack().map(|stack| stack.stack_network_id),
        None
    );
    assert_eq!(
        ledger.storage_generation(),
        None,
        "the rejected prediction also completes the close"
    );
    assert!(!ledger.resync_required());
}

#[test]
fn closing_state_cannot_outlive_its_timeout_authority() {
    let (mut ledger, _request) = closing_with_pending(920);
    assert!(ledger.storage_generation().is_some());

    ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);

    assert_eq!(
        ledger.storage_generation(),
        None,
        "the existing timeout recovery clears the closing window"
    );
    assert!(ledger.pending_request_id().is_none());
    assert!(ledger.resync_required());
    ledger.apply(&player_content());
    ledger.apply(&cursor_content());
    assert!(!ledger.resync_required());
}

#[test]
fn session_reset_clears_a_closing_window_immediately() {
    let (mut ledger, _request) = closing_with_pending(930);
    assert!(ledger.storage_generation().is_some());

    ledger.begin_session(42);

    assert_eq!(ledger.storage_generation(), None);
    assert!(ledger.pending_request_id().is_none());
    assert!(ledger.pending_packet().unwrap().is_none());
    assert!(!ledger.resync_required());
}

#[test]
fn authoritative_close_settles_a_closing_window_immediately() {
    let (mut ledger, request) = closing_with_pending(940);
    assert!(ledger.storage_generation().is_some());

    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        server_initiated: true,
    }));

    assert_eq!(ledger.storage_generation(), None);
    assert!(ledger.resync_required());
    ledger.apply(&response(request, StackResponseStatus::Accepted));
    assert_eq!(
        ledger.cursor_stack().map(|stack| stack.stack_network_id),
        None,
        "the superseded prediction can no longer reconcile"
    );
    ledger.apply(&player_content());
    ledger.apply(&cursor_content());
    assert!(!ledger.resync_required());
}

#[test]
fn replacing_the_window_clears_a_closing_state_immediately() {
    let (mut ledger, request) = closing_with_pending(950);
    let old_generation = ledger.storage_generation().unwrap();

    ledger.apply(&open(1, 0));
    ledger.apply(&content(1, 951, 27));

    let new_generation = ledger
        .storage_generation()
        .expect("the replacement window opened");
    assert_ne!(new_generation, old_generation);
    assert_eq!(ledger.storage_identity().unwrap().dynamic_id, Some(951));
    ledger.apply(&response(request, StackResponseStatus::Accepted));
    assert_eq!(
        ledger.cursor_stack().map(|stack| stack.stack_network_id),
        None,
        "the stale prediction cannot touch the replacement"
    );
    ledger.apply(&player_content());
    ledger.apply(&cursor_content());
    assert!(!ledger.resync_required());
    assert!(ledger.begin_storage_click(2).is_ok());
}

#[test]
fn server_close_with_held_cursor_requires_player_and_cursor_in_both_orders() {
    for player_first in [true, false] {
        let mut ledger = ready(27, 801);
        let request = ledger.begin_storage_click(2).unwrap();
        ledger.mark_transport_enqueued(10);
        ledger.apply(&response(request, StackResponseStatus::Accepted));
        assert!(ledger.cursor_stack().is_some());

        ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(1),
            window_type: 0,
            server_initiated: true,
        }));
        assert!(ledger.resync_required());

        if player_first {
            ledger.apply(&player_content());
            assert!(
                ledger.resync_required(),
                "cursor authority is still missing"
            );
            ledger.apply(&cursor_content());
        } else {
            ledger.apply(&cursor_content());
            assert!(
                ledger.resync_required(),
                "player authority is still missing"
            );
            ledger.apply(&player_content());
        }
        assert!(!ledger.resync_required());
    }
}

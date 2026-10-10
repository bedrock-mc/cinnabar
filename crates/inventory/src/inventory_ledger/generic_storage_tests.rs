use std::sync::Arc;

use crate::inventory_ledger::{
    INVENTORY_REQUEST_TIMEOUT_MILLIS, InventoryGestureError, InventoryPendingState,
    PlayerInventoryLedger,
};
use protocol::{
    ContainerCloseEvent, ContainerIdentity, ContainerOpenEvent, InventoryAuthority,
    InventoryContentEvent, InventoryEvent, InventorySlotEvent, ItemStackResponseEvent,
    NetworkItemStack, SlotIdentity, StackResponse, StackResponseContainer, StackResponseSlot,
    StackResponseStatus,
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

/// Native accepted answers write backing explicitly; success alone never
/// replays the local transfer. Both corrections belong to this take request.
fn accepted_storage_take(request_id: i32, dynamic_id: u32) -> InventoryEvent {
    let corrected_slot = |slot, count, item_stack_id| StackResponseSlot {
        slot,
        hotbar_slot: slot,
        count,
        item_stack_id,
        custom_name: Arc::from(""),
        filtered_custom_name: Arc::from(""),
        durability_correction: 0,
    };
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([
                StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
                        dynamic_id: Some(dynamic_id),
                    },
                    slots: Arc::from([corrected_slot(2, 0, 0)]),
                },
                StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(protocol::CONTAINER_NAME_CURSOR),
                        dynamic_id: None,
                    },
                    slots: Arc::from([corrected_slot(0, 3, 91)]),
                },
            ]),
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

/// Vanilla's cursor authority: the UI window naming the cursor container.
fn cursor_content() -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(124),
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
        assert!(ledger.pending_batch().unwrap().is_some());
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
    assert!(ledger.pending_batch().unwrap().is_some());
}

#[test]
fn storage_gestures_pipeline_through_the_shared_cursor() {
    let mut ledger = ready(27, 777);
    let take = ledger.begin_storage_click(2).unwrap();
    assert_eq!(
        ledger.pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
    assert_eq!(ledger.cursor_stack().unwrap().count, 3);
    assert!(ledger.storage_stack(2).is_none());
    let place = ledger.begin_storage_click(5).unwrap();
    assert_eq!(ledger.pending_request_count(), 2);
    assert_eq!(ledger.storage_stack(5).unwrap().count, 3);
    assert!(ledger.cursor_stack().is_none());

    ledger.mark_transport_enqueued(10);
    ledger.mark_transport_enqueued(10);
    ledger.apply(&response(take, StackResponseStatus::Rejected));
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(
        ledger.storage_stack(5).unwrap().stack_network_id,
        place,
        "the later sparse owner stays until its own server answer"
    );
    assert_eq!(ledger.storage_stack(2).unwrap().count, 3);
    ledger.apply(&response(place, StackResponseStatus::Rejected));
    assert_eq!(ledger.pending_request_count(), 0);
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
fn authoritative_replacement_survives_an_empty_success_without_replay_or_recovery() {
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
    assert!(!ledger.resync_required());
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(ledger.storage_stack(2), Some(&stack(6, 1, 92)));
    assert!(ledger.cursor_stack().is_none());

    ledger.apply(&content(1, 300, 27));
    assert!(!ledger.resync_required());
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
    ledger.apply(&accepted_storage_take(request, 300));
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
    assert!(ledger.pending_batch().unwrap().is_some());
    ledger.note_transport_pressure(10);
    ledger.note_transport_pressure(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.pending_batch().unwrap().is_some());
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        server_initiated: true,
    }));
    assert!(ledger.pending_batch().unwrap().is_none());

    // Structure-editor windows have no client screen.
    let mut unsupported = PlayerInventoryLedger::default();
    unsupported.apply(&open(9, 14));
    assert!(unsupported.pending_batch().unwrap().is_some());
    assert_eq!(unsupported.storage_slot_count(), None);
}

#[test]
fn timed_out_prediction_retires_after_every_touched_surface_refreshes() {
    let mut player_request = ready(27, 600);
    player_request.apply(&player_content_with_first(stack(8, 2, 44)));
    player_request.begin_click(0).unwrap();
    player_request.mark_transport_enqueued(10);
    player_request.apply(&content(1, 600, 27));
    assert!(
        !player_request.resync_required(),
        "content never cancels a prediction"
    );
    assert_eq!(player_request.pending_request_count(), 1);
    player_request.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(player_request.resync_required());
    player_request.apply(&content(1, 600, 27));
    assert!(
        player_request.resync_required(),
        "storage was never touched"
    );
    player_request.apply(&player_content());
    assert!(player_request.resync_required(), "cursor is still touched");
    player_request.apply(&cursor_content());
    assert!(!player_request.resync_required());
    assert_eq!(player_request.pending_request_count(), 0);

    let mut storage_request = ready(27, 601);
    storage_request.begin_storage_click(2).unwrap();
    storage_request.mark_transport_enqueued(10);
    storage_request.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    storage_request.apply(&player_content());
    assert!(storage_request.resync_required());
    storage_request.apply(&cursor_content());
    assert!(
        storage_request.resync_required(),
        "storage is still touched"
    );
    storage_request.apply(&content(1, 601, 27));
    assert!(!storage_request.resync_required());
    assert_eq!(storage_request.pending_request_count(), 0);
}

#[test]
fn foreign_storage_content_and_mismatched_slot_updates_are_fenced() {
    let mut ledger = ready(27, 700);
    ledger.apply(&content(2, 999, 27));
    ledger.apply(&content(1, 701, 27));
    assert_eq!(ledger.storage_identity().unwrap().dynamic_id, Some(700));
    assert_eq!(ledger.storage_stack(2).unwrap().stack_network_id, 91);
    assert!(ledger.pending_batch().unwrap().is_none());

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
    ledger.apply(&accepted_storage_take(request, 800));
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
        Some(request),
        "the visible sparse cursor carries its owning request id"
    );
    assert_eq!(ledger.pending_request_id(), Some(request));
    assert!(
        ledger.pending_batch().unwrap().is_some(),
        "the local ContainerClose still transmits"
    );

    // Every new gesture is blocked while the closing window settles.
    assert_eq!(
        ledger.begin_storage_click(3),
        Err(InventoryGestureError::ResyncRequired)
    );
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::ResyncRequired)
    );

    // A duplicate close gesture cannot restart or requeue the close.
    ledger.request_storage_close();
    assert_eq!(ledger.storage_generation(), Some(generation));
    assert_eq!(ledger.storage_identity().unwrap().dynamic_id, Some(900));
}

#[test]
fn accepted_response_reconciles_then_finishes_the_deferred_close() {
    let (mut ledger, request) = closing_with_pending(910);
    ledger.apply(&accepted_storage_take(request, 910));

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
fn timed_out_closing_state_settles_on_the_server_close() {
    let (mut ledger, request) = closing_with_pending(920);
    assert!(ledger.storage_generation().is_some());

    ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(
        ledger.storage_generation().is_some(),
        "a timeout never rolls back"
    );
    assert_eq!(ledger.pending_request_id(), Some(request));
    assert!(ledger.resync_required());

    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        server_initiated: false,
    }));
    assert_eq!(ledger.storage_generation(), None);
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
    assert!(ledger.pending_batch().unwrap().is_none());
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

/// A menu may close its window before rejecting the click without restating any cells.
#[test]
fn late_rejection_of_a_request_abandoned_by_a_server_close_lifts_its_recovery() {
    let (mut ledger, request) = closing_with_pending(960);
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        server_initiated: true,
    }));
    assert!(ledger.resync_required());

    ledger.apply(&response(request, StackResponseStatus::Rejected));
    assert!(!ledger.resync_required());
    assert_eq!(ledger.cursor_stack(), None);
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
        ledger.apply(&accepted_storage_take(request, 801));
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

/// A closed chest's late answer moves the item into the player inventory without ever
/// hiding or clearing the same slot of the chest opened after it.
#[test]
fn settling_chest_request_never_touches_the_next_chest() {
    let mut ledger = ready(27, 960);
    ledger.apply(&player_content());
    let request = ledger
        .begin_quick_move(crate::inventory_ledger::InventoryTarget::Storage(2))
        .unwrap();
    let destination = (0..36u8)
        .find(|slot| ledger.displayed_stack(*slot).is_some())
        .unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.request_storage_close();
    assert!(ledger.mark_transport_enqueued(11));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        server_initiated: false,
    }));

    let chest_b_slot = stack(7, 4, 77);
    let mut slots = vec![NetworkItemStack::default(); 27];
    slots[2] = chest_b_slot.clone();
    ledger.apply(&open(2, 0));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(2),
            slot_type: Some(7),
            dynamic_id: Some(961),
        },
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    }));
    assert_eq!(ledger.storage_stack(2), Some(&chest_b_slot));

    let corrected = |slot, count, item_stack_id| StackResponseSlot {
        slot,
        hotbar_slot: slot,
        count,
        item_stack_id,
        custom_name: Arc::from(""),
        filtered_custom_name: Arc::from(""),
        durability_correction: 0,
    };
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id: request,
            containers: Arc::from([
                StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
                        dynamic_id: Some(960),
                    },
                    slots: Arc::from([corrected(2, 0, 0)]),
                },
                StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                        dynamic_id: None,
                    },
                    slots: Arc::from([corrected(destination, 3, 91)]),
                },
            ]),
        }]),
    }));
    assert_eq!(ledger.storage_stack(2), Some(&chest_b_slot));
    let received = ledger
        .confirmed
        .get(crate::inventory_ledger::Cell::Inventory(destination))
        .unwrap();
    assert_eq!(
        (received.stack.count, received.stack.stack_network_id),
        (3, 91)
    );
    assert_eq!(ledger.pending_state(), None);
}

/// A cursor refresh retires the earlier abandonment before another window closes.
#[test]
fn late_rejection_ignores_refreshed_abandonment() {
    for reject_earlier_first in [false, true] {
        let (mut ledger, a) = closing_with_pending(960);
        let close = InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(1),
            window_type: 0,
            server_initiated: true,
        });
        ledger.apply(&close);
        ledger.apply(&cursor_content());
        assert!(!ledger.resync_required());
        ledger.apply(&open(1, 0));
        ledger.apply(&content(1, 961, 27));
        let b = ledger.begin_storage_click(2).unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        ledger.apply(&close);
        if reject_earlier_first {
            ledger.apply(&response(a, StackResponseStatus::Rejected));
            assert!(ledger.resync_required(), "B still needs an answer");
        }
        ledger.apply(&response(b, StackResponseStatus::Rejected));
        assert!(
            !ledger.resync_required(),
            "A was already refreshed before B"
        );
        ledger.apply(&open(1, 0));
        ledger.apply(&content(1, 962, 27));
        assert!(ledger.begin_storage_click(2).is_ok());
    }
}

/// A rejected menu click leaves the original stack's authoritative metadata intact.
#[test]
fn review_rejection_preserves_confirmed_response_overlay() {
    let mut ledger = ready(27, 970);
    ledger.apply(&player_content_with_first(stack(5, 3, 92)));
    let overlay = super::StackResponseOverlay {
        custom_name: Some(Arc::from("Menu item")),
        filtered_custom_name: Some(Arc::from("Menu item")),
        durability_correction: Some(4),
    };
    ledger.set_confirmed_overlay(super::Cell::Inventory(0), overlay.clone());
    let request = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        server_initiated: true,
    }));
    ledger.apply(&response(request, StackResponseStatus::Rejected));
    assert!(!ledger.resync_required());
    assert_eq!(ledger.slot_overlay(0), Some(&overlay));
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 3);
    ledger.apply(&open(1, 0));
    ledger.apply(&content(1, 971, 27));
    assert!(ledger.begin_click(0).is_ok());
    assert_eq!(
        ledger
            .view()
            .get(super::Cell::Cursor)
            .unwrap()
            .overlay
            .as_ref(),
        Some(&overlay)
    );
}

/// A full personal UI refresh settles abandoned cursor ambiguity.
#[test]
fn review_full_ui_snapshot_releases_cursor_recovery() {
    let (mut ledger, request) = closing_with_pending(972);
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: 0,
        server_initiated: true,
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
            slot_type: Some(0),
            dynamic_id: None,
        },
        slots: vec![NetworkItemStack::default(); protocol::UI_SLOT_COUNT].into(),
        storage_item: NetworkItemStack::default(),
    }));
    assert!(!ledger.resync_required());
    ledger.apply(&response(request, StackResponseStatus::Accepted));
    assert!(!ledger.resync_required());
}

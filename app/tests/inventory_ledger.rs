use std::sync::Arc;

use bedrock_client::ui_runtime::inventory_ledger::{
    INVENTORY_REQUEST_TIMEOUT_MILLIS, InventoryGestureError, InventoryPendingState,
    PlayerInventoryLedger,
};
use bedrock_client::ui_runtime::{UiRuntime, flush_inventory_send};
use protocol::{
    ContainerIdentity, InventoryAuthority, InventoryContentEvent, InventoryEvent,
    InventorySlotEvent, ItemStackResponseEvent, NetworkItemStack, SlotIdentity, StackResponse,
    StackResponseContainer, StackResponseSlot, StackResponseStatus,
};

fn stack(network_id: i32, count: u16, stack_network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        count,
        stack_network_id,
        block_runtime_id: 0,
        extra_data: Arc::from([]),
        nbt_digest: [0; 32],
    }
}

fn ready(
    first: Option<NetworkItemStack>,
    second: Option<NetworkItemStack>,
) -> PlayerInventoryLedger {
    let mut slots = vec![NetworkItemStack::default(); 36];
    if let Some(stack) = first {
        slots[0] = stack;
    }
    if let Some(stack) = second {
        slots[1] = stack;
    }
    let mut ledger = PlayerInventoryLedger::default();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    }));
    ledger
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

fn response_with_slot(request_id: i32, slot: u8, count: u8, stack_id: i32) -> InventoryEvent {
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([StackResponseContainer {
                container: ContainerIdentity {
                    window_id: None,
                    slot_type: Some(12),
                    dynamic_id: None,
                },
                slots: Arc::from([StackResponseSlot {
                    slot,
                    hotbar_slot: slot,
                    count,
                    item_stack_id: stack_id,
                    custom_name: Arc::from(""),
                    filtered_custom_name: Arc::from(""),
                    durability_correction: 0,
                }]),
            }]),
        }]),
    })
}

fn slot_update(slot: u16, stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot,
        },
        stack,
        storage_item: None,
    })
}

fn cursor_content(stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(-1),
            slot_type: Some(59),
            dynamic_id: None,
        },
        slots: Arc::from([stack]),
        storage_item: NetworkItemStack::default(),
    })
}

fn cursor_slot_with_window_zero(stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(0),
                slot_type: Some(59),
                dynamic_id: None,
            },
            slot: 0,
        },
        stack,
        storage_item: None,
    })
}

fn complete_player_content(
    first: Option<NetworkItemStack>,
    second: Option<NetworkItemStack>,
) -> InventoryEvent {
    let mut slots = vec![NetworkItemStack::default(); 36];
    if let Some(stack) = first {
        slots[0] = stack;
    }
    if let Some(stack) = second {
        slots[1] = stack;
    }
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    })
}

#[derive(Clone, Copy)]
enum CursorGesture {
    Place,
    Swap,
}

impl CursorGesture {
    fn setup(
        self,
    ) -> (
        PlayerInventoryLedger,
        u8,
        NetworkItemStack,
        Option<NetworkItemStack>,
    ) {
        let held = stack(5, 2, 44);
        let occupied = matches!(self, Self::Swap).then(|| stack(6, 3, 55));
        let mut ledger = ready(Some(held.clone()), occupied.clone());
        let take = ledger.begin_click(0).unwrap();
        ledger.mark_transport_enqueued(0);
        ledger.apply(&response(take, StackResponseStatus::Accepted));
        let target = match self {
            Self::Place => 0,
            Self::Swap => 1,
        };
        (ledger, target, held, occupied)
    }
}

#[test]
fn take_predicts_cursor_and_rejects_queued_gestures() {
    let original = stack(5, 12, 44);
    let mut ledger = ready(Some(original.clone()), None);
    let request = ledger.begin_click(0).expect("take request");
    assert_eq!(
        ledger.pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
    assert!(ledger.displayed_stack(0).is_none());
    assert_eq!(ledger.cursor_stack(), Some(&original));
    assert!(ledger.displayed_stack(u8::MAX).is_none());
    assert_eq!(ledger.begin_click(1), Err(InventoryGestureError::Busy));

    assert!(ledger.mark_transport_enqueued(100));
    ledger.apply(&response(request, StackResponseStatus::Accepted));
    assert_eq!(ledger.pending_state(), None);
    assert_eq!(ledger.cursor_stack(), Some(&original));
}

#[test]
fn normal_requests_use_the_vanilla_negative_odd_id_sequence() {
    let original = stack(5, 12, 44);
    let mut ledger = ready(Some(original), None);

    let first = ledger.begin_click(0).expect("first request");
    assert_eq!(first, -3);
    ledger.mark_transport_enqueued(0);
    ledger.apply(&response(first, StackResponseStatus::Accepted));

    let second = ledger.begin_click(0).expect("second request");
    assert_eq!(second, -5);
}

#[test]
fn rejection_rolls_back_prediction_without_discarding_known_state() {
    let original = stack(5, 12, 44);
    let mut ledger = ready(Some(original.clone()), None);
    let request = ledger.begin_click(0).expect("take request");
    ledger.mark_transport_enqueued(100);
    ledger.apply(&response(request, StackResponseStatus::Rejected));
    assert_eq!(ledger.displayed_stack(0), Some(&original));
    assert!(ledger.cursor_stack().is_none());
    assert!(!ledger.resync_required());
    assert!(ledger.begin_click(0).is_ok());
}

#[test]
fn admitted_request_timeout_fails_closed_without_unsafe_retransmission() {
    let mut ledger = ready(Some(stack(5, 1, 44)), None);
    let request = ledger.begin_click(0).expect("take request");
    ledger.mark_transport_enqueued(10);
    assert!(!ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert!(ledger.pending_request_id().is_none());
    assert!(ledger.resync_required());
    assert_eq!(request, -3);
}

#[test]
fn empty_place_and_occupied_swap_preserve_sparse_state() {
    let first = stack(5, 1, 44);
    let second = stack(6, 2, 45);
    let mut ledger = ready(Some(first.clone()), Some(second.clone()));
    let take = ledger.begin_click(0).unwrap();
    ledger.mark_transport_enqueued(0);
    ledger.apply(&response(take, StackResponseStatus::Accepted));

    let swap = ledger.begin_click(1).unwrap();
    assert_eq!(ledger.cursor_stack(), Some(&second));
    assert_eq!(ledger.displayed_stack(1), Some(&first));
    ledger.mark_transport_enqueued(0);
    ledger.apply(&response(swap, StackResponseStatus::Accepted));

    let place = ledger.begin_click(0).unwrap();
    assert_eq!(ledger.cursor_stack(), None);
    assert_eq!(ledger.displayed_stack(0), Some(&second));
    assert_eq!(ledger.pending_request_id(), Some(place));
}

#[test]
fn bounded_transport_pressure_does_not_consume_or_duplicate_the_request() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    let content = InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(
            (0..36)
                .map(|index| {
                    if index == 0 {
                        stack(5, 1, 44)
                    } else {
                        NetworkItemStack::default()
                    }
                })
                .collect::<Vec<_>>(),
        ),
        storage_item: NetworkItemStack::default(),
    });
    runtime.inventory_ledger_mut().apply(&content);
    let request = runtime.inventory_ledger_mut().begin_click(0).unwrap();

    assert_eq!(
        flush_inventory_send(&mut runtime, 10, |_| Err("full")),
        Err("full")
    );
    assert_eq!(
        runtime.inventory_ledger().pending_request_id(),
        Some(request)
    );
    assert_eq!(
        runtime.inventory_ledger().pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
    assert_eq!(
        flush_inventory_send(
            &mut runtime,
            10 + INVENTORY_REQUEST_TIMEOUT_MILLIS,
            |_| Err("full")
        ),
        Err("full")
    );
    assert!(!runtime.inventory_ledger().resync_required());

    runtime.inventory_ledger_mut().apply(&content);
    let retry = runtime.inventory_ledger_mut().begin_click(0).unwrap();
    assert_eq!(
        flush_inventory_send(&mut runtime, 11, |_| Ok::<_, &str>(())),
        Ok(true)
    );
    assert_eq!(
        flush_inventory_send(&mut runtime, 12, |_| Ok::<_, &str>(())),
        Ok(false)
    );
    assert_ne!(request, retry);
}

#[test]
fn short_content_is_partial_and_never_releases_the_resync_gate() {
    let first = stack(5, 1, 44);
    let mut fresh = PlayerInventoryLedger::default();
    fresh.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    fresh.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from([first.clone()]),
        storage_item: NetworkItemStack::default(),
    }));
    assert_eq!(fresh.displayed_stack(0), Some(&first));
    assert_eq!(
        fresh.begin_click(1),
        Err(InventoryGestureError::UnknownSlot(1))
    );

    let pending = fresh.begin_click(0).unwrap();
    fresh.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from([stack(6, 2, 55)]),
        storage_item: NetworkItemStack::default(),
    }));
    assert_eq!(fresh.pending_request_id(), Some(pending));
    fresh.mark_transport_enqueued(0);
    fresh.apply(&response(pending, StackResponseStatus::Accepted));
    assert!(fresh.resync_required());

    let mut recovering = ready(Some(first.clone()), None);
    recovering.begin_click(0).unwrap();
    recovering.mark_transport_enqueued(0);
    recovering.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
    recovering.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from([first]),
        storage_item: NetworkItemStack::default(),
    }));
    assert!(recovering.resync_required());
    assert_eq!(
        recovering.begin_click(0),
        Err(InventoryGestureError::ResyncRequired)
    );
}

#[test]
fn complete_sparse_content_recovers_the_player_side_of_ambiguous_paths() {
    for fail in ["timeout", "closed"] {
        let mut ledger = ready(Some(stack(5, 1, 44)), None);
        ledger.begin_click(0).unwrap();
        ledger.mark_transport_enqueued(0);
        if fail == "timeout" {
            ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
        } else {
            ledger.transport_closed();
        }
        assert!(ledger.resync_required());
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: Arc::from(
                (0..36)
                    .map(|_| NetworkItemStack::default())
                    .collect::<Vec<_>>(),
            ),
            storage_item: NetworkItemStack::default(),
        }));
        assert!(ledger.resync_required());
        ledger.apply(&cursor_content(NetworkItemStack::default()));
        assert!(!ledger.resync_required());
    }
}

#[test]
fn newer_touched_slot_authority_is_never_overwritten_by_accepted_response() {
    for with_correction in [false, true] {
        let original = stack(5, 1, 44);
        let newer = stack(6, 3, 90);
        let mut ledger = ready(Some(original), None);
        let request = ledger.begin_click(0).unwrap();
        assert_eq!(request, -3);
        ledger.mark_transport_enqueued(0);
        ledger.apply(&slot_update(0, newer.clone()));
        let accepted = if with_correction {
            response_with_slot(request, 0, 1, 44)
        } else {
            response(request, StackResponseStatus::Accepted)
        };
        ledger.apply(&accepted);
        assert_eq!(ledger.displayed_stack(0), Some(&newer));
        assert!(ledger.cursor_stack().is_none());
        assert!(ledger.resync_required());
    }
}

#[test]
fn unrelated_or_later_slot_authority_merges_in_fifo_order() {
    let first = stack(5, 1, 44);
    let unrelated = stack(7, 2, 70);
    let mut ledger = ready(Some(first.clone()), None);
    let request = ledger.begin_click(0).unwrap();
    ledger.mark_transport_enqueued(0);
    ledger.apply(&slot_update(1, unrelated.clone()));
    ledger.apply(&response(request, StackResponseStatus::Accepted));
    assert_eq!(ledger.displayed_stack(1), Some(&unrelated));
    assert_eq!(ledger.cursor_stack(), Some(&first));
    assert!(!ledger.resync_required());

    let later = stack(8, 4, 80);
    ledger.apply(&slot_update(0, later.clone()));
    assert_eq!(ledger.displayed_stack(0), Some(&later));
}

#[test]
fn accepted_correction_without_a_new_stack_id_preserves_the_predicted_id() {
    let original = stack(5, 1, 44);
    let mut ledger = ready(Some(original.clone()), None);
    let take = ledger.begin_click(0).unwrap();
    ledger.mark_transport_enqueued(0);
    ledger.apply(&response(take, StackResponseStatus::Accepted));

    let place = ledger.begin_click(1).unwrap();
    ledger.mark_transport_enqueued(0);
    ledger.apply(&response_with_slot(place, 1, 1, -1));

    assert_eq!(ledger.displayed_stack(1), Some(&original));
}

#[test]
fn rejection_duplicates_and_late_responses_never_erase_newer_authority() {
    let newer = stack(6, 3, 90);
    let mut ledger = ready(Some(stack(5, 1, 44)), None);
    let request = ledger.begin_click(0).unwrap();
    ledger.mark_transport_enqueued(0);
    ledger.apply(&slot_update(0, newer.clone()));
    ledger.apply(&response(request, StackResponseStatus::Rejected));
    ledger.apply(&response(request, StackResponseStatus::Accepted));
    ledger.apply(&response(request, StackResponseStatus::Accepted));
    assert_eq!(ledger.displayed_stack(0), Some(&newer));
    assert!(!ledger.resync_required());
}

#[test]
fn place_and_swap_known_not_applied_paths_restore_the_base_cursor() {
    for gesture in [CursorGesture::Place, CursorGesture::Swap] {
        for status in [
            StackResponseStatus::Rejected,
            StackResponseStatus::Unknown(9),
        ] {
            let (mut ledger, target, held, occupied) = gesture.setup();
            let request = ledger.begin_click(target).unwrap();
            ledger.mark_transport_enqueued(10);
            ledger.apply(&response(request, status));
            assert_eq!(ledger.cursor_stack(), Some(&held));
            assert_eq!(ledger.displayed_stack(target), occupied.as_ref());
            assert!(!ledger.resync_required());

            ledger.apply(&response(request, StackResponseStatus::Accepted));
            assert_eq!(ledger.cursor_stack(), Some(&held));
            assert_eq!(ledger.displayed_stack(target), occupied.as_ref());
        }

        let (mut full, target, held, occupied) = gesture.setup();
        let request = full.begin_click(target).unwrap();
        full.note_transport_pressure(10);
        full.note_transport_pressure(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        assert_eq!(full.pending_request_id(), None);
        assert_eq!(full.cursor_stack(), Some(&held));
        assert_eq!(full.displayed_stack(target), occupied.as_ref());
        assert!(!full.resync_required());
        full.apply(&response(request, StackResponseStatus::Accepted));
        assert_eq!(full.cursor_stack(), Some(&held));

        let (mut closed, target, held, occupied) = gesture.setup();
        let request = closed.begin_click(target).unwrap();
        closed.transport_closed();
        assert_eq!(closed.pending_request_id(), None);
        assert_eq!(closed.cursor_stack(), Some(&held));
        assert_eq!(closed.displayed_stack(target), occupied.as_ref());
        assert!(!closed.resync_required());
        closed.apply(&response(request, StackResponseStatus::Accepted));
        assert_eq!(closed.cursor_stack(), Some(&held));
    }
}

#[test]
fn place_and_swap_admitted_timeout_wait_for_player_and_cursor_authority() {
    for (index, gesture) in [CursorGesture::Place, CursorGesture::Swap]
        .into_iter()
        .enumerate()
    {
        let (mut ledger, target, held, occupied) = gesture.setup();
        let request = ledger.begin_click(target).unwrap();
        ledger.mark_transport_enqueued(10);
        ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        assert_eq!(ledger.pending_request_id(), None);
        assert_eq!(ledger.cursor_stack(), Some(&held));
        assert_eq!(ledger.displayed_stack(target), occupied.as_ref());
        assert!(ledger.resync_required());

        ledger.apply(&response(request, StackResponseStatus::Accepted));
        assert_eq!(ledger.cursor_stack(), Some(&held));
        assert_eq!(ledger.displayed_stack(target), occupied.as_ref());

        let player = match gesture {
            CursorGesture::Place => complete_player_content(None, None),
            CursorGesture::Swap => complete_player_content(None, occupied.clone()),
        };
        if index == 0 {
            ledger.apply(&player);
            assert!(ledger.resync_required());
            ledger.apply(&cursor_content(held.clone()));
        } else {
            ledger.apply(&cursor_content(held.clone()));
            assert!(ledger.resync_required());
            ledger.apply(&player);
        }
        assert!(!ledger.resync_required());
        assert_eq!(ledger.cursor_stack(), Some(&held));
    }
}

#[test]
fn window_zero_cursor_slot_does_not_shadow_player_slot_recovery() {
    for cursor_first in [false, true] {
        let original = stack(5, 1, 44);
        let player_authority = stack(6, 3, 55);
        let cursor_authority = stack(7, 2, 66);
        let mut ledger = ready(Some(original.clone()), None);
        ledger.begin_click(0).unwrap();
        ledger.mark_transport_enqueued(10);
        ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        assert!(ledger.resync_required());

        let player = complete_player_content(Some(player_authority.clone()), None);
        let cursor = cursor_slot_with_window_zero(cursor_authority.clone());
        if cursor_first {
            ledger.apply(&cursor);
            assert_eq!(ledger.displayed_stack(0), Some(&original));
            assert!(ledger.resync_required());
            ledger.apply(&player);
        } else {
            ledger.apply(&player);
            assert_eq!(ledger.displayed_stack(0), Some(&player_authority));
            assert!(ledger.resync_required());
            ledger.apply(&cursor);
        }

        assert!(!ledger.resync_required());
        assert_eq!(ledger.displayed_stack(0), Some(&player_authority));
        assert_eq!(ledger.cursor_stack(), Some(&cursor_authority));
    }
}

use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CURSOR, ContainerCloseEvent,
    ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent,
    ItemStackResponseEvent, NetworkItemStack, StackRequestAction, StackRequestContainer,
    StackResponse, StackResponseContainer, StackResponseSlot, StackResponseStatus,
};
use sha2::{Digest, Sha256};

use crate::InventorySession;

use super::*;

fn stack(stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        metadata: 0,
        stack_network_id,
        count,
        nbt_digest: Sha256::digest([]).into(),
        block_runtime_id: 0,
        extra_data: Arc::from([]),
    }
}

fn ledger_with_slot_zero() -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = stack(9, 32);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger
}

/// A refused batch retains every request; one successful retry admits them all exactly once.
#[test]
fn inventory_request_batch_retries_atomically() {
    let mut runtime = InventorySession::new(1);
    *runtime.ledger_mut() = ledger_with_slot_zero();
    let ledger = runtime.ledger_mut();
    assert_eq!(ledger.begin_world_drop(0, Some(1)).unwrap(), -3);
    assert_eq!(ledger.begin_world_drop(0, Some(1)).unwrap(), -5);
    let mut attempts = Vec::new();
    assert_eq!(
        runtime.flush_inventory_send(10, |packet| {
            attempts.push(
                protocol::encode(&packet, &protocol::BedrockSession { shield_item_id: 0 }).unwrap(),
            );
            Err("full")
        }),
        Err("full")
    );
    assert!(
        runtime
            .ledger()
            .queue
            .iter()
            .all(|request| request.state == InventoryPendingState::AwaitingTransport)
    );
    assert!(
        runtime
            .flush_inventory_send(20, |packet| {
                attempts.push(
                    protocol::encode(&packet, &protocol::BedrockSession { shield_item_id: 0 })
                        .unwrap(),
                );
                Ok::<_, &str>(())
            })
            .unwrap()
    );
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0], attempts[1]);
    let ledger = runtime.ledger();
    assert!(
        ledger
            .queue
            .iter()
            .all(|request| request.state == InventoryPendingState::AwaitingResponse)
    );
    assert!(
        !runtime
            .flush_inventory_send::<&str>(21, |_| panic!("batch was already sent"))
            .unwrap()
    );
}

fn personal_open(window_id: i32) -> ContainerOpenEvent {
    personal_open_with_actor(window_id, -1)
}

fn personal_open_with_actor(window_id: i32, runtime_entity_id: i64) -> ContainerOpenEvent {
    ContainerOpenEvent {
        container: ContainerIdentity::window(window_id),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id,
    }
}

fn acknowledge_personal_open(ledger: &mut PlayerInventoryLedger, window_id: i32) {
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Open(personal_open(window_id)));
}

fn correction(container: u8, slot: u8, count: u8, stack_network_id: i32) -> StackResponseContainer {
    StackResponseContainer {
        container: ContainerIdentity {
            window_id: None,
            slot_type: Some(container),
            dynamic_id: None,
        },
        slots: Arc::from([StackResponseSlot {
            slot,
            hotbar_slot: slot,
            count,
            item_stack_id: stack_network_id,
            custom_name: Arc::from(""),
            filtered_custom_name: Arc::from(""),
            durability_correction: 0,
        }]),
    }
}

fn accept_cursor_move(ledger: &mut PlayerInventoryLedger, request_id: i32, returning: bool) {
    let (player, cursor) = if returning { (32, 0) } else { (0, 32) };
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([
                correction(
                    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                    0,
                    player,
                    if player == 0 { -1 } else { 9 },
                ),
                correction(
                    CONTAINER_NAME_CURSOR,
                    0,
                    cursor,
                    if cursor == 0 { -1 } else { 9 },
                ),
            ]),
        }]),
    }));
}

#[test]
fn personal_gesture_waits_for_open_admission_and_uses_empty_stack_id_zero() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.pending_batch().unwrap().is_some());
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::PersonalInventoryUnavailable)
    );

    assert!(ledger.mark_transport_enqueued(10));
    assert_eq!(ledger.begin_click(0).unwrap(), -3);
    let pending = ledger.newest_request().unwrap();
    let StackRequestAction::Take {
        amount,
        source,
        destination,
    } = pending.actions[0]
    else {
        panic!("expected Take action");
    };
    assert_eq!(amount, 32);
    assert_eq!(source.stack_network_id, 9);
    assert_eq!(destination.stack_network_id, 0);
    assert_eq!(source.container, StackRequestContainer::PlayerInventory);
    assert_eq!(destination.container, StackRequestContainer::Cursor);
}

#[test]
fn open_queue_pressure_retains_one_control_and_admits_it_once() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    ledger.note_transport_pressure(10);
    ledger.note_transport_pressure(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.pending_batch().unwrap().is_some());
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::PersonalInventoryUnavailable)
    );

    assert!(ledger.mark_transport_enqueued(20));
    assert!(!ledger.mark_transport_enqueued(20));
    assert_eq!(ledger.begin_click(0).unwrap(), -3);
}

#[test]
fn failed_transport_retries_each_control_and_mutation_without_duplicate_admission() {
    let mut runtime = InventorySession::new(1);
    runtime.publish_inventory_authority(InventoryAuthority::Server);
    runtime.publish_local_runtime_id(1, 42).unwrap();
    runtime
        .ledger_mut()
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: Arc::from(
                (0..PLAYER_INVENTORY_SLOT_COUNT)
                    .map(|index| {
                        if index == 0 {
                            stack(9, 32)
                        } else {
                            NetworkItemStack::empty()
                        }
                    })
                    .collect::<Vec<_>>(),
            ),
            storage_item: NetworkItemStack::empty(),
        }));
    assert!(runtime.ledger_mut().request_personal_open(42));

    let mut open_attempts = 0;
    for now_millis in [10, 20] {
        assert_eq!(
            runtime.flush_inventory_send(now_millis, |_| {
                open_attempts += 1;
                Err("full")
            }),
            Err("full")
        );
    }
    assert_eq!(open_attempts, 2);
    assert!(
        runtime
            .flush_inventory_send(30, |_| Ok::<_, &str>(()))
            .unwrap()
    );
    assert!(
        !runtime
            .flush_inventory_send(31, |_| Ok::<_, &str>(()))
            .unwrap()
    );

    assert_eq!(runtime.ledger_mut().begin_click(0).unwrap(), -3);
    let mut mutation_attempts = 0;
    for now_millis in [40, 50] {
        assert_eq!(
            runtime.flush_inventory_send(now_millis, |_| {
                mutation_attempts += 1;
                Err("full")
            }),
            Err("full")
        );
    }
    assert_eq!(mutation_attempts, 2);
    assert!(
        runtime
            .flush_inventory_send(60, |_| Ok::<_, &str>(()))
            .unwrap()
    );
    assert!(
        !runtime
            .flush_inventory_send(61, |_| Ok::<_, &str>(()))
            .unwrap()
    );
    assert_eq!(
        runtime.ledger().pending_state(),
        Some(InventoryPendingState::AwaitingResponse)
    );
    runtime.inventory_transport_closed();
    assert!(!runtime.ledger().personal_inventory_desired_open());
    assert_eq!(runtime.ledger().pending_state(), None);
    assert!(runtime.ledger().resync_required());
}

#[test]
fn missing_open_and_close_acknowledgements_expire_without_reopening_late() {
    let mut opening = ledger_with_slot_zero();
    assert!(opening.request_personal_open(42));
    assert!(opening.mark_transport_enqueued(10));
    assert!(opening.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert!(!opening.personal_inventory_desired_open());
    assert!(
        !opening.request_personal_open(42),
        "an uncorrelated late acknowledgement makes this session unavailable"
    );
    opening.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(!opening.personal_inventory_desired_open());
    assert_eq!(opening.pending_closes.front().unwrap().window_id, 2);
    assert!(!opening.request_personal_open(42));
    opening.begin_session(2);
    opening.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    assert!(opening.request_personal_open(42));

    let mut closing = ledger_with_slot_zero();
    acknowledge_personal_open(&mut closing, 2);
    closing.request_personal_close();
    assert!(closing.mark_transport_enqueued(20));
    assert!(closing.poll_timeout(20 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert!(closing.personal.is_none());
    assert!(!closing.personal_inventory_desired_open());
    assert!(!closing.request_personal_open(42));
}

#[test]
fn personal_ack_retains_dynamic_window_identity_for_exact_close() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    assert!(ledger.personal_inventory_desired_open());
    assert_eq!(ledger.storage_generation(), None);

    ledger.request_personal_close();
    let close = ledger
        .pending_closes
        .front()
        .copied()
        .expect("personal close");
    assert_eq!(close.window_id, 2);
    assert_eq!(close.window_type, PERSONAL_INVENTORY_WINDOW_TYPE);
    assert!(matches!(close.owner, PendingCloseOwner::Personal(_)));
    assert!(!ledger.personal_inventory_desired_open());

    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(3),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: true,
    }));
    assert!(ledger.personal.is_some(), "mismatched close stays isolated");
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: true,
    }));
    assert!(ledger.personal.is_none());
}

#[test]
fn client_ack_only_completes_an_admitted_personal_close_without_payload_correlation() {
    let close = |window_id, window_type, server_initiated| {
        InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(window_id),
            window_type,
            server_initiated,
        })
    };
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);

    ledger.apply(&close(2, NO_CONTAINER_WINDOW_TYPE, false));
    assert!(ledger.personal_inventory_desired_open());

    ledger.request_personal_close();
    ledger.apply(&close(2, NO_CONTAINER_WINDOW_TYPE, false));
    ledger.apply(&close(2, PERSONAL_INVENTORY_WINDOW_TYPE, false));
    assert!(ledger.personal.is_some(), "a queued close is not admitted");
    assert!(
        ledger.pending_batch().unwrap().is_some(),
        "even an exact-payload stale response cannot consume the unsent close"
    );

    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&close(2, NO_CONTAINER_WINDOW_TYPE, true));
    assert!(
        ledger.personal.is_some(),
        "unmatched close shapes stay isolated"
    );

    ledger.apply(&close(3, GENERIC_STORAGE_WINDOW_TYPE, false));
    assert!(ledger.personal.is_none());
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(30));

    ledger.apply(&close(2, NO_CONTAINER_WINDOW_TYPE, false));
    assert!(ledger.personal_inventory_desired_open());
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    ledger.apply(&close(2, NO_CONTAINER_WINDOW_TYPE, false));
    assert!(ledger.personal_inventory_desired_open());

    ledger.apply(&close(2, PERSONAL_INVENTORY_WINDOW_TYPE, true));
    assert!(
        ledger.personal.is_none(),
        "exact typed server close remains valid"
    );
}

#[test]
fn personal_window_zero_and_non_sentinel_actor_complete_the_same_lifecycle() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Open(personal_open_with_actor(0, 42)));

    assert!(ledger.personal_inventory_desired_open());
    assert_eq!(ledger.begin_click(0).unwrap(), -3);
    ledger.request_personal_close();
    assert_eq!(
        ledger.pending_state(),
        Some(InventoryPendingState::AwaitingTransport),
        "the close retains the take and its dependent cursor return"
    );
    let close = ledger.pending_closes.front().copied().unwrap();
    assert_eq!(
        (close.window_id, close.window_type),
        (0, PERSONAL_INVENTORY_WINDOW_TYPE)
    );
    assert!(matches!(close.owner, PendingCloseOwner::Personal(_)));
    assert!(ledger.mark_transport_enqueued(20));
    accept_cursor_move(&mut ledger, -3, false);
    accept_cursor_move(&mut ledger, -5, true);
    assert!(ledger.mark_transport_enqueued(21));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(0),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: true,
    }));
    assert!(ledger.personal.is_none());
}

#[test]
fn cleanup_then_owned_storage_close_preserves_required_fifo_order() {
    let mut ledger = ledger_with_slot_zero();
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(1),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    ledger.request_storage_close();

    assert_eq!(
        ledger
            .pending_closes
            .iter()
            .map(|close| (close.window_id, close.window_type, close.owner))
            .collect::<Vec<_>>(),
        vec![
            (
                2,
                PERSONAL_INVENTORY_WINDOW_TYPE,
                PendingCloseOwner::Cleanup
            ),
            (1, GENERIC_STORAGE_WINDOW_TYPE, PendingCloseOwner::Storage),
        ]
    );
    ledger.note_transport_pressure(10);
    ledger.note_transport_pressure(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert_eq!(ledger.pending_closes.len(), 2);
    assert!(ledger.mark_transport_enqueued(20));
    assert_eq!(ledger.pending_closes.front().unwrap().window_id, 1);
    assert!(ledger.mark_transport_enqueued(21));
    assert!(ledger.pending_closes.is_empty());
}

#[test]
fn late_open_cleanup_is_deduplicated_bounded_and_never_evicts_personal_close() {
    let mut late = ledger_with_slot_zero();
    for window_id in [2, 3, 3] {
        late.apply(&InventoryEvent::Open(personal_open(window_id)));
    }
    assert_eq!(
        late.pending_closes
            .iter()
            .map(|close| close.window_id)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );

    let mut personal = ledger_with_slot_zero();
    acknowledge_personal_open(&mut personal, 2);
    for offset in 0..=MAX_PENDING_CLOSES {
        personal.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(10 + i32::try_from(offset).unwrap()),
            window_type: 5,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
    }
    assert_eq!(personal.pending_closes.len(), MAX_PENDING_CLOSES);
    assert_eq!(personal.pending_closes.back().unwrap().window_id, 18);

    personal.request_personal_close();
    assert_eq!(personal.pending_closes.len(), MAX_PENDING_CLOSES);
    assert!(personal.pending_closes.iter().any(|close| {
        close.window_id == 2 && matches!(close.owner, PendingCloseOwner::Personal(_))
    }));
    personal.begin_session(2);
    assert!(personal.pending_closes.is_empty());
}

#[test]
fn superseded_storage_closes_evict_oldest_and_retain_latest_window() {
    let mut ledger = ledger_with_slot_zero();
    for window_id in 1..=i32::try_from(MAX_PENDING_CLOSES + 2).unwrap() {
        ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(window_id),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: Some(window_id),
                slot_type: Some(GENERIC_STORAGE_SLOT_TYPE),
                dynamic_id: Some(u32::try_from(window_id).unwrap()),
            },
            slots: Arc::from([NetworkItemStack::empty()]),
            storage_item: NetworkItemStack::empty(),
        }));
    }

    assert_eq!(ledger.pending_closes.len(), MAX_PENDING_CLOSES);
    assert_eq!(ledger.pending_closes.front().unwrap().window_id, 3);
    assert_eq!(ledger.pending_closes.back().unwrap().window_id, 10);
    assert!(
        ledger
            .pending_closes
            .iter()
            .all(|close| close.owner == PendingCloseOwner::Storage)
    );
}

#[test]
fn local_close_before_ack_never_reopens_and_closes_the_late_dynamic_window() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.request_personal_close();
    assert!(!ledger.personal_inventory_desired_open());

    ledger.apply(&InventoryEvent::Open(personal_open(7)));

    assert!(!ledger.personal_inventory_desired_open());
    let close = ledger
        .pending_closes
        .front()
        .copied()
        .expect("late acknowledgement close");
    assert_eq!((close.window_id, close.window_type), (7, -1));
}

#[test]
fn unexpected_personal_open_and_close_do_not_steal_storage_authority() {
    let mut ledger = ledger_with_slot_zero();
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(4),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    let storage_generation = ledger.storage_generation();
    assert!(!ledger.request_personal_open(42));

    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert_eq!(ledger.storage_generation(), storage_generation);
    assert!(ledger.personal.is_none());
    assert_eq!(ledger.skipped_unknown_containers(), 1);

    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: true,
    }));
    assert_eq!(ledger.storage_generation(), storage_generation);
}

#[test]
fn storage_sized_content_tracing_is_session_bounded_and_does_not_admit() {
    let mut ledger = ledger_with_slot_zero();
    let rejected = InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(4),
            slot_type: Some(211),
            dynamic_id: Some(91),
        },
        slots: Arc::from(vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT]),
        storage_item: NetworkItemStack::empty(),
    });

    ledger.apply(&rejected);
    assert_eq!(
        ledger.storage_content_traces_remaining,
        MAX_STORAGE_CONTENT_TRACES - 1,
        "content-before-Open consumes one bounded diagnostic record"
    );
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(4),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    let generation = ledger.storage_generation().unwrap();
    let wrong_window = InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(9),
            slot_type: Some(GENERIC_STORAGE_SLOT_TYPE),
            dynamic_id: Some(91),
        },
        slots: Arc::from(vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT]),
        storage_item: NetworkItemStack::empty(),
    });

    ledger.apply(&wrong_window);
    ledger.apply(&rejected);
    assert_eq!(ledger.authority, Some(InventoryAuthority::Server));
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(9)
    );
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.storage_generation(), Some(generation));
    assert_eq!(ledger.storage_slot_count(), None);
    assert_eq!(ledger.storage_identity(), None);
    assert!(ledger.pending_closes.is_empty());
    assert_eq!(ledger.skipped_unknown_containers(), 2);
    assert_eq!(
        ledger.storage_content_traces_remaining,
        MAX_STORAGE_CONTENT_TRACES - 3,
        "canonical wrong-window and unrouted identities are both observable"
    );

    for _ in 0..MAX_STORAGE_CONTENT_TRACES {
        ledger.apply(&rejected);
    }
    assert_eq!(ledger.storage_generation(), Some(generation));
    assert_eq!(ledger.storage_slot_count(), None);
    assert_eq!(ledger.storage_identity(), None);
    assert!(ledger.pending_closes.is_empty());
    assert_eq!(
        ledger.skipped_unknown_containers(),
        2 + u64::from(MAX_STORAGE_CONTENT_TRACES)
    );
    assert_eq!(ledger.storage_content_traces_remaining, 0);

    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(5),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [2, 64, 2],
        runtime_entity_id: -1,
    }));
    assert_eq!(
        ledger.storage_content_traces_remaining, 0,
        "opening another window cannot reset the session-wide bound"
    );
}

#[test]
fn accepted_personal_response_reconciles_player_and_cursor_cells() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    let request_id = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([
                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, -1),
                correction(CONTAINER_NAME_CURSOR, 0, 32, 9),
            ]),
        }]),
    }));

    assert_eq!(ledger.pending_state(), None);
    assert_eq!(ledger.displayed_stack(0), None);
    assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(32));

    assert_eq!(ledger.begin_click(9).unwrap(), -5);
    let pending = ledger.newest_request().unwrap();
    let StackRequestAction::Place {
        amount,
        source,
        destination,
    } = pending.actions[0]
    else {
        panic!("expected Place action");
    };
    assert_eq!(amount, 32);
    assert_eq!(source.stack_network_id, 9);
    assert_eq!(destination.stack_network_id, 0);
    assert_eq!(source.container, StackRequestContainer::Cursor);
    assert_eq!(
        destination.container,
        StackRequestContainer::PlayerInventory
    );
}

#[test]
fn admitted_local_close_returns_confirmed_cursor_with_its_overlay() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    let request_id = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([
                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, -1),
                StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(CONTAINER_NAME_CURSOR),
                        dynamic_id: None,
                    },
                    slots: Arc::from([StackResponseSlot {
                        slot: 0,
                        hotbar_slot: 0,
                        count: 32,
                        item_stack_id: 9,
                        custom_name: Arc::from("Retained stack"),
                        filtered_custom_name: Arc::from(""),
                        durability_correction: 4,
                    }]),
                },
            ]),
        }]),
    }));
    let confirmed = ledger.cursor_stack().cloned().unwrap();
    let overlay = ledger.cursor_overlay().cloned().unwrap();

    ledger.request_personal_close();
    let returning = ledger.newest_request().unwrap().request_id;
    assert!(
        ledger.cursor_stack().is_none(),
        "cursor return is predicted"
    );
    assert_eq!(ledger.displayed_stack(0).map(|s| s.count), Some(32));
    assert_eq!(ledger.presented_slot_overlay(0), Some(&overlay));
    assert!(ledger.mark_transport_enqueued(30));
    let mut destination = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 32, 9);
    Arc::make_mut(&mut destination.slots)[0].durability_correction = 4;
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id: returning,
            containers: Arc::from([destination, correction(CONTAINER_NAME_CURSOR, 0, 0, -1)]),
        }]),
    }));
    assert!(ledger.mark_transport_enqueued(31));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.displayed_stack(0), Some(&confirmed));
    assert_eq!(ledger.slot_overlay(0), Some(&overlay));
    assert!(!ledger.resync_required());

    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(40));
    ledger.apply(&InventoryEvent::Open(personal_open(3)));
    assert_eq!(ledger.begin_click(0).unwrap(), -7);
    let pending = ledger.newest_request().unwrap();
    let StackRequestAction::Take {
        source,
        destination,
        ..
    } = pending.actions[0]
    else {
        panic!("expected Take action")
    };
    assert_eq!(source.stack_network_id, 9);
    assert_eq!(destination.stack_network_id, 0);
    assert_eq!(
        ledger.view().get(Cell::Cursor).unwrap().overlay.as_ref(),
        Some(&overlay)
    );
}

#[test]
fn admitted_mutation_and_cursor_return_settle_before_personal_close() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    assert!(ledger.confirmed_stack(Cell::Cursor).is_none());
    assert!(ledger.cursor_stack().is_some(), "the prediction is visible");

    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(30));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));

    assert!(
        matches!(ledger.personal, Some(PersonalWindow::Closing { .. })),
        "unadmitted Close response cannot erase outstanding inputs"
    );
    accept_cursor_move(&mut ledger, -3, false);
    accept_cursor_move(&mut ledger, -5, true);
    assert!(ledger.mark_transport_enqueued(31));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert!(ledger.personal.is_none());
    assert_eq!(ledger.pending_state(), None);
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.displayed_stack(0).map(|s| s.count), Some(32));
    assert!(!ledger.resync_required());
}

#[test]
fn unsent_and_admitted_cursor_mutations_are_retained_for_close_cleanup() {
    let mut unsent = ledger_with_slot_zero();
    acknowledge_personal_open(&mut unsent, 2);
    unsent.begin_click(0).unwrap();
    unsent.request_personal_close();
    assert_eq!(
        unsent.pending_state(),
        Some(InventoryPendingState::AwaitingTransport)
    );
    assert_eq!(unsent.pending_request_count(), 2);
    assert_eq!(unsent.displayed_stack(0).map(|stack| stack.count), Some(32));
    assert_eq!(unsent.cursor_stack(), None);

    let mut admitted = ledger_with_slot_zero();
    acknowledge_personal_open(&mut admitted, 2);
    admitted.begin_click(0).unwrap();
    assert!(admitted.mark_transport_enqueued(20));
    admitted.request_personal_close();
    assert_eq!(
        admitted.pending_state(),
        Some(InventoryPendingState::AwaitingResponse)
    );
    assert_eq!(admitted.pending_request_count(), 2);
}

#[test]
fn transport_and_authority_teardown_discard_personal_work() {
    let mut disconnected = ledger_with_slot_zero();
    acknowledge_personal_open(&mut disconnected, 2);
    disconnected.begin_click(0).unwrap();
    assert!(disconnected.mark_transport_enqueued(20));
    disconnected.transport_closed();
    assert!(disconnected.personal.is_none());
    assert_eq!(disconnected.pending_state(), None);
    assert!(disconnected.resync_required());

    let mut unauthorized = ledger_with_slot_zero();
    acknowledge_personal_open(&mut unauthorized, 2);
    unauthorized.begin_click(0).unwrap();
    unauthorized.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    assert!(unauthorized.personal.is_none());
    assert_eq!(unauthorized.pending_state(), None);
    assert_eq!(unauthorized.cursor_stack(), None);
    assert_eq!(
        unauthorized.begin_click(0),
        Err(InventoryGestureError::PersonalInventoryUnavailable)
    );
    assert!(unauthorized.request_personal_open(42));
}

#[test]
fn close_ack_clears_unrestated_cursor_and_session_reset_drops_all_personal_work() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    let request_id = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([
                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, 0),
                correction(CONTAINER_NAME_CURSOR, 0, 32, 10),
            ]),
        }]),
    }));
    assert!(ledger.cursor_stack().is_some());
    ledger.request_personal_close();
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: true,
    }));
    assert_eq!(ledger.cursor_stack(), None);
    assert!(ledger.resync_required());

    assert!(!ledger.request_personal_open(0));
    ledger.begin_session(2);
    assert!(ledger.personal.is_none());
    assert!(ledger.pending_closes.is_empty());
    assert_eq!(ledger.pending_state(), None);
}

fn mining_response(
    request_id: i32,
    status: StackResponseStatus,
    damage: i32,
    stack_id: i32,
) -> InventoryEvent {
    let mut container = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 1, stack_id);
    container.slots = Arc::from([StackResponseSlot {
        durability_correction: damage,
        ..container.slots[0].clone()
    }]);
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status,
            request_id,
            containers: Arc::from([container]),
        }]),
    })
}

#[test]
fn mining_responses_correct_the_worn_slot_without_a_pending_gesture() {
    let mut ledger = ledger_with_slot_zero();
    let first = ledger.begin_mining_request(0, 4).unwrap();
    let second = ledger.begin_mining_request(0, 5).unwrap();
    assert_eq!(
        (first, second),
        (-3, -5),
        "mining ids share the gesture counter"
    );
    // Outstanding predictions chain onto each other.
    assert_eq!(ledger.predicted_slot_damage(0), Some(5));
    ledger.apply(&mining_response(
        first,
        StackResponseStatus::Accepted,
        4,
        77,
    ));
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(77)
    );
    assert_eq!(ledger.predicted_slot_damage(0), Some(5));
    // A rejected prediction is dropped; the last accepted damage remains.
    ledger.apply(&mining_response(
        second,
        StackResponseStatus::Rejected,
        9,
        88,
    ));
    assert_eq!(ledger.predicted_slot_damage(0), Some(4));
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(77)
    );
    // Unknown and repeated ids change nothing.
    ledger.apply(&mining_response(
        second,
        StackResponseStatus::Accepted,
        9,
        88,
    ));
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(77)
    );
    acknowledge_personal_open(&mut ledger, 2);
    assert_eq!(
        ledger.begin_click(0).unwrap(),
        -7,
        "the next gesture keeps a distinct id"
    );
}

#[test]
fn mining_corrections_never_touch_a_slot_owned_by_a_pending_gesture() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    let mining = ledger.begin_mining_request(0, 1).unwrap();
    let gesture = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&mining_response(
        mining,
        StackResponseStatus::Accepted,
        1,
        77,
    ));
    assert_eq!(ledger.pending_request_id(), Some(gesture));
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id: gesture,
            containers: Arc::from([
                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, -1),
                correction(CONTAINER_NAME_CURSOR, 0, 32, 9),
            ]),
        }]),
    }));
    assert_eq!(ledger.pending_state(), None);
    assert!(!ledger.resync_required());
}

#[test]
fn outstanding_mining_requests_are_bounded() {
    let mut ledger = ledger_with_slot_zero();
    let ids = (0..40)
        .map(|damage| ledger.begin_mining_request(0, damage).unwrap())
        .collect::<Vec<_>>();
    ledger.apply(&mining_response(
        ids[0],
        StackResponseStatus::Accepted,
        0,
        55,
    ));
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(9),
        "an evicted request's late response is ignored"
    );
    assert_eq!(ledger.predicted_slot_damage(0), Some(39));
}

#[test]
fn cancelled_mining_request_leaves_no_prediction_behind() {
    let mut ledger = ledger_with_slot_zero();
    let id = ledger.begin_mining_request(0, 4).unwrap();
    ledger.cancel_mining_request(id);
    assert_eq!(ledger.predicted_slot_damage(0), None);
    assert_eq!(ledger.begin_mining_request(0, 4), Some(id - 2));
}

#[test]
fn accepted_mining_behind_an_expired_head_still_corrects_the_slot() {
    let mut ledger = ledger_with_slot_zero();
    let _unanswered = ledger.begin_mining_request(0, 4).unwrap();
    let answered = ledger.begin_mining_request(0, 5).unwrap();
    ledger.poll_timeout(0);
    ledger.apply(&mining_response(
        answered,
        StackResponseStatus::Accepted,
        5,
        88,
    ));
    ledger.poll_timeout(super::INVENTORY_REQUEST_TIMEOUT_MILLIS + 1);
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(88)
    );
}

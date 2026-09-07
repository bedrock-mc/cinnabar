use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CURSOR, ContainerCloseEvent,
    ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent,
    ItemStackResponseEvent, NetworkItemStack, StackResponse, StackResponseContainer,
    StackResponseSlot, StackResponseStatus,
};
use sha2::{Digest, Sha256};

use crate::ui_runtime::{UiRuntime, interaction::flush_inventory_send};

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

fn personal_open(window_id: i32) -> ContainerOpenEvent {
    ContainerOpenEvent {
        container: ContainerIdentity::window(window_id),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
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

#[test]
fn personal_gesture_waits_for_open_admission_and_uses_empty_stack_id_zero() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.pending_packet().unwrap().is_some());
    assert_eq!(
        ledger.begin_click(0),
        Err(InventoryGestureError::PersonalInventoryUnavailable)
    );

    assert!(ledger.mark_transport_enqueued(10));
    assert_eq!(ledger.begin_click(0).unwrap(), -3);
    let pending = ledger.pending.as_ref().unwrap();
    let StackRequestAction::Take {
        amount,
        source,
        destination,
    } = pending.action
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
    assert!(ledger.pending_packet().unwrap().is_some());
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
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(InventoryAuthority::Server);
    runtime.publish_local_runtime_id(1, 42).unwrap();
    runtime
        .inventory_ledger_mut()
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
    runtime.toggle_inventory();

    let mut open_attempts = 0;
    for now_millis in [10, 20] {
        assert_eq!(
            flush_inventory_send(&mut runtime, now_millis, |_| {
                open_attempts += 1;
                Err("full")
            }),
            Err("full")
        );
    }
    assert_eq!(open_attempts, 2);
    assert!(flush_inventory_send(&mut runtime, 30, |_| Ok::<_, &str>(())).unwrap());
    assert!(!flush_inventory_send(&mut runtime, 31, |_| Ok::<_, &str>(())).unwrap());

    assert_eq!(runtime.inventory_ledger_mut().begin_click(0).unwrap(), -3);
    let mut mutation_attempts = 0;
    for now_millis in [40, 50] {
        assert_eq!(
            flush_inventory_send(&mut runtime, now_millis, |_| {
                mutation_attempts += 1;
                Err("full")
            }),
            Err("full")
        );
    }
    assert_eq!(mutation_attempts, 2);
    assert!(flush_inventory_send(&mut runtime, 60, |_| Ok::<_, &str>(())).unwrap());
    assert!(!flush_inventory_send(&mut runtime, 61, |_| Ok::<_, &str>(())).unwrap());
    assert_eq!(
        runtime.inventory_ledger().pending_state(),
        Some(InventoryPendingState::AwaitingResponse)
    );
    runtime.inventory_transport_closed();
    assert!(!runtime.inventory_open());
    assert_eq!(runtime.inventory_ledger().pending_state(), None);
    assert!(runtime.inventory_ledger().resync_required());
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
    assert_eq!(opening.pending_close.unwrap().window_id, 2);
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
    let close = ledger.pending_close.expect("personal close");
    assert_eq!(close.window_id, 2);
    assert_eq!(close.window_type, PERSONAL_INVENTORY_WINDOW_TYPE);
    assert!(close.personal_generation.is_some());
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
fn local_close_before_ack_never_reopens_and_closes_the_late_dynamic_window() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.request_personal_close();
    assert!(!ledger.personal_inventory_desired_open());

    ledger.apply(&InventoryEvent::Open(personal_open(7)));

    assert!(!ledger.personal_inventory_desired_open());
    let close = ledger.pending_close.expect("late acknowledgement close");
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
    let pending = ledger.pending.as_ref().unwrap();
    let StackRequestAction::Place {
        amount,
        source,
        destination,
    } = pending.action
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
fn unsent_personal_mutation_is_dropped_on_close_but_admitted_one_reconciles() {
    let mut unsent = ledger_with_slot_zero();
    acknowledge_personal_open(&mut unsent, 2);
    unsent.begin_click(0).unwrap();
    unsent.request_personal_close();
    assert_eq!(unsent.pending_state(), None);
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
        Err(InventoryGestureError::AuthorityUnavailable)
    );
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
            containers: Arc::from([]),
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
    assert!(ledger.pending_close.is_none());
    assert_eq!(ledger.pending_state(), None);
}

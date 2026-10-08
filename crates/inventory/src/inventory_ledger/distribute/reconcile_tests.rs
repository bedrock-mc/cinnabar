//! A hover may happen after BDS has replaced every sparse ID with an ordinary
//! positive stack ID. Do not let a temporary transfer clone become a wire ID.

use super::*;
use protocol::{
    CONTAINER_NAME_INVENTORY, ItemStackResponseEvent, StackResponse, StackResponseContainer,
    StackResponseSlot, StackResponseStatus,
};

fn targets(slots: &[u8]) -> Vec<InventoryTarget> {
    slots.iter().copied().map(InventoryTarget::Player).collect()
}

fn accept(ledger: &mut PlayerInventoryLedger, request_id: i32, rows: &[(u8, u8, u8, i32)]) {
    let containers: Vec<_> = rows
        .iter()
        .map(
            |&(name, slot, count, item_stack_id)| StackResponseContainer {
                container: ContainerIdentity {
                    window_id: None,
                    slot_type: Some(name),
                    dynamic_id: None,
                },
                slots: Arc::from([StackResponseSlot {
                    slot,
                    hotbar_slot: slot,
                    count,
                    item_stack_id,
                    custom_name: Arc::from(""),
                    filtered_custom_name: Arc::from(""),
                    durability_correction: 0,
                }]),
            },
        )
        .collect();
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: containers.into(),
        }]),
    }));
}

fn accept_initial_split(ledger: &mut PlayerInventoryLedger) {
    let requests: Vec<_> = ledger
        .queue
        .iter()
        .map(|request| request.request_id)
        .collect();
    assert_eq!(requests.len(), 2);
    accept(
        ledger,
        requests[0],
        &[
            (CONTAINER_NAME_INVENTORY, 9, 5, 201),
            (CONTAINER_NAME_CURSOR, 0, 5, 60),
        ],
    );
    accept(
        ledger,
        requests[1],
        &[
            (CONTAINER_NAME_INVENTORY, 10, 5, 202),
            (CONTAINER_NAME_CURSOR, 0, 0, 0),
        ],
    );
    assert!(ledger.queue.is_empty());
}

#[test]
fn third_slot_after_acceptance_does_not_name_a_donor_as_the_new_destination() {
    let mut ledger = personal_ledger(&[]);
    set_cursor(&mut ledger, stack(60, 10));
    let mut drag = None;
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10]), DistributeMode::Even)
        .unwrap()
        .unwrap();
    accept_initial_split(&mut ledger);
    assert_eq!(ledger.displayed_stack(9).unwrap().stack_network_id, 201);
    assert_eq!(ledger.displayed_stack(10).unwrap().stack_network_id, 202);

    let second = ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even)
        .unwrap()
        .unwrap();
    let into_new_slot: Vec<_> = ledger
        .queue
        .iter()
        .flat_map(|request| &request.actions)
        .filter_map(|action| match action {
            StackRequestAction::Place { destination, .. }
                if destination.container == StackRequestContainer::PlayerInventory
                    && destination.slot == 11 =>
            {
                Some(destination.stack_network_id)
            }
            _ => None,
        })
        .collect();
    assert!(
        into_new_slot.len() >= 2,
        "the new slot has two different donors"
    );
    assert_eq!(into_new_slot, vec![0, ledger.queue[0].request_id]);
    assert_eq!(ledger.newest_request().unwrap().request_id, second);
    for request in &ledger.queue {
        assert_eq!(request.actions.len(), 1);
        assert!(protocol::item_stack_request_packet(request.request_id, &request.actions).is_ok());
    }
    for slot in [9, 10, 11] {
        assert_eq!(ledger.displayed_stack(slot).unwrap().count, 3);
    }
    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
}

#[test]
fn newly_empty_cursor_names_its_prior_request_when_multiple_donors_return_items() {
    let mut ledger = personal_ledger(&[(11, stack(203, 63))]);
    set_cursor(&mut ledger, stack(60, 10));
    let mut drag = None;
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10]), DistributeMode::Even)
        .unwrap()
        .unwrap();
    accept_initial_split(&mut ledger);
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even)
        .unwrap();
    let returned: Vec<_> = ledger
        .queue
        .iter()
        .flat_map(|request| &request.actions)
        .filter_map(|action| match action {
            StackRequestAction::Take { destination, .. } => Some(destination),
            _ => None,
        })
        .collect();
    assert_eq!(returned.len(), 2);
    for destination in &returned {
        assert_eq!(destination.container, StackRequestContainer::Cursor);
    }
    assert_eq!(returned[0].stack_network_id, 0);
    assert_eq!(returned[1].stack_network_id, ledger.queue[1].request_id);
    for request in &ledger.queue {
        assert!(protocol::item_stack_request_packet(request.request_id, &request.actions).is_ok());
    }
    assert_eq!(ledger.cursor_stack().unwrap().count, 3);
}

#[test]
fn fourth_slot_rebalances_after_every_third_slot_transfer_is_accepted() {
    let mut ledger = personal_ledger(&[]);
    set_cursor(&mut ledger, stack(60, 10));
    let mut drag = None;
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10]), DistributeMode::Even)
        .unwrap();
    accept_initial_split(&mut ledger);
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even)
        .unwrap();
    let requests: Vec<_> = ledger
        .queue
        .iter()
        .map(|request| request.request_id)
        .collect();
    assert_eq!(requests.len(), 3);
    accept(
        &mut ledger,
        requests[0],
        &[
            (CONTAINER_NAME_INVENTORY, 9, 3, 201),
            (CONTAINER_NAME_INVENTORY, 11, 2, 203),
        ],
    );
    accept(
        &mut ledger,
        requests[1],
        &[
            (CONTAINER_NAME_INVENTORY, 10, 4, 202),
            (CONTAINER_NAME_INVENTORY, 11, 3, 203),
        ],
    );
    accept(
        &mut ledger,
        requests[2],
        &[
            (CONTAINER_NAME_INVENTORY, 10, 3, 202),
            (CONTAINER_NAME_CURSOR, 0, 1, 204),
        ],
    );
    assert!(ledger.queue.is_empty());
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 11, 12]), DistributeMode::Even)
        .unwrap();
    for slot in [9, 10, 11, 12] {
        assert_eq!(ledger.displayed_stack(slot).unwrap().count, 2);
    }
    assert_eq!(ledger.cursor_stack().unwrap().count, 2);
    for request in &ledger.queue {
        assert!(protocol::item_stack_request_packet(request.request_id, &request.actions).is_ok());
    }
}

#[test]
fn same_item_added_to_cursor_is_not_taken_into_the_retained_split() {
    let mut ledger = personal_ledger(&[]);
    set_cursor(&mut ledger, stack(60, 10));
    let mut drag = None;
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10]), DistributeMode::Even)
        .unwrap();
    accept_initial_split(&mut ledger);
    set_cursor(&mut ledger, stack(204, 4));
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even)
        .unwrap();
    for slot in [9, 10, 11] {
        assert_eq!(ledger.displayed_stack(slot).unwrap().count, 3);
    }
    assert_eq!(ledger.cursor_stack().unwrap().count, 5);
}

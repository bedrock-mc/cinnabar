use super::*;
use crate::inventory_ledger::settling::{MAX_SETTLING_CLOSES, SettlingWindow};

fn acknowledge_close(ledger: &mut PlayerInventoryLedger, window_id: i32) {
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(window_id),
        window_type: NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
}

/// A second acknowledged close keeps the first closed generation's late answers current.
#[test]
fn two_overtaken_closes_each_settle_their_own_returns() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    accept_cursor_move(&mut ledger, -3, false);
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(30));
    assert!(ledger.mark_transport_enqueued(31));
    acknowledge_close(&mut ledger, 2);

    // Reopening is not held back by the unanswered return.
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(40));
    ledger.apply(&InventoryEvent::Open(personal_open(3)));
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 1,
        },
        stack: stack(11, 5),
        storage_item: None,
    }));
    ledger.begin_click(1).unwrap();
    assert!(ledger.mark_transport_enqueued(50));
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(60));
    assert!(ledger.mark_transport_enqueued(61));
    acknowledge_close(&mut ledger, 3);

    accept_cursor_move(&mut ledger, -5, true);
    let slot = ledger.confirmed.get(Cell::Inventory(0)).unwrap();
    assert_eq!((slot.stack.count, slot.stack.stack_network_id), (32, 9));
    assert_eq!(ledger.settling, [SettlingWindow::Personal(2)]);
}

/// Closed windows kept for late answers are bounded; the oldest is recovered first.
#[test]
fn settling_closes_are_bounded_oldest_first() {
    let mut ledger = ledger_with_slot_zero();
    for generation in 0..=MAX_SETTLING_CLOSES as u64 {
        ledger.retain_settling(SettlingWindow::Personal(generation));
    }
    assert_eq!(ledger.settling.len(), MAX_SETTLING_CLOSES);
    assert_eq!(ledger.settling.front(), Some(&SettlingWindow::Personal(1)));
}

use super::*;
use crate::inventory_ledger::{
    InventoryTarget, PERSONAL_INVENTORY_WINDOW_TYPE, PLAYER_INVENTORY_SLOT_COUNT,
};
use protocol::{StackRequestAction, StackRequestContainer};

/// Accepts the cursor cleanup, with an optional destination in the player inventory.
fn accept_return(ledger: &mut PlayerInventoryLedger, request_id: i32, destination: Option<u8>) {
    let correction = |name, slot, count, item_stack_id| StackResponseContainer {
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
    };
    let mut containers = vec![correction(protocol::CONTAINER_NAME_CURSOR, 0, 0, 0)];
    if let Some(slot) = destination {
        containers.push(correction(
            protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            slot,
            3,
            91,
        ));
    }
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: containers.into(),
        }]),
    }));
}

/// A chest or kit menu returns the cursor before closing, dropping only what cannot fit.
#[test]
fn storage_close_returns_or_drops_cursor_without_locking_inventory() {
    for full in [false, true] {
        for take_pending in [false, true] {
            for close_first in [false, true] {
                let mut ledger = ready(super::super::SMALL_STORAGE_SLOT_COUNT, 980);
                let slots = (0..PLAYER_INVENTORY_SLOT_COUNT)
                    .map(|slot| {
                        if full {
                            stack(8, 64, slot as i32 + 100)
                        } else {
                            NetworkItemStack::empty()
                        }
                    })
                    .collect::<Vec<_>>();
                ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                    container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                    slots: slots.into(),
                    storage_item: NetworkItemStack::empty(),
                }));
                let take = ledger.begin_storage_click(2).unwrap();
                assert!(ledger.mark_transport_enqueued(10));
                if !take_pending {
                    ledger.apply(&accepted_storage_take(take, 980));
                }

                ledger.request_storage_close();
                let returning = ledger.newest_request().expect("cursor cleanup");
                assert_ne!(returning.request_id, take, "closing must return the cursor");
                let return_id = returning.request_id;
                if full {
                    assert!(matches!(returning.actions.as_slice(),
                        [StackRequestAction::Drop { amount: 3, source, randomly: false }]
                        if source.container == StackRequestContainer::Cursor));
                } else {
                    assert!(matches!(returning.actions.as_slice(),
                        [StackRequestAction::Place { amount: 3, source, destination }]
                        if source.container == StackRequestContainer::Cursor
                            && destination.slot == 0));
                }
                assert!(ledger.cursor_stack().is_none());
                let (batch, count) = ledger.pending_batch().unwrap().unwrap();
                assert_eq!(format!("{:?}", batch.kind()), "ItemStackRequestPacket");
                assert_eq!(count, 1);
                assert!(ledger.mark_transport_enqueued(20));
                let (close, _) = ledger.pending_batch().unwrap().unwrap();
                assert_eq!(format!("{:?}", close.kind()), "ContainerClosePacket");
                assert!(ledger.mark_transport_enqueued(21));
                let close_ack = InventoryEvent::Close(ContainerCloseEvent {
                    container: ContainerIdentity::window(1),
                    window_type: protocol::NO_CONTAINER_WINDOW_TYPE,
                    server_initiated: false,
                });
                if close_first {
                    ledger.apply(&close_ack);
                }
                if take_pending {
                    ledger.apply(&accepted_storage_take(take, 980));
                }
                accept_return(&mut ledger, return_id, (!full).then_some(0));
                if !close_first {
                    ledger.apply(&close_ack);
                }
                assert!(
                    !ledger.resync_required(),
                    "close must leave world interactions usable"
                );
                assert!(ledger.cursor_stack().is_none());
                assert_eq!(ledger.pending_request_count(), 0);
                assert_eq!(
                    ledger.displayed_stack(0).unwrap().count,
                    if full { 64 } else { 3 }
                );

                assert!(ledger.request_personal_open(42));
                assert!(ledger.mark_transport_enqueued(30));
                ledger.apply(&open(2, PERSONAL_INVENTORY_WINDOW_TYPE));
                assert!(
                    ledger.begin_click(0).is_ok(),
                    "inventory gestures still work"
                );
            }
        }
    }
}

/// Restates a retained personal crafting input independently of the open chest.
fn personal_input(stack_id: i32) -> InventoryEvent {
    let slot = *protocol::CRAFTING_INPUT_SLOTS.start();
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                slot_type: protocol::ui_slot_container_name(slot),
                dynamic_id: None,
            },
            slot: u16::from(slot),
        },
        stack: stack(5, 3, stack_id),
        storage_item: None,
    })
}

/// A chest close cannot move a personal crafting input the chest does not expose.
#[test]
fn storage_close_does_not_return_inactive_personal_input() {
    let mut ledger = ready(super::super::SMALL_STORAGE_SLOT_COUNT, 980);
    ledger.apply(&player_content());
    ledger.apply(&personal_input(92));
    ledger.request_storage_close();
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(
        ledger
            .target_stack(InventoryTarget::Craft(
                *protocol::CRAFTING_INPUT_SLOTS.start()
            ))
            .is_some()
    );
}

/// An invalid identity in an inactive grid cannot abort a chest's cursor return.
#[test]
fn storage_close_returns_cursor_despite_inactive_input_identity() {
    let mut ledger = ready(super::super::SMALL_STORAGE_SLOT_COUNT, 980);
    ledger.apply(&player_content());
    let take = ledger.begin_storage_click(2).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&accepted_storage_take(take, 980));
    ledger.apply(&personal_input(0));
    ledger.request_storage_close();
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.pending_request_count(), 1);
    assert!(
        matches!(ledger.newest_request().unwrap().actions.as_slice(),
        [StackRequestAction::Place { amount: 3, source, .. }]
        if source.container == StackRequestContainer::Cursor)
    );
}

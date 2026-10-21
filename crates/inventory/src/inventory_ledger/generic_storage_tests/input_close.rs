use super::*;
use crate::inventory_ledger::{InventoryTarget, PLAYER_INVENTORY_SLOT_COUNT};
use protocol::StackRequestAction;
use sha2::{Digest, Sha256};

/// Builds a valid empty user-data envelope for either inventory authority.
fn item(network_id: i32, count: u16, stack_id: i32) -> NetworkItemStack {
    let extra_data: Arc<[u8]> = Arc::from([0; 10]);
    NetworkItemStack {
        network_id,
        count,
        stack_network_id: stack_id,
        nbt_digest: Sha256::digest(&extra_data).into(),
        extra_data,
        ..NetworkItemStack::empty()
    }
}

/// Restates one UI input independently of local predictions.
fn input(slot: u8, stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                slot_type: protocol::ui_slot_container_name(slot),
                dynamic_id: None,
            },
            slot: u16::from(slot),
        },
        stack,
        storage_item: None,
    })
}

/// Active station inputs return before close, dropping only when the player has no room.
#[test]
fn station_close_returns_each_input_in_both_authority_modes() {
    for authority in [InventoryAuthority::Client, InventoryAuthority::Server] {
        for full in [false, true] {
            for window_type in i8::MIN..=i8::MAX {
                let Some(kind) = protocol::WindowKind::from_window_type(window_type) else {
                    continue;
                };
                for slot in super::super::screen_actions::input_slots(kind) {
                    let mut ledger = PlayerInventoryLedger::default();
                    ledger.begin_session(41);
                    ledger.apply(&InventoryEvent::Authority(authority));
                    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                        slots: (0..PLAYER_INVENTORY_SLOT_COUNT)
                            .map(|index| {
                                if full {
                                    item(8, 64, index as i32 + 100)
                                } else {
                                    NetworkItemStack::empty()
                                }
                            })
                            .collect(),
                        storage_item: NetworkItemStack::empty(),
                    }));
                    ledger.apply(&open(1, window_type));
                    ledger.apply(&input(*slot, item(5, 1, 92)));
                    let inactive = *protocol::CRAFTING_INPUT_SLOTS.start();
                    ledger.apply(&input(inactive, item(5, 2, 0)));
                    ledger.request_storage_close();
                    let cleanup = ledger.newest_request().expect("active input cleanup");
                    let expected_container = protocol::ui_slot_request_container(*slot).unwrap();
                    assert!(
                        matches!(cleanup.actions.as_slice(),
                    [StackRequestAction::Drop { amount: 1, source, randomly: false }]
                    if full && source.slot == *slot && source.container == expected_container)
                            || matches!(cleanup.actions.as_slice(),
                    [StackRequestAction::Place { amount: 1, source, destination }]
                    if !full && source.slot == *slot && source.container == expected_container
                        && destination.slot == 0)
                    );
                    assert!(ledger.target_stack(InventoryTarget::Craft(*slot)).is_none());
                    assert_eq!(
                        ledger.displayed_stack(0).unwrap().count,
                        if full { 64 } else { 1 }
                    );
                    assert_eq!(
                        ledger
                            .target_stack(InventoryTarget::Craft(inactive))
                            .unwrap()
                            .count,
                        2
                    );
                    let packet = ledger.pending_batch().unwrap().unwrap().0;
                    assert_eq!(
                        protocol::decode_batch(protocol::encode(&packet).unwrap()).unwrap(),
                        vec![packet]
                    );
                    assert!(ledger.mark_transport_enqueued(10));
                    assert!(
                        protocol::test_support::packets::container_close(
                            &ledger.pending_batch().unwrap().unwrap().0
                        )
                        .is_some()
                    );
                }
            }
        }
    }
}

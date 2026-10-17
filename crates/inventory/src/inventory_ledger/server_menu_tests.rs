//! Captured server-menu default descriptors resolve against their open window.

use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_LEVEL_ENTITY, ContainerIdentity, ContainerOpenEvent, InventoryContentEvent,
    InventoryEvent, InventorySlotEvent, NetworkItemStack, SlotIdentity, StackRequestAction,
    StackRequestContainer, WINDOW_TYPE_ANVIL,
};

use super::*;

mod large_menu;
mod repeated_open;

const MENU_WINDOW: i32 = 1;
const MENU_CELLS: usize = 45;

/// Creates one occupied menu cell with the supplied stack identity.
fn stack(id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        stack_network_id: id,
        count: 1,
        ..NetworkItemStack::default()
    }
}

/// Opens a server-authoritative window for descriptor admission tests.
fn open(window_type: i8) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(MENU_WINDOW),
        window_type,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
}

/// Addresses the fixture window with the supplied wire descriptor.
fn descriptor(name: Option<u8>, dynamic_id: Option<u32>) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(MENU_WINDOW),
        slot_type: name,
        dynamic_id,
    }
}

/// Populates the last cell so row bounds and identity can be checked together.
fn content(container: ContainerIdentity) -> InventoryEvent {
    let mut slots = vec![NetworkItemStack::default(); MENU_CELLS];
    slots[MENU_CELLS - 1] = stack(100);
    InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    })
}

/// Updates one cell without replacing the rest of the menu contents.
fn slot(container: ContainerIdentity, at: u16, id: i32) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container,
            slot: at,
        },
        stack: stack(id),
        storage_item: None,
    })
}

#[test]
fn server_menu_default_descriptors_admit_content_updates_and_level_entity_requests() {
    for name in [None, Some(0)] {
        let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
        let container = descriptor(name, None);
        ledger.apply(&content(container));
        assert_eq!(ledger.storage_slot_count(), Some(MENU_CELLS));
        assert_eq!(
            ledger
                .storage_stack((MENU_CELLS - 1) as u8)
                .unwrap()
                .stack_network_id,
            100
        );
        ledger.apply(&slot(container, (MENU_CELLS - 1) as u16, 101));
        assert_eq!(
            ledger
                .storage_stack((MENU_CELLS - 1) as u8)
                .unwrap()
                .stack_network_id,
            101
        );
        ledger.begin_storage_click((MENU_CELLS - 1) as u8).unwrap();
        let Some(StackRequestAction::Take { source, .. }) = ledger.newest_action() else {
            panic!("taking the last server menu item");
        };
        assert_eq!(
            source.container,
            StackRequestContainer::LevelEntity { dynamic_id: None }
        );
        assert_eq!(usize::from(source.slot), MENU_CELLS - 1);
        assert_eq!(source.stack_network_id, 101);
    }
}

#[test]
fn server_menu_admits_its_window_ignoring_descriptor_names() {
    for container in [
        ContainerIdentity {
            window_id: Some(2),
            ..descriptor(Some(0), None)
        },
        descriptor(Some(0), Some(91)),
        descriptor(None, Some(91)),
        descriptor(Some(protocol::CONTAINER_NAME_INVENTORY), None),
    ] {
        let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
        ledger.apply(&content(container));
        let matching = container.window_id == Some(1);
        assert_eq!(
            ledger.storage_slot_count(),
            matching.then_some(MENU_CELLS),
            "{container:?}"
        );
        ledger.apply(&slot(container, (MENU_CELLS - 1) as u16, 102));
        assert_eq!(
            ledger
                .storage_stack((MENU_CELLS - 1) as u8)
                .map(|stack| stack.stack_network_id),
            matching.then_some(102),
            "{container:?}"
        );
    }
}

#[test]
fn server_menu_ignores_ordinary_dynamic_descriptor_in_requests() {
    let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
    let container = descriptor(Some(CONTAINER_NAME_LEVEL_ENTITY), Some(91));
    ledger.apply(&content(container));
    ledger.apply(&slot(container, (MENU_CELLS - 1) as u16, 103));
    ledger.begin_storage_click((MENU_CELLS - 1) as u8).unwrap();
    let Some(StackRequestAction::Take { source, .. }) = ledger.newest_action() else {
        panic!("taking the ordinary window item");
    };
    assert_eq!(
        source.container,
        StackRequestContainer::LevelEntity { dynamic_id: None }
    );
    assert_eq!(source.stack_network_id, 103);
}

#[test]
fn ordinary_anvil_slot_uses_manager_index_instead_of_packet_name() {
    let mut ledger = open(WINDOW_TYPE_ANVIL);
    ledger.apply(&slot(descriptor(Some(0), None), 1, 104));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(2))
            .unwrap()
            .stack_network_id,
        104
    );
    assert!(ledger.target_stack(InventoryTarget::Craft(1)).is_none());
    assert!(ledger.storage_stack(1).is_none());
}

#[test]
fn server_menu_zero_default_descriptor_admits_its_window_contents() {
    let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
    let container = descriptor(Some(0), Some(0));
    ledger.apply(&content(container));
    assert_eq!(ledger.storage_slot_count(), Some(MENU_CELLS));
    ledger.apply(&slot(container, (MENU_CELLS - 1) as u16, 105));
    ledger.begin_storage_click((MENU_CELLS - 1) as u8).unwrap();
    let Some(StackRequestAction::Take { source, .. }) = ledger.newest_action() else {
        panic!("taking the server menu item");
    };
    assert_eq!(
        source.container,
        StackRequestContainer::LevelEntity { dynamic_id: None }
    );
    assert_eq!(source.stack_network_id, 105);
}

#[test]
fn server_menu_accepts_another_click_after_an_unnamed_cursor_reset() {
    for name in [None, Some(0)] {
        let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
        let menu = descriptor(Some(0), None);
        ledger.apply(&content(menu));
        for id in 110..113 {
            let request = ledger.begin_storage_click((MENU_CELLS - 1) as u8).unwrap();
            assert!(matches!(
                ledger.newest_action(),
                Some(StackRequestAction::Take { .. })
            ));
            assert!(ledger.mark_transport_enqueued(10));
            ledger.apply(&InventoryEvent::Response(
                protocol::ItemStackResponseEvent {
                    responses: vec![protocol::StackResponse {
                        status: protocol::StackResponseStatus::Accepted,
                        request_id: request,
                        containers: vec![protocol::StackResponseContainer {
                            container: ContainerIdentity {
                                window_id: None,
                                slot_type: Some(protocol::CONTAINER_NAME_CURSOR),
                                dynamic_id: None,
                            },
                            slots: vec![protocol::StackResponseSlot {
                                slot: 0,
                                hotbar_slot: 0,
                                count: 1,
                                item_stack_id: id,
                                custom_name: Arc::from(""),
                                filtered_custom_name: Arc::from(""),
                                durability_correction: -1,
                            }]
                            .into(),
                        }]
                        .into(),
                    }]
                    .into(),
                },
            ));
            assert!(ledger.cursor_stack().is_some());
            ledger.apply(&slot(menu, (MENU_CELLS - 1) as u16, id + 10));
            ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity {
                        window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                        slot_type: name,
                        dynamic_id: None,
                    },
                    slot: 0,
                },
                stack: NetworkItemStack::empty(),
                storage_item: None,
            }));
            assert!(
                ledger.cursor_stack().is_none(),
                "cursor descriptor {name:?}"
            );
            assert!(!ledger.resync_required());
        }
    }
}

#[test]
fn server_menu_zero_response_descriptor_does_not_lock_repeat_clicks() {
    let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
    ledger.apply(&content(descriptor(Some(0), None)));
    let request = ledger.begin_storage_click((MENU_CELLS - 1) as u8).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: vec![protocol::StackResponse {
                status: protocol::StackResponseStatus::Accepted,
                request_id: request,
                containers: vec![protocol::StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(CONTAINER_NAME_LEVEL_ENTITY),
                        dynamic_id: Some(0),
                    },
                    slots: vec![protocol::StackResponseSlot {
                        slot: (MENU_CELLS - 1) as u8,
                        hotbar_slot: (MENU_CELLS - 1) as u8,
                        count: 1,
                        item_stack_id: 100,
                        custom_name: Arc::from(""),
                        filtered_custom_name: Arc::from(""),
                        durability_correction: -1,
                    }]
                    .into(),
                }]
                .into(),
            }]
            .into(),
        },
    ));
    assert!(!ledger.resync_required());
    assert!(ledger.cursor_stack().is_none());
    assert!(ledger.begin_storage_click((MENU_CELLS - 1) as u8).is_ok());
}

#[test]
fn empty_success_ack_allows_repeated_purchases_and_reopening() {
    for list_present in [false, true] {
        let mut ledger = PlayerInventoryLedger::default();
        ledger.begin_session(1);
        ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
        let mut now = 0;
        for window in 1..=3 {
            let menu = ContainerIdentity {
                window_id: Some(window),
                ..descriptor(Some(0), None)
            };
            ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(window),
                window_type: GENERIC_STORAGE_WINDOW_TYPE,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }));
            ledger.apply(&content(menu));
            for sync_first in [false, true, false] {
                let request = ledger.begin_storage_click((MENU_CELLS - 1) as u8).unwrap();
                let Some(StackRequestAction::Take {
                    source,
                    destination,
                    ..
                }) = ledger.newest_action()
                else {
                    panic!("every purchase starts with an empty cursor");
                };
                assert!(source.stack_network_id > 0);
                assert_eq!(destination.stack_network_id, 0);
                assert!(ledger.mark_transport_enqueued(now));
                let sync = |ledger: &mut PlayerInventoryLedger| {
                    ledger.apply(&slot(menu, (MENU_CELLS - 1) as u16, 100));
                    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                        container: ContainerIdentity {
                            window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                            slot_type: Some(0),
                            dynamic_id: None,
                        },
                        slots: vec![NetworkItemStack::empty(); protocol::UI_SLOT_COUNT].into(),
                        storage_item: NetworkItemStack::empty(),
                    }));
                };
                if sync_first {
                    sync(&mut ledger);
                }
                let packet =
                    protocol::test_support::packets::empty_stack_response(request, list_present);
                let decoded = protocol::decode_batch(protocol::encode(&packet).unwrap())
                    .unwrap()
                    .pop()
                    .unwrap();
                let Some(protocol::WorldEvent::Inventory(event)) =
                    protocol::into_world_event(decoded, 0).unwrap()
                else {
                    panic!("decoded shop acknowledgement");
                };
                ledger.apply(&event);
                if !sync_first {
                    sync(&mut ledger);
                }
                now += INVENTORY_REQUEST_TIMEOUT_MILLIS + 1;
                ledger.poll_timeout(now);
                assert_eq!(ledger.pending_request_count(), 0);
                assert!(ledger.cursor_stack().is_none());
                assert!(!ledger.resync_required());
            }
            ledger.request_storage_close();
            assert!(ledger.pending_control_packet().unwrap().is_some());
            assert!(ledger.mark_transport_enqueued(now));
            ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
                container: ContainerIdentity::window(window),
                window_type: GENERIC_STORAGE_WINDOW_TYPE,
                server_initiated: false,
            }));
            assert!(ledger.storage_generation().is_none());
            assert!(!ledger.resync_required());
        }
    }
}

/// Anvil Content has two inputs plus a deferred preview and requires the complete shape.
#[test]
fn ordinary_anvil_content_requires_all_manager_cells_and_skips_preview() {
    for count in [2, 3, 4] {
        let mut ledger = open(WINDOW_TYPE_ANVIL);
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: descriptor(Some(protocol::CONTAINER_NAME_CURSOR), Some(91)),
            slots: (0..count)
                .map(|index| stack(100 + index))
                .collect::<Vec<_>>()
                .into(),
            storage_item: NetworkItemStack::empty(),
        }));
        for (slot, id) in [(ANVIL_TARGET_SLOT, 100), (ANVIL_MATERIAL_SLOT, 101)] {
            assert_eq!(
                ledger
                    .target_stack(InventoryTarget::Craft(slot))
                    .map(|stack| stack.stack_network_id),
                (count == 3).then_some(id)
            );
        }
        assert!(ledger.created_output_stack().is_none());
        assert!(ledger.cursor_stack().is_none());
    }
}

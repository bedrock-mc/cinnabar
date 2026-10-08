//! Captured server-menu default descriptors resolve against their open window.

use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_LEVEL_ENTITY, ContainerIdentity, ContainerOpenEvent, InventoryContentEvent,
    InventoryEvent, InventorySlotEvent, NetworkItemStack, SlotIdentity, StackRequestAction,
    StackRequestContainer, WINDOW_TYPE_ANVIL,
};

use super::*;

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
fn server_menu_default_alias_never_claims_foreign_or_dynamic_descriptors() {
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
        assert_eq!(ledger.storage_slot_count(), None, "{container:?}");
        ledger.apply(&slot(container, (MENU_CELLS - 1) as u16, 102));
        assert!(
            ledger.storage_stack((MENU_CELLS - 1) as u8).is_none(),
            "{container:?}"
        );
    }
}

#[test]
fn server_menu_named_dynamic_identity_remains_exact_in_requests() {
    let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
    let container = descriptor(Some(CONTAINER_NAME_LEVEL_ENTITY), Some(91));
    ledger.apply(&content(container));
    ledger.apply(&slot(container, (MENU_CELLS - 1) as u16, 103));
    ledger.begin_storage_click((MENU_CELLS - 1) as u8).unwrap();
    let Some(StackRequestAction::Take { source, .. }) = ledger.newest_action() else {
        panic!("taking the dynamically addressed item");
    };
    assert_eq!(
        source.container,
        StackRequestContainer::LevelEntity {
            dynamic_id: Some(91)
        }
    );
    assert_eq!(source.stack_network_id, 103);
}

#[test]
fn server_menu_alias_leaves_real_anvil_zero_name_as_ui_input() {
    let mut ledger = open(WINDOW_TYPE_ANVIL);
    ledger.apply(&slot(descriptor(Some(0), None), 1, 104));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(1))
            .unwrap()
            .stack_network_id,
        104
    );
    assert!(ledger.storage_stack(1).is_none());
}

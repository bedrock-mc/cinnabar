use std::sync::Arc;

use assets::entity::mount::{MountBody, MountEquipment, MountSlots};
use inventory::inventory_ledger::InventoryTarget;
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, NetworkItemStack,
    WINDOW_TYPE_HORSE,
};

use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::inventory_drag::PointerAction;
use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;

/// Opens a mount window with one known player stack and empty storage cells.
fn fixture(identifier: &str) -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([ItemRegistryEntry {
            components: None,
            identifier: Arc::from(identifier),
            network_id: 1,
            component_based: false,
            version: ItemRegistryVersion::None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    });
    let mut slots = vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[0] = NetworkItemStack {
        network_id: 1,
        stack_network_id: 10,
        count: 1,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(4),
        window_type: WINDOW_TYPE_HORSE,
        position: [0, 64, 0],
        runtime_entity_id: 42,
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(4),
        slots: vec![NetworkItemStack::empty(); 5].into(),
        storage_item: NetworkItemStack::empty(),
    }));
    (player, runtime)
}

#[test]
fn mount_quick_move_uses_admitted_equipment_slots() {
    let saddle_only = MountSlots {
        saddle: true,
        body: MountBody::None,
    };
    let body_only = MountSlots {
        saddle: false,
        body: MountBody::Carpet,
    };
    for equipment in [
        MountEquipment::Unresolved,
        MountEquipment::NotEquippable,
        MountEquipment::Equippable(saddle_only),
        MountEquipment::Provisional(body_only),
    ] {
        for (identifier, destination) in [
            ("minecraft:saddle", 0),
            ("minecraft:red_carpet", 1),
            ("minecraft:iron_horse_armor", 1),
            ("minecraft:apple", 2),
        ] {
            let (mut player, mut runtime) = fixture(identifier);
            runtime.screen_state_mut().mount_equipment = equipment;
            runtime.perform_pointer_action(
                &mut player,
                PointerAction::QuickMove(InventoryCellHit::Player(0)),
            );
            let ledger = runtime.inventory_ledger(&player);
            assert!(ledger.target_stack(InventoryTarget::Player(0)).is_none());
            let allowed = equipment.permits_inventory_slot(destination);
            assert_eq!(
                ledger
                    .target_stack(InventoryTarget::Storage(destination))
                    .is_some(),
                allowed,
                "{equipment:?}: {identifier}",
            );
            assert_eq!(ledger.displayed_stack(9).is_some(), !allowed);
            assert_eq!(ledger.pending_request_count(), 1);
        }
    }
}

#[test]
fn mount_quick_move_returns_storage_items_to_the_player() {
    let (mut player, mut runtime) = fixture("minecraft:apple");
    let mut held = runtime
        .inventory_ledger(&player)
        .displayed_stack(0)
        .unwrap()
        .clone();
    held.stack_network_id = 20;
    let mut cells = vec![NetworkItemStack::empty(); 5];
    cells[2] = held;
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(4),
            slots: cells.into(),
            storage_item: NetworkItemStack::empty(),
        }));
    assert!(
        runtime
            .inventory_ledger(&player)
            .target_stack(InventoryTarget::Storage(2))
            .is_some()
    );
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::QuickMove(InventoryCellHit::Storage(2)),
    );
    let ledger = runtime.inventory_ledger(&player);
    assert!(ledger.target_stack(InventoryTarget::Storage(2)).is_none());
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 2);
    assert!(ledger.displayed_stack(9).is_none());
    assert_eq!(ledger.pending_request_count(), 1);
}

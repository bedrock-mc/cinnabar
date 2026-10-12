//! Offhand quick moves retain server authority and whole-item identity.

use std::sync::Arc;

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent,
    ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, ItemStackResponseEvent,
    NetworkItemStack, StackRequestAction, StackRequestContainer, StackResponse,
    StackResponseStatus,
};

use super::*;

/// Creates a server-backed personal screen with a totem and known offhand content.
fn ledger(source: u8, offhand: NetworkItemStack) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([ItemRegistryEntry {
            components: None,
            identifier: Arc::from(TOTEM_IDENTIFIER),
            network_id: 6,
            component_based: false,
            version: ItemRegistryVersion::None,
            component_digest: [6; 32],
            negotiated_max_stack_size: None,
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    });
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[usize::from(source)] = totem(100);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
        slots: Arc::from([offhand]),
        storage_item: NetworkItemStack::default(),
    }));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
}

/// A single totem with an authoritative stack identity.
fn totem(id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        count: 1,
        stack_network_id: id,
        ..NetworkItemStack::default()
    }
}

/// Counts all retained player and offhand totems in the predicted view.
fn count(ledger: &PlayerInventoryLedger) -> u16 {
    (0..protocol::PLAYER_INVENTORY_SLOTS)
        .filter_map(|slot| ledger.displayed_stack(slot))
        .chain(ledger.target_stack(InventoryTarget::Offhand))
        .filter(|stack| stack.network_id == 6)
        .map(|stack| stack.count)
        .sum()
}

#[test]
fn totem_quick_move_equips_from_main_inventory_and_hotbar() {
    for source in [0, 4, protocol::HOTBAR_SLOT_COUNT, 20] {
        let mut ledger = ledger(source, NetworkItemStack::default());
        ledger
            .begin_quick_move(InventoryTarget::Player(source))
            .unwrap();
        assert!(matches!(
            ledger.newest_action(),
            Some(StackRequestAction::Place { amount: 1, source: from, destination })
                if from.slot == source && from.stack_network_id == 100
                    && destination.container == StackRequestContainer::Offhand
                    && destination.slot == 1 && destination.stack_network_id == 0
        ));
        assert!(ledger.displayed_stack(source).is_none());
        assert_eq!(
            ledger.target_stack(InventoryTarget::Offhand).unwrap().count,
            1
        );
        assert_eq!(count(&ledger), 1);
        assert_eq!(
            ledger.begin_quick_move(InventoryTarget::Player(source)),
            Err(InventoryGestureError::EmptyGesture)
        );
        assert_eq!(count(&ledger), 1);
    }
}

#[test]
fn totem_quick_move_from_main_inventory_targets_offhand() {
    let mut ledger = ledger(20, NetworkItemStack::default());
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    assert!(matches!(
        ledger.newest_action(),
        Some(StackRequestAction::Place { amount: 1, source, destination })
            if source.slot == 20 && source.stack_network_id == 100
                && destination.container == StackRequestContainer::Offhand
                && destination.slot == 1 && destination.stack_network_id == 0
    ));
}

#[test]
fn occupied_offhand_keeps_its_totem_and_uses_inventory_fallback() {
    for source in [0, 20] {
        let held = totem(101);
        let mut ledger = ledger(source, held.clone());
        ledger
            .begin_quick_move(InventoryTarget::Player(source))
            .unwrap();
        assert!(matches!(
            ledger.newest_action(),
            Some(StackRequestAction::Place { destination, .. })
                if destination.container == StackRequestContainer::PlayerInventory
        ));
        assert_eq!(ledger.target_stack(InventoryTarget::Offhand), Some(&held));
        assert!(ledger.displayed_stack(source).is_none());
        assert_eq!(count(&ledger), 2);
    }
}

#[test]
fn rejected_offhand_quick_move_restores_the_source_without_duplication() {
    let mut ledger = ledger(20, NetworkItemStack::default());
    let request = ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Rejected,
            request_id: request,
            containers: Arc::from([]),
        }]),
    }));
    assert_eq!(ledger.displayed_stack(20), Some(&totem(100)));
    assert!(ledger.target_stack(InventoryTarget::Offhand).is_none());
    assert_eq!(count(&ledger), 1);
    assert!(ledger.cursor_stack().is_none());
}

#[test]
fn totem_quick_move_targets_an_open_chest_before_equipment() {
    let mut ledger = ledger(20, NetworkItemStack::default());
    ledger.request_personal_close();
    ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: true,
    }));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(7),
        slots: Arc::from(vec![NetworkItemStack::default(); SMALL_STORAGE_SLOT_COUNT]),
        storage_item: NetworkItemStack::default(),
    }));
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    assert!(matches!(
        ledger.newest_action(),
        Some(StackRequestAction::Place { destination, .. })
            if matches!(destination.container, StackRequestContainer::LevelEntity { .. })
    ));
    assert!(ledger.target_stack(InventoryTarget::Offhand).is_none());
    assert_eq!(
        ledger.storage_stack(0),
        Some(&NetworkItemStack {
            stack_network_id: -3,
            ..totem(100)
        })
    );
}

/// Replaces the same item identity with explicit session declarations.
fn declare(ledger: &mut PlayerInventoryLedger, components: protocol::ItemComponents, custom: bool) {
    let slots: Arc<[NetworkItemStack]> = (0..protocol::PLAYER_INVENTORY_SLOTS)
        .map(|slot| ledger.displayed_stack(slot).cloned().unwrap_or_default())
        .collect();
    let offhand = ledger
        .target_stack(InventoryTarget::Offhand)
        .cloned()
        .unwrap_or_default();
    let mut entry = ledger.negotiated_item_entry(6).unwrap().clone();
    if custom {
        entry.identifier = Arc::from("test:charm");
    }
    entry.component_based = custom;
    entry.version = ItemRegistryVersion::DataDriven;
    entry.negotiated_max_stack_size = components.max_stack_size;
    entry.components = Some(Arc::new(components));
    entry.canonical_empty_component_data = false;
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([entry]),
    });
    if custom {
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots,
            storage_item: NetworkItemStack::default(),
        }));
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
            slots: Arc::from([offhand]),
            storage_item: NetworkItemStack::default(),
        }));
    }
}

#[test]
fn explicit_offhand_permission_overrides_stock_and_admits_custom_items() {
    for (custom, allow) in [(false, Some(false)), (true, Some(true)), (true, None)] {
        let mut ledger = ledger(20, NetworkItemStack::default());
        declare(
            &mut ledger,
            protocol::ItemComponents {
                allow_off_hand: allow,
                allow_off_hand_declared: allow.is_some(),
                max_stack_size: Some(1),
                ..Default::default()
            },
            custom,
        );
        ledger
            .begin_quick_move(InventoryTarget::Player(20))
            .unwrap();
        let equips = ledger.target_stack(InventoryTarget::Offhand).is_some();
        assert_eq!(equips, allow == Some(true));
        assert_eq!(count(&ledger), 1);
    }
}

#[test]
fn malformed_permission_does_not_inherit_stock_permission() {
    let mut ledger = ledger(20, NetworkItemStack::default());
    declare(
        &mut ledger,
        protocol::ItemComponents {
            allow_off_hand_declared: true,
            ..Default::default()
        },
        false,
    );
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    assert!(ledger.target_stack(InventoryTarget::Offhand).is_none());
    assert_eq!(count(&ledger), 1);
}

#[test]
fn offhand_wearable_admission_precedes_an_explicit_denial() {
    let mut ledger = ledger(20, NetworkItemStack::default());
    declare(
        &mut ledger,
        protocol::ItemComponents {
            allow_off_hand: Some(false),
            allow_off_hand_declared: true,
            wearable_slot: Some(Arc::from("slot.weapon.offhand")),
            wearable_declared: true,
            max_stack_size: Some(1),
            ..Default::default()
        },
        true,
    );
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    assert!(ledger.target_stack(InventoryTarget::Offhand).is_some());
    assert_eq!(count(&ledger), 1);
}

#[test]
fn stackable_custom_offhand_merges_only_up_to_declared_capacity() {
    let mut held = totem(101);
    held.count = 14;
    let mut ledger = ledger(20, held);
    declare(
        &mut ledger,
        protocol::ItemComponents {
            allow_off_hand: Some(true),
            allow_off_hand_declared: true,
            max_stack_size: Some(16),
            ..Default::default()
        },
        true,
    );
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    assert_eq!(
        ledger.target_stack(InventoryTarget::Offhand).unwrap().count,
        15
    );
    assert!(ledger.displayed_stack(20).is_none());
    assert_eq!(count(&ledger), 15);
}

#[test]
fn stock_totem_keeps_capacity_one_with_unrelated_components() {
    let mut ledger = ledger(20, NetworkItemStack::default());
    declare(
        &mut ledger,
        protocol::ItemComponents {
            glint: true,
            ..Default::default()
        },
        false,
    );
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 20,
        },
        stack: NetworkItemStack {
            count: 2,
            ..totem(100)
        },
        storage_item: None,
    }));
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    assert_eq!(
        ledger.target_stack(InventoryTarget::Offhand).unwrap().count,
        1
    );
    assert_eq!(ledger.displayed_stack(20).unwrap().count, 1);
    assert_eq!(count(&ledger), 2);
}

#[test]
fn named_custom_stacks_skip_offhand_merges_and_keep_inventory_fallback() {
    for named in [Cell::Inventory(20), Cell::Offhand] {
        let mut held = totem(101);
        held.count = 14;
        let mut ledger = ledger(20, held);
        declare(
            &mut ledger,
            protocol::ItemComponents {
                allow_off_hand: Some(true),
                allow_off_hand_declared: true,
                max_stack_size: Some(16),
                ..Default::default()
            },
            true,
        );
        ledger.set_confirmed_overlay(
            named,
            StackResponseOverlay {
                custom_name: Some(Arc::from("Named charm")),
                ..Default::default()
            },
        );
        ledger
            .begin_quick_move(InventoryTarget::Player(20))
            .unwrap();
        assert_eq!(ledger.displayed_stack(0).unwrap().count, 1);
        assert!(ledger.displayed_stack(20).is_none());
        assert_eq!(
            ledger.target_stack(InventoryTarget::Offhand).unwrap().count,
            14
        );
        assert_eq!(count(&ledger), 15);
        let named_destination = if named == Cell::Offhand {
            named
        } else {
            Cell::Inventory(0)
        };
        assert_eq!(
            ledger
                .view()
                .get(named_destination)
                .unwrap()
                .overlay
                .as_ref()
                .unwrap()
                .custom_name
                .as_deref(),
            Some("Named charm")
        );
    }
}

#[test]
fn changed_admission_retires_an_unanswered_equipment_prediction() {
    let mut ledger = ledger(20, NetworkItemStack::default());
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    assert!(ledger.target_stack(InventoryTarget::Offhand).is_some());
    declare(
        &mut ledger,
        protocol::ItemComponents {
            allow_off_hand: Some(false),
            allow_off_hand_declared: true,
            ..Default::default()
        },
        false,
    );
    assert!(ledger.target_stack(InventoryTarget::Offhand).is_none());
    assert_eq!(ledger.displayed_stack(20), Some(&totem(100)));
    assert_eq!(count(&ledger), 1);
}

#[test]
fn offhand_quick_move_returns_to_combined_inventory_in_slot_order() {
    let mut ledger = ledger(20, totem(101));
    ledger.begin_quick_move(InventoryTarget::Offhand).unwrap();
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 1);
    assert!(ledger.target_stack(InventoryTarget::Offhand).is_none());
    assert_eq!(count(&ledger), 2);
}

#[test]
fn explicit_armor_slot_precedes_offhand_and_caps_equipment_to_one() {
    let mut ledger = ledger(20, NetworkItemStack::default());
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
        slots: Arc::from(vec![
            NetworkItemStack::default();
            usize::from(protocol::ARMOR_SLOTS)
        ]),
        storage_item: NetworkItemStack::default(),
    }));
    declare(
        &mut ledger,
        protocol::ItemComponents {
            wearable_slot: Some(Arc::from("slot.armor.head")),
            wearable_declared: true,
            allow_off_hand: Some(true),
            allow_off_hand_declared: true,
            max_stack_size: Some(64),
            ..Default::default()
        },
        true,
    );
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 20,
        },
        stack: NetworkItemStack {
            count: 2,
            ..totem(100)
        },
        storage_item: None,
    }));
    ledger
        .begin_quick_move(InventoryTarget::Player(20))
        .unwrap();
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Armor(0))
            .unwrap()
            .count,
        1
    );
    assert_eq!(ledger.displayed_stack(20).unwrap().count, 1);
    assert!(ledger.target_stack(InventoryTarget::Offhand).is_none());
}

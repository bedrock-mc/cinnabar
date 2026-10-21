use inventory::{PlayerInventoryLedger, inventory_ledger::InventoryTarget};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, InventorySlotEvent, NetworkItemStack, SlotIdentity,
};

/// Creates an original positive-id item for routing observations.
fn stack(id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        count: 1,
        stack_network_id: id,
        ..NetworkItemStack::empty()
    }
}

/// Opens one manager and supplies an observed empty player inventory.
fn open(kind: i8) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)]
            .into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: kind,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
}

/// Lists each observed legacy network index and its effective raw UI destination.
fn cases() -> Vec<(&'static str, i8, Vec<Option<u8>>, usize)> {
    vec![
        (
            "anvil",
            protocol::WINDOW_TYPE_ANVIL,
            vec![Some(1), Some(2), None],
            3,
        ),
        (
            "workbench",
            protocol::WINDOW_TYPE_WORKBENCH,
            vec![None; 10],
            0,
        ),
        (
            "enchanting",
            protocol::WINDOW_TYPE_ENCHANTMENT,
            vec![Some(14), Some(15)],
            2,
        ),
        ("beacon", protocol::WINDOW_TYPE_BEACON, vec![Some(27)], 1),
        (
            "grindstone",
            protocol::WINDOW_TYPE_GRINDSTONE,
            vec![Some(16), Some(17), None],
            3,
        ),
        (
            "loom",
            protocol::WINDOW_TYPE_LOOM,
            vec![Some(9), Some(10), Some(11), None],
            4,
        ),
        (
            "cartography",
            protocol::WINDOW_TYPE_CARTOGRAPHY,
            vec![Some(12), Some(13), None],
            3,
        ),
        (
            "stonecutter",
            protocol::WINDOW_TYPE_STONECUTTER,
            vec![Some(3), None],
            2,
        ),
        (
            "smithing",
            protocol::WINDOW_TYPE_SMITHING_TABLE,
            vec![Some(51), Some(52), None, None],
            3,
        ),
        (
            "trading",
            protocol::WINDOW_TYPE_TRADE,
            protocol::MODERN_TRADE_SLOTS
                .into_iter()
                .map(Some)
                .chain([None])
                .collect(),
            protocol::MODERN_TRADE_SLOTS.len() + 1,
        ),
    ]
}

/// Observes all retained UI backing cells, excluding the separate cursor.
fn cells(ledger: &PlayerInventoryLedger) -> Vec<(u8, i32)> {
    (1..u8::try_from(protocol::UI_SLOT_COUNT).unwrap())
        .filter_map(|slot| {
            ledger
                .target_stack(InventoryTarget::Craft(slot))
                .map(|s| (slot, s.stack_network_id))
        })
        .collect()
}

/// Ordinary manager Slot indices ignore packet names and exclude network result cells.
#[test]
fn ordinary_manager_slot_matrix() {
    let mut failures = Vec::new();
    for (name, kind, mapping, _) in cases() {
        for (index, expected) in mapping.into_iter().enumerate() {
            let mut ledger = open(kind);
            ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity {
                        window_id: Some(7),
                        slot_type: Some(0),
                        dynamic_id: None,
                    },
                    slot: index as u16,
                },
                stack: stack(100 + index as i32),
                storage_item: None,
            }));
            let wanted: Vec<_> = expected
                .map(|raw| (raw, 100 + index as i32))
                .into_iter()
                .collect();
            let observed = cells(&ledger);
            eprintln!("SLOT {name} index={index} expected={wanted:?} actual={observed:?}");
            if observed != wanted {
                failures.push((name, index, observed, wanted));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

/// Ordinary Content must match each manager's copies extent before per-index application.
#[test]
fn ordinary_manager_content_matrix() {
    let mut failures = Vec::new();
    for (name, kind, mapping, extent) in cases() {
        for size in [extent, extent + 1] {
            let mut ledger = open(kind);
            ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity {
                    window_id: Some(7),
                    slot_type: Some(0),
                    dynamic_id: None,
                },
                slots: (0..size)
                    .map(|i| stack(100 + i as i32))
                    .collect::<Vec<_>>()
                    .into(),
                storage_item: NetworkItemStack::empty(),
            }));
            let wanted: Vec<_> = if size == extent {
                mapping
                    .iter()
                    .take(size)
                    .enumerate()
                    .filter_map(|(i, raw)| raw.map(|raw| (raw, 100 + i as i32)))
                    .collect()
            } else {
                vec![]
            };
            let mut wanted = wanted;
            wanted.sort();
            let observed = cells(&ledger);
            eprintln!("CONTENT {name} len={size} expected={wanted:?} actual={observed:?}");
            if observed != wanted {
                failures.push((name, size, observed, wanted));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

/// A newly supported UI-backed window must receive an explicit manager-routing fixture.
#[test]
fn supported_ui_managers_have_routing_coverage() {
    for wire_type in i8::MIN..=i8::MAX {
        let Some(kind) = protocol::WindowKind::from_window_type(wire_type) else {
            continue;
        };
        if kind.is_ui_backed() && kind != protocol::WindowKind::Lectern {
            assert!(
                cases()
                    .iter()
                    .any(|(_, fixture_type, _, _)| *fixture_type == wire_type),
                "missing manager fixture: {kind:?}"
            );
        }
    }
}

use super::*;
use assets::{
    CompiledEntityAssets, EntityAssetSource, ItemDisplayTransform, ItemVisualDefinition,
    ItemVisualDefinitionRoute, ItemVisualKey,
    item::{fuel, retail},
};
use sha2::{Digest, Sha256};

/// Builds a validated, texture-free catalog using the owning compatibility inputs.
fn catalog() -> RuntimeEntityAssets {
    let sources = [
        (fuel::SOURCE_PATH, fuel::SOURCE_BYTES),
        (retail::SOURCE_PATH, retail::SOURCE_BYTES),
    ]
    .map(|(path, bytes)| EntityAssetSource {
        path: path.into(),
        source_bytes: bytes.len() as u32,
        source_sha256: Sha256::digest(bytes).into(),
    });
    let visuals =
        ["minecraft:coal", "minecraft:oak_planks"].map(|identifier| ItemVisualDefinition {
            key: ItemVisualKey {
                identifier: identifier.into(),
                metadata: 0,
            },
            source: 0,
            route: ItemVisualDefinitionRoute::Missing,
            fuel: fuel::builtin_item_fuel(identifier),
            grip: Default::default(),
            glint: Default::default(),
            durability: assets::item::durability::ItemDurability::Unresolved,
            armour: assets::item::armour::ItemArmour::Unresolved,
            first_person: ItemDisplayTransform::identity(),
            third_person: ItemDisplayTransform::identity(),
            dropped: ItemDisplayTransform::identity(),
        });
    RuntimeEntityAssets::from_compiled(
        CompiledEntityAssets {
            item_use_profiles: Box::new([]),
            seat_layouts: Box::new([]),
            source_manifest_sha256: [1; 32],
            interaction_traits: Box::new([]),
            block_visual_count: 0,
            sources: sources.into(),
            symbols: Box::new([]),
            geometries: Box::new([]),
            animation_clips: Box::new([]),
            animation_channels: Box::new([]),
            animation_keyframes: Box::new([]),
            molang_symbols: Box::new([]),
            molang_expressions: Box::new([]),
            molang_ops: Box::new([]),
            molang_collections: Box::new([]),
            molang_collection_items: Box::new([]),
            controllers: Box::new([]),
            controller_states: Box::new([]),
            controller_animations: Box::new([]),
            controller_transitions: Box::new([]),
            rig_bindings: Box::new([]),
            rig_geometries: Box::new([]),
            rig_animations: Box::new([]),
            rig_controllers: Box::new([]),
            item_visuals: visuals.into(),
            item_visual_aliases: Box::new([]),
            render: Default::default(),
        }
        .with_unresolved_item_use()
        .with_unresolved_seats()
        .with_unresolved_interactions(),
    )
    .unwrap()
}

/// Models an empty server descriptor for an existing vanilla item.
fn descriptor(identifier: &str, component_based: bool) -> ItemRegistryEntry {
    let mut entry = protocol::vanilla_item_registry()[0].clone();
    entry.identifier = identifier.into();
    entry.component_based = component_based;
    entry.version = ItemRegistryVersion::None;
    entry.components = None;
    entry.canonical_empty_component_data = true;
    entry
}

#[test]
fn empty_component_descriptors_preserve_admitted_furnace_fuel() {
    let catalog = catalog();
    for identifier in ["minecraft:coal", "minecraft:oak_planks"] {
        for component_based in [false, true] {
            assert_eq!(
                resolve(&descriptor(identifier, component_based), Some(&catalog)).eligible(),
                Some(true),
                "{identifier}, component_based={component_based}"
            );
        }
    }
}

#[test]
fn explicit_and_unknown_fuel_descriptors_keep_their_authority() {
    let catalog = catalog();
    let mut entry = descriptor("minecraft:coal", true);
    entry.canonical_empty_component_data = false;
    assert_eq!(resolve(&entry, Some(&catalog)), ItemFuel::Unresolved);
    entry.components = Some(Arc::new(protocol::ItemComponents {
        fuel_declared: true,
        ..Default::default()
    }));
    assert_eq!(resolve(&entry, Some(&catalog)), ItemFuel::Unresolved);
    Arc::make_mut(entry.components.as_mut().unwrap()).fuel_duration_ticks = Some(0);
    assert_eq!(resolve(&entry, Some(&catalog)).eligible(), Some(false));
    Arc::make_mut(entry.components.as_mut().unwrap()).fuel_duration_ticks = Some(40);
    assert_eq!(
        resolve(&entry, Some(&catalog)),
        ItemFuel::Session { burn_ticks: 40 }
    );
    entry.version = ItemRegistryVersion::Unknown(100);
    assert_eq!(resolve(&entry, Some(&catalog)), ItemFuel::Unresolved);
    assert_eq!(
        resolve(&descriptor("custom:coal", true), Some(&catalog)),
        ItemFuel::Unresolved
    );
    assert_eq!(
        resolve(&descriptor("minecraft:coal", true), None),
        ItemFuel::Unresolved
    );
}

#[test]
fn furnace_fuel_response_retains_the_accepted_window_slot() {
    use protocol::{
        ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
        InventoryEvent, ItemRegistryEvent, ItemStackResponseEvent, NetworkItemStack, StackResponse,
        StackResponseContainer, StackResponseSlot, StackResponseStatus,
    };
    let mut entry = descriptor("minecraft:oak_planks", false);
    entry.network_id = 10001;
    entry.negotiated_max_stack_size = Some(protocol::ITEM_DEFAULT_MAX_STACK_SIZE);
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: vec![entry.clone()].into(),
    });
    ledger.set_item_fuel_catalog(Arc::new(catalog()));
    let mut slots = vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[0] = NetworkItemStack {
        network_id: entry.network_id,
        count: 10,
        stack_network_id: 100,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(3),
        window_type: protocol::WINDOW_TYPE_FURNACE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(3),
        slots: vec![NetworkItemStack::empty(); 3].into(),
        storage_item: NetworkItemStack::empty(),
    }));
    let request_id = ledger
        .begin_quick_move(super::super::InventoryTarget::Player(0))
        .unwrap();
    assert_eq!(ledger.storage_stack(1).map(|stack| stack.count), Some(10));
    let containers = [
        (protocol_ids::ContainerName::HotBar, 0, 0, 0),
        (protocol_ids::ContainerName::FurnaceFuel, 1, 10, 200),
    ]
    .map(
        |(name, slot, count, item_stack_id)| StackResponseContainer {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(name.wire_value()),
                dynamic_id: None,
            },
            slots: vec![StackResponseSlot {
                slot,
                hotbar_slot: slot,
                count,
                item_stack_id,
                custom_name: "".into(),
                filtered_custom_name: "".into(),
                durability_correction: 0,
            }]
            .into(),
        },
    );
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: vec![StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: containers.into(),
        }]
        .into(),
    }));
    assert!(ledger.displayed_stack(0).is_none());
    assert_eq!(
        ledger.storage_stack(1).map(|stack| (
            stack.network_id,
            stack.count,
            stack.stack_network_id
        )),
        Some((entry.network_id, 10, 200))
    );
    assert!(ledger.storage_stack(2).is_none());
}

#[test]
fn named_station_request_addresses_return_to_their_original_cells() {
    use protocol::{ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent};
    let mut checked = 0;
    for window_type in i8::MIN..=i8::MAX {
        let Some(kind) = protocol::WindowKind::from_window_type(window_type) else {
            continue;
        };
        let Some(protocol::OpenCells::Named { lengths, .. }) = kind.open_cells() else {
            continue;
        };
        for &length in lengths {
            let mut ledger = PlayerInventoryLedger::default();
            ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(3),
                window_type,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }));
            ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(3),
                slots: vec![protocol::NetworkItemStack::empty(); length].into(),
                storage_item: protocol::NetworkItemStack::empty(),
            }));
            for cell in 0..length {
                let cell = u8::try_from(cell).unwrap();
                let (container, slot) =
                    protocol::open_cell_request(kind, cell, None, None).unwrap();
                assert_eq!(
                    ledger.request_cell(protocol::StackRequestSlot {
                        container,
                        slot,
                        stack_network_id: 0
                    }),
                    Some(super::super::Cell::Storage(cell)),
                    "{kind:?}, slot{cell}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
}

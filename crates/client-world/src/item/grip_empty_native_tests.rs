use super::*;
use assets::{CompiledEntityAssets, ItemVisualDefinition};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Supplies a checked native grip without requiring installed packs or artwork.
fn native_assets(identifier: &str) -> Arc<RuntimeEntityAssets> {
    let sources = [
        (
            assets::item::grip::GRIP_SOURCE_PATH,
            assets::item::grip::GRIP_SOURCE_BYTES,
        ),
        (
            assets::item::retail::SOURCE_PATH,
            assets::item::retail::SOURCE_BYTES,
        ),
    ]
    .map(|(path, bytes)| assets::EntityAssetSource {
        path: path.into(),
        source_bytes: bytes.len() as u32,
        source_sha256: Sha256::digest(bytes).into(),
    });
    let compiled = CompiledEntityAssets {
        source_manifest_sha256: [1; 32],
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
        item_use_profiles: Box::new([]),
        seat_layouts: Box::new([]),
        interaction_traits: Box::new([]),
        item_visuals: vec![ItemVisualDefinition {
            key: assets::ItemVisualKey {
                identifier: identifier.into(),
                metadata: 0,
            },
            source: 0,
            route: assets::ItemVisualDefinitionRoute::Missing,
            grip: assets::item::grip::builtin_item_grip(identifier),
            glint: assets::item::glint::ItemGlint::Unresolved,
            fuel: assets::item::fuel::ItemFuel::Unresolved,
            armour: assets::item::armour::ItemArmour::Unresolved,
            durability: assets::item::durability::ItemDurability::Unresolved,
            first_person: assets::ItemDisplayTransform::identity(),
            third_person: assets::ItemDisplayTransform::identity(),
            dropped: assets::ItemDisplayTransform::identity(),
        }]
        .into(),
        item_visual_aliases: Box::new([]),
        render: Default::default(),
    }
    .with_unresolved_item_use();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
}

/// Recreates a component-based native definition with canonical empty component data.
fn native_entry(identifier: &str) -> ItemRegistryEntry {
    let mut entry = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap()
        .clone();
    entry.component_based = true;
    entry.version = ItemRegistryVersion::DataDriven;
    entry.components = None;
    entry.canonical_empty_component_data = true;
    entry
}

#[test]
fn empty_component_based_native_definition_keeps_compiled_grip() {
    let identifier = "minecraft:fishing_rod";
    let assets = native_assets(identifier);
    let entry = native_entry(identifier);
    let expected = assets::item::grip::builtin_item_grip(identifier);
    assert_eq!(expected.upright(), Some(true));
    assert!(expected.mirrored_art());
    for components in [None, Some(Arc::new(protocol::ItemComponents::default()))] {
        let mut entry = entry.clone();
        entry.components = components;
        assert_eq!(resolve(&entry, Some(&assets)), expected);
    }
}

#[test]
fn native_grip_requires_compiled_facts_and_respects_component_overrides() {
    let identifier = "minecraft:fishing_rod";
    let assets = native_assets(identifier);
    let mut entry = native_entry(identifier);
    assert_eq!(resolve(&entry, None), ItemGrip::Unresolved);
    entry.version = ItemRegistryVersion::Unknown(42);
    assert_eq!(resolve(&entry, Some(&assets)), ItemGrip::Unresolved);
    entry.version = ItemRegistryVersion::DataDriven;
    entry.identifier = "custom:rod".into();
    assert_eq!(resolve(&entry, Some(&assets)), ItemGrip::Unresolved);
    entry.identifier = identifier.into();
    entry.canonical_empty_component_data = false;
    assert_eq!(resolve(&entry, Some(&assets)), ItemGrip::Unresolved);
    entry.components = Some(Arc::new(protocol::ItemComponents {
        hand_equipped: Some(false),
        hand_equipped_declared: true,
        ..Default::default()
    }));
    let grip = resolve(&entry, Some(&assets));
    assert_eq!(grip.upright(), Some(false));
    assert!(grip.mirrored_art());
    Arc::make_mut(entry.components.as_mut().unwrap()).hand_equipped = None;
    assert_eq!(resolve(&entry, Some(&assets)), ItemGrip::Unresolved);
}

#[test]
fn empty_native_grip_refreshes_retained_equipment_after_an_override() {
    let identifier = "minecraft:fishing_rod";
    let expected = assets::item::grip::builtin_item_grip(identifier);
    let mut store = crate::item::ItemStateStore::with_assets(native_assets(identifier));
    let mut entry = native_entry(identifier);
    let network_id = entry.network_id;
    assert!(store.apply_registry(protocol::ItemRegistryEvent {
        entries: vec![entry.clone()].into()
    }));
    let lifetime = actor_animation::ActorLifetimeId {
        session_id: 1,
        dimension: 0,
        runtime_id: 1,
        spawn_revision: 1,
    };
    let stack = protocol::NetworkItemStack {
        network_id,
        count: 1,
        ..protocol::NetworkItemStack::empty()
    };
    assert!(store.apply_equipment(
        lifetime,
        1,
        protocol::EquipmentEvent {
            actor_runtime_id: 1,
            stack,
            inventory_slot: 0,
            selected_slot: 0,
            window_id: 0,
            handedness: Some(protocol::ActorHandedness::Right),
        }
    ));
    let key = (lifetime, protocol::ActorHandedness::Right);
    assert_eq!(store.equipment[&key].item.grip, expected);
    entry.canonical_empty_component_data = false;
    entry.components = Some(Arc::new(protocol::ItemComponents {
        hand_equipped: Some(false),
        hand_equipped_declared: true,
        ..Default::default()
    }));
    assert!(store.apply_registry(protocol::ItemRegistryEvent {
        entries: vec![entry.clone()].into()
    }));
    assert_eq!(store.equipment[&key].item.grip.upright(), Some(false));
    assert!(store.equipment[&key].item.grip.mirrored_art());
    entry.canonical_empty_component_data = true;
    entry.components = None;
    assert!(store.apply_registry(protocol::ItemRegistryEvent {
        entries: vec![entry].into()
    }));
    assert_eq!(store.equipment[&key].item.grip, expected);
}

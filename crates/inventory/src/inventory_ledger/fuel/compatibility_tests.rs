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
            durability: Default::default(),
            armour: Default::default(),
            first_person: ItemDisplayTransform::identity(),
            third_person: ItemDisplayTransform::identity(),
            dropped: ItemDisplayTransform::identity(),
        });
    RuntimeEntityAssets::from_compiled(
        CompiledEntityAssets {
            item_use_profiles: Box::new([]),
            seat_layouts: Box::new([]),
            source_manifest_sha256: [0; 32],
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

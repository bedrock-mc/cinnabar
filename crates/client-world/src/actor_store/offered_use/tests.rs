use super::*;
use protocol::{NetworkItemStack, VerifiedNetworkItemStack};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Installs an original behavior descriptor and the existing pinned item-use profiles.
fn fixture(identifier: &str, definition: EntityUseAdmission) -> ActorStore {
    let mut compiled =
        assets::carriers::startup::test_carriers::synthetic_interaction_assets(identifier);
    let mut sources = compiled.sources.into_vec();
    sources.insert(
        0,
        assets::EntityAssetSource {
            path: "behavior/entities/fixture.json".into(),
            source_bytes: 1,
            source_sha256: [1; 32],
        },
    );
    let (source_bytes, source_sha256) = assets::item_use::source_identity();
    sources.push(assets::EntityAssetSource {
        path: assets::item_use::SOURCE_PATH.into(),
        source_bytes,
        source_sha256,
    });
    compiled.symbols[0].source_index += 1;
    compiled.sources = sources.into();
    compiled.item_use_profiles = assets::item_use::compile(&compiled.item_visuals);
    compiled
        .interaction_traits
        .iter_mut()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap()
        .offered_use = definition;
    let runtime = Arc::new(
        assets::RuntimeEntityAssets::decode(&assets::encode_entity_blob(&compiled).unwrap())
            .unwrap(),
    );
    let mut store = ActorStore::new_with_entity_assets(1, 0, runtime);
    assert!(store.items.apply_registry(protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry().into()
    }));
    store.apply(
        1,
        1,
        crate::actor_store::tests::entity_spawn(8, -8, identifier, Arc::from([])),
    );
    store
}

use assets::entity::offered_use::{ComponentUse, EntityUseAdmission, UseCondition, UseSelector};

/// Makes the absence proof explicit without inferring callback success.
fn no_offering() -> EntityUseAdmission {
    EntityUseAdmission::Components {
        source: 0,
        offerings: Box::new([]),
    }
}

/// Verifies one retained stack against the same empty payload digest used for packet admission.
fn stack(identifier: &str) -> VerifiedNetworkItemStack {
    let network_id = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap()
        .network_id;
    let digest: [u8; 32] = Sha256::digest([]).into();
    VerifiedNetworkItemStack::try_new(
        NetworkItemStack {
            network_id,
            count: 1,
            stack_network_id: 5,
            nbt_digest: digest,
            ..NetworkItemStack::empty()
        },
        digest,
    )
    .unwrap()
}

#[test]
fn verified_resolver_and_behavior_absence_admit_a_retail_throwable_only() {
    let store = fixture("minecraft:zombie", no_offering());
    let actor = store.get(8).unwrap();
    assert_eq!(
        store.offered_throwable_use(actor, &stack("minecraft:snowball")),
        OfferedUse::Absent
    );
    assert_eq!(
        store.offered_throwable_use(actor, &stack("minecraft:name_tag")),
        OfferedUse::Unresolved
    );
}

#[test]
fn missing_behavior_unknown_native_and_prevent_default_do_not_prove_absence() {
    for (identifier, definition) in [
        ("minecraft:zombie", EntityUseAdmission::Unresolved),
        ("minecraft:armor_stand", no_offering()),
        ("fixture:overridden", no_offering()),
    ] {
        let store = fixture(identifier, definition);
        assert_eq!(
            store.offered_throwable_use(store.get(8).unwrap(), &stack("minecraft:snowball")),
            OfferedUse::Unresolved
        );
    }
}

#[test]
fn matching_feed_or_unknown_group_state_cannot_become_an_absence_proof() {
    let definition = EntityUseAdmission::Components {
        source: 0,
        offerings: Box::from([ComponentUse {
            selector: UseSelector::UnresolvedComponentGroup { group: 0 },
            component: "minecraft:ageable".into(),
            condition: UseCondition::HeldItem {
                identifier: "minecraft:snowball".into(),
                metadata: None,
            },
        }]),
    };
    let store = fixture("minecraft:happy_ghast", definition);
    assert_eq!(
        store.offered_throwable_use(store.get(8).unwrap(), &stack("minecraft:snowball")),
        OfferedUse::Unresolved
    );
}

#[test]
fn a_nonempty_server_component_override_cannot_inherit_the_retail_absence_proof() {
    let mut store = fixture("minecraft:zombie", no_offering());
    let stack = stack("minecraft:snowball");
    let mut entry = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.network_id == stack.network_id())
        .unwrap()
        .clone();
    entry.component_based = true;
    entry.canonical_empty_component_data = false;
    assert!(store.items.apply_registry(protocol::ItemRegistryEvent {
        entries: Arc::from([entry])
    }));
    assert_eq!(
        store.offered_throwable_use(store.get(8).unwrap(), &stack),
        OfferedUse::Unresolved
    );
}

#[test]
fn a_custom_actor_cannot_inherit_a_verified_base_resolver() {
    let mut store = fixture("fixture:overridden", no_offering());
    store.apply_actor_interactions(protocol::ActorIdentifierRegistry {
        entries: Arc::from([protocol::ActorIdentifier {
            identifier: "fixture:overridden".into(),
            base_identifier: "minecraft:zombie".into(),
        }]),
        skipped: 0,
    });
    let actor = store.get(8).unwrap();
    assert_eq!(
        actor.entity_fact.actor_use_resolver,
        ActorUseResolver::Unresolved
    );
    assert_eq!(
        store.offered_throwable_use(actor, &stack("minecraft:snowball")),
        OfferedUse::Unresolved
    );
}

/// Recreates the native definition carried by a negotiated startup registry.
fn component_native_snowball() -> protocol::ItemRegistryEntry {
    let mut entry = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:snowball")
        .unwrap()
        .clone();
    entry.component_based = true;
    entry.version = protocol::ItemRegistryVersion::None;
    entry.components = None;
    entry.canonical_empty_component_data = true;
    entry
}

#[test]
fn a_canonical_empty_component_native_throwable_keeps_its_compiled_admission() {
    let mut store = fixture("minecraft:zombie", no_offering());
    let entry = component_native_snowball();
    assert!(entry.component_based);
    assert_eq!(entry.version, protocol::ItemRegistryVersion::None);
    assert!(entry.canonical_empty_component_data);
    assert!(entry.components.is_none());
    assert!(entry.item_tags.is_empty());
    assert!(store.items.apply_registry(protocol::ItemRegistryEvent {
        entries: Arc::from([entry.clone()])
    }));
    assert_eq!(
        store.items.item_use_profile(entry.network_id).kind,
        Some(assets::item_use::ItemUseKind::Throw)
    );
    assert_eq!(
        store.offered_throwable_use(store.get(8).unwrap(), &stack("minecraft:snowball")),
        OfferedUse::Absent
    );
    let mut data_driven = entry.clone();
    data_driven.version = protocol::ItemRegistryVersion::DataDriven;
    assert!(store.items.apply_registry(protocol::ItemRegistryEvent {
        entries: Arc::from([data_driven])
    }));
    assert_eq!(
        store.offered_throwable_use(store.get(8).unwrap(), &stack("minecraft:snowball")),
        OfferedUse::Absent
    );
    for mutation in 0..4 {
        let mut entry = entry.clone();
        match mutation {
            0 => entry.version = protocol::ItemRegistryVersion::Unknown(42),
            1 => entry.canonical_empty_component_data = false,
            2 => {
                entry.canonical_empty_component_data = false;
                entry.components = Some(Arc::new(protocol::ItemComponents::default()));
            }
            _ => entry.identifier = "fixture:custom_snowball".into(),
        }
        assert!(store.items.apply_registry(protocol::ItemRegistryEvent {
            entries: Arc::from([entry])
        }));
        assert_eq!(
            store.offered_throwable_use(store.get(8).unwrap(), &stack("minecraft:snowball")),
            OfferedUse::Unresolved
        );
    }
}

#[test]
fn an_empty_live_registry_uses_only_the_effective_builtin_throwable_record() {
    let mut store = fixture("minecraft:zombie", no_offering());
    assert!(store.items.apply_registry(protocol::ItemRegistryEvent {
        entries: Arc::from([component_native_snowball()])
    }));
    store.apply_item_actor(
        1,
        2,
        protocol::ItemActorEvent::Registry(protocol::ItemRegistryEvent {
            entries: Arc::from([]),
        }),
    );
    assert_eq!(
        store.offered_throwable_use(store.get(8).unwrap(), &stack("minecraft:snowball")),
        OfferedUse::Absent
    );
    assert_eq!(
        store.offered_throwable_use(store.get(8).unwrap(), &stack("minecraft:name_tag")),
        OfferedUse::Unresolved
    );
}

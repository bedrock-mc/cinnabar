use sha2::{Digest, Sha256};
use std::hash::Hash;

use assets::{
    BlockVisualId, ItemActionPhase, ItemIconRef, ItemStackIdentity, ItemVisualId, ItemVisualRoute,
};

use assets as entity;
use assets as item;

use entity::{CompiledEntityAssets as CompiledEntityAssetsV4, RuntimeEntityAssets};
use item::{
    ItemDisplayScalar, ItemDisplayTransform, ItemTextureReference, ItemVisualAlias,
    ItemVisualDefinition, ItemVisualDefinitionRoute, ItemVisualKey, MAX_ITEM_IDENTIFIER_BYTES,
    MAX_ITEM_VISUAL_ALIASES, MAX_ITEM_VISUALS,
};

fn assert_hash<T: Hash>() {}

fn nonempty_identity(metadata: u32) -> ItemStackIdentity {
    ItemStackIdentity {
        network_id: 42,
        metadata,
        stack_network_id: -1,
        count: 3,
        nbt_digest: [0x5a; 32],
        block_runtime_id: 0,
    }
}

fn visual_key(identifier: impl Into<Box<str>>) -> ItemVisualKey {
    ItemVisualKey {
        identifier: identifier.into(),
        metadata: 0,
    }
}

#[test]
fn item_identity_and_visual_contracts_are_copyable_values() {
    assert_hash::<ItemStackIdentity>();
    assert_hash::<ItemVisualId>();
    assert_hash::<BlockVisualId>();
    assert_hash::<ItemIconRef>();

    let identity = nonempty_identity(7);
    let identity_copy = identity;
    assert_eq!(identity, identity_copy);

    let compiled = ItemVisualRoute::Compiled(ItemVisualId(9));
    let block = ItemVisualRoute::BlockItem(BlockVisualId(11));
    assert_eq!(compiled, ItemVisualRoute::Compiled(ItemVisualId(9)));
    assert_eq!(block, ItemVisualRoute::BlockItem(BlockVisualId(11)));
    assert_eq!(ItemVisualRoute::EmptyHand, ItemVisualRoute::EmptyHand);
    assert_eq!(ItemVisualRoute::Missing, ItemVisualRoute::Missing);

    let icon = ItemIconRef {
        asset_identity: [0xa5; 32],
        texture_page: 17,
        uv: [1, 2, 3, 4],
    };
    assert_eq!(icon, icon);
}

#[test]
fn empty_identity_has_one_exact_canonical_value() {
    let empty = ItemStackIdentity::empty();
    assert_eq!(
        empty,
        ItemStackIdentity {
            network_id: 0,
            metadata: 0,
            stack_network_id: -1,
            count: 0,
            nbt_digest: [0; 32],
            block_runtime_id: 0,
        }
    );
    assert!(empty.is_empty());
}

#[test]
fn zero_count_validation_canonicalizes_the_complete_identity() {
    let noncanonical = ItemStackIdentity {
        network_id: -99,
        metadata: u32::MAX,
        stack_network_id: 812,
        count: 0,
        nbt_digest: [0xff; 32],
        // A retained block identity on a zero-count stack is dropped with the
        // rest of the canonical empty value.
        block_runtime_id: 77,
    };

    let validated = noncanonical.validate().unwrap();
    assert_eq!(validated, ItemStackIdentity::empty());
    assert_eq!(validated.block_runtime_id, 0);
}

#[test]
fn validation_rejects_negative_network_ids_only_for_nonempty_stacks() {
    let invalid = ItemStackIdentity {
        network_id: -1,
        ..nonempty_identity(0)
    };
    assert!(invalid.validate().is_err());

    let valid = nonempty_identity(0);
    assert_eq!(valid.validate().unwrap(), valid);
    assert!(!valid.is_empty());
}

#[test]
fn metadata_remains_lossless_across_the_full_u32_range() {
    for metadata in [u32::MIN, 1, i32::MAX as u32 + 1, u32::MAX] {
        let identity = nonempty_identity(metadata);
        assert_eq!(identity.validate().unwrap().metadata, metadata);
    }
}

#[test]
fn retained_block_runtime_identity_is_bound_into_equality_and_hash() {
    let base = nonempty_identity(0);
    let mut differing_block = base;
    differing_block.block_runtime_id = 5;

    // Two stacks that differ only in their retained block runtime identity
    // must never collapse into one canonical identity.
    assert_ne!(base, differing_block);

    let mut distinct = std::collections::HashSet::new();
    assert!(distinct.insert(base));
    assert!(distinct.insert(differing_block));
    assert_eq!(distinct.len(), 2);
}

#[test]
fn block_runtime_identity_is_lossless_across_the_full_wire_range() {
    for block_runtime_id in [i32::MIN, -1, 0, 1, 92, i32::MAX] {
        let identity = ItemStackIdentity {
            block_runtime_id,
            ..nonempty_identity(4)
        };
        let validated = identity.validate().unwrap();
        assert_eq!(validated, identity);
        assert_eq!(validated.block_runtime_id, block_runtime_id);
    }
}

#[test]
fn ordinary_non_block_items_keep_the_previous_identity_contract() {
    let legacy_shape = ItemStackIdentity {
        network_id: 42,
        metadata: 7,
        stack_network_id: -1,
        count: 3,
        nbt_digest: [0x5a; 32],
        block_runtime_id: 0,
    };
    assert_eq!(legacy_shape, nonempty_identity(7));
    assert_eq!(legacy_shape.validate(), Ok(legacy_shape));
    assert!(!legacy_shape.is_empty());
}

#[test]
fn absent_stack_network_identity_uses_the_negative_one_sentinel() {
    let identity = nonempty_identity(0);
    let stack_network_id: i32 = identity.stack_network_id;
    assert_eq!(stack_network_id, -1);
    assert_eq!(identity.validate().unwrap(), identity);
}

#[test]
fn action_phase_contract_carries_exact_tick_progress() {
    assert_eq!(ItemActionPhase::Idle, ItemActionPhase::Idle);
    assert_eq!(
        ItemActionPhase::Windup { elapsed_ticks: 2 },
        ItemActionPhase::Windup { elapsed_ticks: 2 }
    );
    assert_eq!(
        ItemActionPhase::Active { elapsed_ticks: 3 },
        ItemActionPhase::Active { elapsed_ticks: 3 }
    );
    assert_eq!(
        ItemActionPhase::Recover { elapsed_ticks: 4 },
        ItemActionPhase::Recover { elapsed_ticks: 4 }
    );
    assert_eq!(
        ItemActionPhase::UseHeld {
            elapsed_ticks: 5,
            duration_ticks: 20,
        },
        ItemActionPhase::UseHeld {
            elapsed_ticks: 5,
            duration_ticks: 20,
        }
    );
    assert_eq!(ItemActionPhase::Cancelled, ItemActionPhase::Cancelled);
}

fn item_carrier_fixture() -> CompiledEntityAssetsV4 {
    CompiledEntityAssetsV4 {
        source_manifest_sha256: [0x31; 32],
        block_visual_count: 1,
        sources: vec![
            entity::EntityAssetSource {
                path: "entity/item_frame.entity.json".into(),
                source_bytes: 1,
                source_sha256: [0x31; 32],
            },
            entity::EntityAssetSource {
                path: "textures/item_texture.json".into(),
                source_bytes: 1,
                source_sha256: [0x32; 32],
            },
            entity::EntityAssetSource {
                path: "textures/items/apple.png".into(),
                source_bytes: 1,
                source_sha256: [0x33; 32],
            },
        ]
        .into_boxed_slice(),
        symbols: vec![entity::EntityAssetSymbol {
            kind: entity::EntityAssetKind::Entity,
            identifier: "minecraft:item_frame".into(),
            source_index: 0,
            dependencies: Box::new([]),
        }]
        .into_boxed_slice(),
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
        item_visuals: vec![ItemVisualDefinition {
            key: visual_key("minecraft:apple"),
            source: 1,
            route: ItemVisualDefinitionRoute::Sprite {
                texture: ItemTextureReference {
                    source: 2,
                    variant: 0,
                },
            },
            first_person: ItemDisplayTransform::identity(),
            third_person: ItemDisplayTransform::identity(),
            dropped: ItemDisplayTransform::identity(),
        }]
        .into_boxed_slice(),
        item_visual_aliases: vec![ItemVisualAlias {
            key: visual_key("minecraft:apple_alias"),
            visual: item::ItemVisualId(0),
        }]
        .into_boxed_slice(),
        render: Default::default(),
    }
}

const DEFAULT_BINDINGS: &[u8] = include_bytes!("../../data/default-sprite-bindings-1.26.50.json");

fn default_binding_fixture() -> CompiledEntityAssetsV4 {
    let mut compiled = item_carrier_fixture();
    compiled.sources[1] = entity::EntityAssetSource {
        path: "registry/default-sprite-bindings-1.26.50.json".into(),
        source_bytes: DEFAULT_BINDINGS.len() as u32,
        source_sha256: Sha256::digest(DEFAULT_BINDINGS).into(),
    };
    compiled
}

#[test]
fn reviewed_default_sprite_carrier_accepts_every_canonical_key_and_missing_route() {
    let table: serde_json::Value = serde_json::from_slice(DEFAULT_BINDINGS).unwrap();
    let routes = table["routes"].as_array().unwrap();
    assert!(!routes.is_empty());
    for route in routes {
        for missing in [false, true] {
            let mut compiled = default_binding_fixture();
            compiled.item_visuals[0].key = visual_key(route["identifier"].as_str().unwrap());
            if missing {
                compiled.item_visuals[0].route = ItemVisualDefinitionRoute::Missing;
            }
            compiled.validate().unwrap();
            let blob = entity::encode_entity_blob(&compiled).unwrap();
            let runtime = RuntimeEntityAssets::decode(&blob).unwrap();
            assert_eq!(runtime.item_visuals(), compiled.item_visuals.as_ref());
        }
    }
}

fn reject_default_binding_mutation(mutate: impl FnOnce(&mut CompiledEntityAssetsV4)) {
    let original = default_binding_fixture();
    let original_blob = entity::encode_entity_blob(&original).unwrap();
    let mut mutated = original.clone();
    mutate(&mut mutated);
    let expected_error = mutated.validate().unwrap_err().to_string();
    assert!(entity::encode_entity_blob(&mutated).is_err());

    // Preserve canonical field ordering while forging a valid checksum, so the
    // decoder must apply the same trust checks to untrusted carrier contents.
    let mut payload =
        String::from_utf8(original_blob[80..original_blob.len() - 32].to_vec()).unwrap();
    for (field, before, after) in [
        (
            "sources",
            serde_json::to_string(&original.sources).unwrap(),
            serde_json::to_string(&mutated.sources).unwrap(),
        ),
        (
            "item_visuals",
            serde_json::to_string(&original.item_visuals).unwrap(),
            serde_json::to_string(&mutated.item_visuals).unwrap(),
        ),
    ] {
        payload = payload.replacen(
            &format!("\"{field}\":{before}"),
            &format!("\"{field}\":{after}"),
            1,
        );
    }
    let mut blob = original_blob[..80].to_vec();
    blob[12..16].copy_from_slice(&(mutated.sources.len() as u32).to_le_bytes());
    blob[56..64].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    blob.extend_from_slice(payload.as_bytes());
    let digest = Sha256::digest(&blob);
    blob.extend_from_slice(&digest);
    assert_eq!(
        RuntimeEntityAssets::decode(&blob).unwrap_err().to_string(),
        expected_error
    );
}

#[test]
fn reviewed_default_sprite_carrier_rejects_source_identity_and_index_substitution() {
    reject_default_binding_mutation(|c| c.sources[1].source_sha256 = [0x45; 32]);
    reject_default_binding_mutation(|c| c.sources[1].source_bytes += 1);
    reject_default_binding_mutation(|c| {
        c.sources[1].path = "registry/default-sprite-bindings-1.26.51.json".into()
    });
    reject_default_binding_mutation(|c| c.sources[1].path = "registry/unreviewed.json".into());
    reject_default_binding_mutation(|c| {
        let mut sources = c.sources.to_vec();
        sources.insert(2, sources[1].clone());
        c.sources = sources.into_boxed_slice();
    });
    reject_default_binding_mutation(|c| c.item_visuals[0].source = 2);
    reject_default_binding_mutation(|c| {
        c.sources[1].source_sha256 = [0x45; 32];
        c.item_visuals[0].source = 0;
    });
}

#[test]
fn reviewed_default_sprite_carrier_rejects_unreviewed_keys_and_routes() {
    reject_default_binding_mutation(|c| c.item_visuals[0].key = visual_key("minecraft:unreviewed"));
    reject_default_binding_mutation(|c| c.item_visuals[0].key.metadata = 1);
    reject_default_binding_mutation(|c| {
        c.item_visuals[0].route = ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference {
                source: 2,
                variant: 1,
            },
        };
    });
    reject_default_binding_mutation(|c| {
        c.item_visuals[0].route = ItemVisualDefinitionRoute::BlockItem {
            block_visual: BlockVisualId(0),
        }
    });
    reject_default_binding_mutation(|c| {
        c.item_visuals[0].route = ItemVisualDefinitionRoute::EmptyHand
    });
    reject_default_binding_mutation(|c| {
        c.item_visuals[0].route = ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference {
                source: 0,
                variant: 0,
            },
        };
    });
}

#[test]
fn item_carrier_round_trips_typed_routes_defining_sources_and_aliases() {
    let mut compiled = item_carrier_fixture();
    compiled.item_visuals[0].first_person.translation = [
        ItemDisplayScalar::new(1.0).unwrap(),
        ItemDisplayScalar::new(2.0).unwrap(),
        ItemDisplayScalar::new(3.0).unwrap(),
    ];
    compiled.item_visuals[0].third_person.rotation[1] = ItemDisplayScalar::new(45.0).unwrap();
    compiled.item_visuals[0].dropped.scale = [ItemDisplayScalar::new(0.5).unwrap(); 3];

    let blob = entity::encode_entity_blob(&compiled).unwrap();
    let runtime = RuntimeEntityAssets::decode(&blob).unwrap();
    assert_eq!(runtime.item_visuals(), compiled.item_visuals.as_ref());
    assert_eq!(
        runtime.item_visual_aliases(),
        compiled.item_visual_aliases.as_ref()
    );
    assert_eq!(runtime.item_visuals()[0].source, 1);
    assert_eq!(
        runtime.item_visuals()[0].route,
        ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference {
                source: 2,
                variant: 0
            }
        }
    );
    assert_eq!(runtime.encode().unwrap().as_ref(), blob.as_ref());
    assert_eq!(
        entity::encode_entity_blob(&compiled).unwrap().as_ref(),
        blob.as_ref()
    );
}

#[test]
fn item_carrier_round_trips_all_four_exact_routes() {
    let mut compiled = item_carrier_fixture();
    let base = compiled.item_visuals[0].clone();
    compiled.item_visuals = vec![
        ItemVisualDefinition {
            key: visual_key("minecraft:air"),
            route: ItemVisualDefinitionRoute::EmptyHand,
            ..base.clone()
        },
        base.clone(),
        ItemVisualDefinition {
            key: visual_key("minecraft:missing"),
            route: ItemVisualDefinitionRoute::Missing,
            ..base.clone()
        },
        ItemVisualDefinition {
            key: visual_key("minecraft:stone"),
            route: ItemVisualDefinitionRoute::BlockItem {
                block_visual: BlockVisualId(0),
            },
            ..base
        },
    ]
    .into_boxed_slice();
    compiled.item_visual_aliases = Box::new([]);

    let blob = entity::encode_entity_blob(&compiled).unwrap();
    let runtime = RuntimeEntityAssets::decode(&blob).unwrap();
    assert_eq!(runtime.item_visuals(), compiled.item_visuals.as_ref());
    assert_eq!(runtime.encode().unwrap().as_ref(), blob.as_ref());
}

#[test]
fn item_carrier_accepts_exact_limits_and_rejects_limit_plus_one() {
    let mut compiled = item_carrier_fixture();
    compiled.item_visuals = (0..MAX_ITEM_VISUALS)
        .map(|index| ItemVisualDefinition {
            key: visual_key(format!("minecraft:item_{index:05}").into_boxed_str()),
            ..compiled.item_visuals[0].clone()
        })
        .collect();
    compiled.item_visual_aliases = Box::new([]);
    assert!(compiled.validate().is_ok());
    compiled.item_visuals = (0..=MAX_ITEM_VISUALS)
        .map(|index| ItemVisualDefinition {
            key: visual_key(format!("minecraft:item_{index:05}").into_boxed_str()),
            ..compiled.item_visuals[0].clone()
        })
        .collect();
    assert!(compiled.validate().is_err());

    let mut compiled = item_carrier_fixture();
    compiled.item_visual_aliases = (0..MAX_ITEM_VISUAL_ALIASES)
        .map(|index| ItemVisualAlias {
            key: visual_key(format!("minecraft:alias_{index:05}").into_boxed_str()),
            visual: item::ItemVisualId(0),
        })
        .collect();
    assert!(compiled.validate().is_ok());
    compiled.item_visual_aliases = (0..=MAX_ITEM_VISUAL_ALIASES)
        .map(|index| ItemVisualAlias {
            key: visual_key(format!("minecraft:alias_{index:05}").into_boxed_str()),
            visual: item::ItemVisualId(0),
        })
        .collect();
    assert!(compiled.validate().is_err());

    let mut compiled = item_carrier_fixture();
    compiled.item_visuals[0].key.identifier =
        "x".repeat(MAX_ITEM_IDENTIFIER_BYTES).into_boxed_str();
    assert!(compiled.validate().is_ok());
    compiled.item_visuals[0].key.identifier =
        "x".repeat(MAX_ITEM_IDENTIFIER_BYTES + 1).into_boxed_str();
    assert!(compiled.validate().is_err());
}

#[test]
fn item_carrier_requires_sorted_unique_identifiers_and_bounded_block_routes() {
    let mut compiled = item_carrier_fixture();
    compiled.item_visuals = vec![
        compiled.item_visuals[0].clone(),
        compiled.item_visuals[0].clone(),
    ]
    .into_boxed_slice();
    assert!(compiled.validate().is_err());

    let mut compiled = item_carrier_fixture();
    compiled.item_visual_aliases = vec![
        ItemVisualAlias {
            key: visual_key("minecraft:z"),
            visual: item::ItemVisualId(0),
        },
        ItemVisualAlias {
            key: visual_key("minecraft:a"),
            visual: item::ItemVisualId(0),
        },
    ]
    .into_boxed_slice();
    assert!(compiled.validate().is_err());

    let mut compiled = item_carrier_fixture();
    compiled.item_visuals[0].route = ItemVisualDefinitionRoute::BlockItem {
        block_visual: item::BlockVisualId(0),
    };
    assert!(compiled.validate().is_ok());
    compiled.item_visuals[0].route = ItemVisualDefinitionRoute::BlockItem {
        block_visual: item::BlockVisualId(1),
    };
    assert!(compiled.validate().is_err());
}

#[test]
fn item_carrier_rejects_collisions_across_visual_and_alias_namespaces() {
    let mut compiled = item_carrier_fixture();
    compiled.item_visual_aliases[0].key = compiled.item_visuals[0].key.clone();
    assert!(compiled.validate().is_err());
}

#[test]
fn item_carrier_rejects_nonfinite_transforms_and_out_of_range_indices() {
    assert!(ItemDisplayScalar::new(f32::NAN).is_none());
    assert!(ItemDisplayScalar::new(f32::INFINITY).is_none());
    assert_eq!(ItemDisplayScalar::new(-0.0).unwrap().bits(), 0);

    let mut compiled = item_carrier_fixture();
    compiled.item_visuals[0].source = 3;
    assert!(compiled.validate().is_err());

    let mut compiled = item_carrier_fixture();
    compiled.item_visuals[0].route = ItemVisualDefinitionRoute::Sprite {
        texture: ItemTextureReference {
            source: 3,
            variant: 0,
        },
    };
    assert!(compiled.validate().is_err());

    let mut compiled = item_carrier_fixture();
    compiled.item_visuals[0].route = ItemVisualDefinitionRoute::Sprite {
        texture: ItemTextureReference {
            source: 0,
            variant: 0,
        },
    };
    assert!(compiled.validate().is_err());

    let mut compiled = item_carrier_fixture();
    compiled.item_visuals[0].route = ItemVisualDefinitionRoute::Sprite {
        texture: ItemTextureReference {
            source: 1,
            variant: 0,
        },
    };
    assert!(compiled.validate().is_err());

    let mut compiled = item_carrier_fixture();
    compiled.item_visual_aliases[0].visual = item::ItemVisualId(1);
    assert!(compiled.validate().is_err());
}

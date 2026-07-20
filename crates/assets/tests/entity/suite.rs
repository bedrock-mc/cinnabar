use assets::{
    CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EntityAssetSymbol, EntityDependency,
    EntityDependencyKind, EntityDependencyResolution, EntityGeometry, EntityGeometryBone,
    EntityGeometryCube, EntityGeometryFaceUv, EntityGeometryFaceUvs, EntityGeometryInheritance,
    EntityGeometryScalar, EntityGeometryUv, RuntimeEntityAssets, encode_entity_blob,
};
use sha2::{Digest, Sha256};

use super::{entity, item};

use entity::{
    CompiledEntityAssets as CompiledEntityAssetsV4, CompiledMolangExpression,
    EntityAnimationChannel, EntityAnimationClip, EntityAnimationController,
    EntityAnimationInterpolation, EntityAnimationKeyframe, EntityAnimationLoop,
    EntityAnimationProperty, EntityControllerAnimation, EntityControllerState,
    EntityControllerTransition, EntityRigAnimationBinding, EntityRigBinding,
    EntityRigControllerBinding, EntityRigFallback, EntityRigGeometryBinding,
    MAX_ENTITY_ANIMATION_CHANNELS, MAX_ENTITY_ANIMATION_CLIPS, MAX_ENTITY_ANIMATION_KEYFRAMES,
    MAX_ENTITY_CONTROLLER_STATES, MAX_ENTITY_CONTROLLER_TRANSITIONS, MAX_ENTITY_CONTROLLERS,
    MAX_ENTITY_RIG_BINDINGS, MAX_MOLANG_COLLECTION_ITEMS, MAX_MOLANG_EXPRESSIONS,
    MAX_MOLANG_OPS_PER_EXPRESSION, MAX_MOLANG_STACK_DEPTH, MolangCollection, MolangCollectionItem,
    MolangOp, MolangSymbol, MolangSymbolKind, RuntimeEntityAssets as RuntimeEntityAssetsV4,
};
use item::{
    BlockVisualId as LeafBlockVisualId, ItemDisplayTransform, ItemTextureReference,
    ItemVisualAlias, ItemVisualDefinition, ItemVisualDefinitionRoute,
    ItemVisualId as LeafItemVisualId, ItemVisualKey,
};

fn fixture() -> CompiledEntityAssets {
    let source_bytes = br#"{"format_version":"1.10.0"}"#;
    CompiledEntityAssets {
        source_manifest_sha256: [0x11; 32],
        block_visual_count: 0,
        sources: vec![EntityAssetSource {
            path: "entity/allay.entity.json".into(),
            source_bytes: source_bytes.len() as u32,
            source_sha256: Sha256::digest(source_bytes).into(),
        }]
        .into_boxed_slice(),
        symbols: vec![EntityAssetSymbol {
            kind: EntityAssetKind::Entity,
            identifier: "minecraft:allay".into(),
            source_index: 0,
            dependencies: vec![
                EntityDependency {
                    kind: EntityDependencyKind::Geometry,
                    identifier: "geometry.allay".into(),
                    resolution: EntityDependencyResolution::External,
                },
                EntityDependency {
                    kind: EntityDependencyKind::Texture,
                    identifier: "textures/entity/allay/allay".into(),
                    resolution: EntityDependencyResolution::External,
                },
            ]
            .into_boxed_slice(),
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
        rig_textures: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
    }
}

fn scalar(value: f32) -> EntityGeometryScalar {
    EntityGeometryScalar::new(value).expect("finite bounded geometry scalar")
}

fn geometry_fixture() -> CompiledEntityAssets {
    let geometry_source = br#"{"format_version":"1.21.0"}"#;
    let mut compiled = fixture();
    compiled.sources = vec![
        compiled.sources[0].clone(),
        EntityAssetSource {
            path: "models/entity/allay.geo.json".into(),
            source_bytes: geometry_source.len() as u32,
            source_sha256: Sha256::digest(geometry_source).into(),
        },
    ]
    .into_boxed_slice();
    compiled.symbols[0].dependencies[0].resolution = EntityDependencyResolution::Catalog;
    compiled.symbols = vec![
        compiled.symbols[0].clone(),
        EntityAssetSymbol {
            kind: EntityAssetKind::Geometry,
            identifier: "geometry.allay".into(),
            source_index: 1,
            dependencies: Box::new([]),
        },
    ]
    .into_boxed_slice();
    compiled.geometries = vec![EntityGeometry {
        identifier: "geometry.allay".into(),
        inherits: None,
        source_index: 1,
        texture_width: 32,
        texture_height: 32,
        bones: vec![EntityGeometryBone {
            name: "wing".into(),
            parent: None,
            pivot: Some([scalar(0.5), scalar(4.0), scalar(1.0)]),
            rotation: Some([scalar(0.0), scalar(15.0), scalar(0.0)]),
            mirror: None,
            inflate: None,
            never_render: None,
            reset: None,
            cubes: vec![EntityGeometryCube {
                origin: [scalar(0.5), scalar(-1.0), scalar(1.0)],
                size: [scalar(0.0), scalar(5.0), scalar(8.0)],
                pivot: [scalar(0.5), scalar(4.0), scalar(1.0)],
                rotation: [scalar(0.0), scalar(0.0), scalar(-2.5)],
                uv: EntityGeometryUv::Box([scalar(16.0), scalar(14.0)]),
                inflate: scalar(-0.2),
                mirror: true,
            }]
            .into_boxed_slice(),
        }]
        .into_boxed_slice(),
    }]
    .into_boxed_slice();
    compiled
}

fn inherited_geometry_fixture() -> CompiledEntityAssets {
    let mut compiled = geometry_fixture();
    compiled.sources = vec![
        compiled.sources[0].clone(),
        compiled.sources[1].clone(),
        EntityAssetSource {
            path: "models/entity/z_derived.geo.json".into(),
            source_bytes: 1,
            source_sha256: [0x77; 32],
        },
    ]
    .into_boxed_slice();
    compiled.symbols = vec![
        compiled.symbols[0].clone(),
        compiled.symbols[1].clone(),
        EntityAssetSymbol {
            kind: EntityAssetKind::Geometry,
            identifier: "geometry.derived".into(),
            source_index: 2,
            dependencies: Box::new([]),
        },
    ]
    .into_boxed_slice();
    compiled.geometries = vec![
        compiled.geometries[0].clone(),
        EntityGeometry {
            identifier: "geometry.derived".into(),
            inherits: Some(EntityGeometryInheritance {
                identifier: "geometry.allay".into(),
                resolution: EntityDependencyResolution::Catalog,
            }),
            source_index: 2,
            texture_width: 32,
            texture_height: 32,
            bones: Box::new([]),
        },
    ]
    .into_boxed_slice();
    compiled
}

#[test]
fn entity_carrier_round_trips_sparse_per_face_uvs() {
    let mut compiled = geometry_fixture();
    compiled.geometries[0].bones[0].cubes[0].uv = EntityGeometryUv::Faces(EntityGeometryFaceUvs {
        east: Some(EntityGeometryFaceUv {
            uv: [scalar(0.0), scalar(5.0)],
            uv_size: Some([scalar(5.0), scalar(16.0)]),
        }),
        north: None,
        south: None,
        west: None,
        up: None,
        down: None,
    });

    let runtime = RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap();
    assert_eq!(runtime.geometries(), compiled.geometries.as_ref());
}

#[test]
fn entity_carrier_round_trips_sparse_inherited_bone_fields() {
    let mut compiled = geometry_fixture();
    let bone = &mut compiled.geometries[0].bones[0];
    bone.pivot = None;
    bone.rotation = None;
    bone.mirror = Some(true);
    bone.inflate = Some(scalar(1.0));
    bone.never_render = Some(false);
    bone.reset = Some(true);

    let runtime = RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap();
    assert_eq!(runtime.geometries(), compiled.geometries.as_ref());
}

#[test]
fn entity_carrier_rejects_transitive_geometry_inheritance_cycles() {
    let mut compiled = inherited_geometry_fixture();
    compiled.geometries[0].inherits = Some(EntityGeometryInheritance {
        identifier: "geometry.derived".into(),
        resolution: EntityDependencyResolution::Catalog,
    });

    assert!(encode_entity_blob(&compiled).is_err());
}

#[test]
fn entity_carrier_rejects_unresolved_inherited_bone_parents() {
    let mut compiled = inherited_geometry_fixture();
    compiled.geometries[1].bones = vec![EntityGeometryBone {
        name: "nose".into(),
        parent: Some("missing".into()),
        pivot: None,
        rotation: None,
        mirror: None,
        inflate: None,
        never_render: None,
        reset: None,
        cubes: Box::new([]),
    }]
    .into_boxed_slice();

    assert!(encode_entity_blob(&compiled).is_err());
}

#[test]
fn entity_carrier_round_trips_canonical_catalog_and_provenance() {
    let compiled = fixture();
    let first = encode_entity_blob(&compiled).expect("encode MCBEENT5");
    let second = encode_entity_blob(&compiled).expect("encode MCBEENT5 twice");
    assert_eq!(first, second);
    assert_eq!(&first[..8], b"MCBEENT3");

    let runtime = RuntimeEntityAssets::decode(&first).expect("decode MCBEENT5");
    assert_eq!(runtime.source_manifest_sha256(), [0x11; 32]);
    assert_eq!(runtime.sources(), compiled.sources.as_ref());
    assert_eq!(runtime.symbols(), compiled.symbols.as_ref());
    assert_eq!(
        runtime.symbol_candidates(EntityAssetKind::Entity, "minecraft:allay"),
        compiled.symbols.as_ref()
    );
}

#[test]
fn entity_carrier_round_trips_bounded_canonical_geometry_payloads() {
    let compiled = geometry_fixture();
    let blob = encode_entity_blob(&compiled).expect("encode geometry payload");
    assert_eq!(&blob[..8], b"MCBEENT3");

    let runtime = RuntimeEntityAssets::decode(&blob).expect("decode geometry payload");
    assert_eq!(runtime.geometries(), compiled.geometries.as_ref());
    assert_eq!(
        runtime.geometry_candidates("geometry.allay"),
        compiled.geometries.as_ref()
    );
    assert_eq!(
        runtime.geometries()[0].bones[0].cubes[0].inflate.get(),
        -0.2
    );
}

#[test]
fn geometry_candidates_preserve_same_identifier_from_distinct_sources() {
    let mut compiled = geometry_fixture();
    compiled.sources = vec![
        compiled.sources[0].clone(),
        compiled.sources[1].clone(),
        EntityAssetSource {
            path: "models/entity/z_allay.compat.geo.json".into(),
            source_bytes: 1,
            source_sha256: [0x66; 32],
        },
    ]
    .into_boxed_slice();
    let mut second_symbol = compiled.symbols[1].clone();
    second_symbol.source_index = 2;
    compiled.symbols = vec![
        compiled.symbols[0].clone(),
        compiled.symbols[1].clone(),
        second_symbol,
    ]
    .into_boxed_slice();
    let mut second_geometry = compiled.geometries[0].clone();
    second_geometry.source_index = 2;
    compiled.geometries = vec![compiled.geometries[0].clone(), second_geometry].into_boxed_slice();

    let runtime = RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap();
    let candidates = runtime.geometry_candidates("geometry.allay");
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].source_index, 1);
    assert_eq!(candidates[1].source_index, 2);
}

#[test]
fn carrier_rejects_geometry_without_exact_catalog_provenance_or_canonical_scalars() {
    let mut missing = geometry_fixture();
    missing.geometries = Box::new([]);
    assert!(encode_entity_blob(&missing).is_err());

    let mut extra = geometry_fixture();
    extra.geometries[0].source_index = 0;
    assert!(encode_entity_blob(&extra).is_err());

    assert!(EntityGeometryScalar::new(f32::NAN).is_none());
    assert!(EntityGeometryScalar::new(f32::INFINITY).is_none());
    assert_eq!(EntityGeometryScalar::new(-0.0).unwrap().bits(), 0);
}

#[test]
fn duplicate_symbol_lookup_returns_every_source_candidate() {
    let mut compiled = fixture();
    compiled.sources = vec![
        compiled.sources[0].clone(),
        EntityAssetSource {
            path: "entity/z_allay.compat.entity.json".into(),
            source_bytes: 1,
            source_sha256: [0x44; 32],
        },
    ]
    .into_boxed_slice();
    compiled.symbols = vec![
        compiled.symbols[0].clone(),
        EntityAssetSymbol {
            kind: EntityAssetKind::Entity,
            identifier: "minecraft:allay".into(),
            source_index: 1,
            dependencies: Box::new([]),
        },
    ]
    .into_boxed_slice();

    let blob = encode_entity_blob(&compiled).unwrap();
    let runtime = RuntimeEntityAssets::decode(&blob).unwrap();
    let candidates = runtime.symbol_candidates(EntityAssetKind::Entity, "minecraft:allay");
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].source_index, 0);
    assert_eq!(candidates[1].source_index, 1);
}

#[test]
fn carrier_rejects_dependency_resolution_that_disagrees_with_catalog() {
    let mut compiled = fixture();
    compiled.symbols[0].dependencies[0].resolution = EntityDependencyResolution::Catalog;
    assert!(encode_entity_blob(&compiled).is_err());

    compiled.sources = vec![
        compiled.sources[0].clone(),
        EntityAssetSource {
            path: "models/entity/allay.geo.json".into(),
            source_bytes: 1,
            source_sha256: [0x55; 32],
        },
    ]
    .into_boxed_slice();
    compiled.symbols = vec![
        compiled.symbols[0].clone(),
        EntityAssetSymbol {
            kind: EntityAssetKind::Geometry,
            identifier: "geometry.allay".into(),
            source_index: 1,
            dependencies: Box::new([]),
        },
    ]
    .into_boxed_slice();
    compiled.geometries = vec![EntityGeometry {
        identifier: "geometry.allay".into(),
        inherits: None,
        source_index: 1,
        texture_width: 16,
        texture_height: 16,
        bones: Box::new([]),
    }]
    .into_boxed_slice();
    encode_entity_blob(&compiled).expect("catalog dependency now resolves");
    compiled.symbols[0].dependencies[0].resolution = EntityDependencyResolution::External;
    assert!(encode_entity_blob(&compiled).is_err());
}

#[test]
fn entity_carrier_rejects_corruption_noncanonical_order_and_unbounded_strings() {
    let mut corrupt = encode_entity_blob(&fixture()).unwrap().into_vec();
    corrupt[64] ^= 0x80;
    assert!(RuntimeEntityAssets::decode(&corrupt).is_err());

    let mut unordered = fixture();
    unordered.sources = vec![
        EntityAssetSource {
            path: "entity/z.json".into(),
            source_bytes: 1,
            source_sha256: [1; 32],
        },
        EntityAssetSource {
            path: "entity/a.json".into(),
            source_bytes: 1,
            source_sha256: [2; 32],
        },
    ]
    .into_boxed_slice();
    unordered.symbols[0].source_index = 1;
    assert!(encode_entity_blob(&unordered).is_err());

    let mut oversized = fixture();
    oversized.symbols[0].identifier = "x".repeat(513).into_boxed_str();
    assert!(encode_entity_blob(&oversized).is_err());
}

#[test]
fn entity_rig_texture_rejects_bad_dimensions_provenance_hash_and_binding() {
    let mut valid = carrier_v4_fixture();
    let pixels = vec![31_u8; 8 * 4 * 4].into_boxed_slice();
    valid.rig_textures = vec![entity::EntityRigTexture {
        symbol: 5,
        source: 5,
        width: 8,
        height: 4,
        pixels_sha256: Sha256::digest(&pixels).into(),
        rgba8: pixels,
    }]
    .into_boxed_slice();
    valid.rig_bindings[0].default_texture = Some(0);
    encode_entity_blob(&valid).expect("bounded texture payload");

    let mut bad_dimensions = valid.clone();
    bad_dimensions.rig_textures[0].width = 0;
    assert!(encode_entity_blob(&bad_dimensions).is_err());

    let mut bad_hash = valid.clone();
    bad_hash.rig_textures[0].pixels_sha256[0] ^= 1;
    assert!(encode_entity_blob(&bad_hash).is_err());

    let mut bad_source = valid.clone();
    bad_source.rig_textures[0].source = 4;
    assert!(encode_entity_blob(&bad_source).is_err());

    let mut bad_binding = valid;
    bad_binding.rig_bindings[0].default_texture = Some(1);
    assert!(encode_entity_blob(&bad_binding).is_err());
}

fn identity_transform() -> ItemDisplayTransform {
    ItemDisplayTransform::identity()
}

pub(super) fn carrier_v4_fixture() -> CompiledEntityAssetsV4 {
    let sources = [
        ("animation_controllers/allay.controller.json", 0x10),
        ("animations/allay.animation.json", 0x11),
        ("entity/allay.entity.json", 0x12),
        ("models/entity/allay.geo.json", 0x13),
        ("render_controllers/allay.render.json", 0x14),
        ("textures/entity/allay.png", 0x15),
        ("textures/item_texture.json", 0x16),
        ("textures/items/allay_spawn_egg.png", 0x17),
    ]
    .into_iter()
    .map(|(path, digest)| entity::EntityAssetSource {
        path: path.into(),
        source_bytes: 1,
        source_sha256: [digest; 32],
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let symbols = [
        (entity::EntityAssetKind::Entity, "minecraft:allay", 2),
        (entity::EntityAssetKind::Geometry, "geometry.allay", 3),
        (
            entity::EntityAssetKind::Animation,
            "animation.allay.idle",
            1,
        ),
        (
            entity::EntityAssetKind::AnimationController,
            "controller.animation.allay",
            0,
        ),
        (
            entity::EntityAssetKind::RenderController,
            "controller.render.allay",
            4,
        ),
        (entity::EntityAssetKind::Texture, "textures/entity/allay", 5),
    ]
    .into_iter()
    .map(
        |(kind, identifier, source_index)| entity::EntityAssetSymbol {
            kind,
            identifier: identifier.into(),
            source_index,
            dependencies: Box::new([]),
        },
    )
    .collect::<Vec<_>>()
    .into_boxed_slice();

    CompiledEntityAssetsV4 {
        source_manifest_sha256: [0x21; 32],
        block_visual_count: 8,
        sources,
        symbols,
        geometries: vec![entity::EntityGeometry {
            identifier: "geometry.allay".into(),
            inherits: None,
            source_index: 3,
            texture_width: 32,
            texture_height: 32,
            bones: vec![entity::EntityGeometryBone {
                name: "root".into(),
                parent: None,
                pivot: None,
                rotation: None,
                mirror: None,
                inflate: None,
                never_render: None,
                reset: None,
                cubes: Box::new([]),
            }]
            .into_boxed_slice(),
        }]
        .into_boxed_slice(),
        animation_clips: vec![EntityAnimationClip {
            symbol: 2,
            length_seconds: entity::EntityGeometryScalar::new(1.0).unwrap(),
            loop_mode: EntityAnimationLoop::Loop,
            first_channel: 0,
            channel_count: 1,
            source: 1,
        }]
        .into_boxed_slice(),
        animation_channels: vec![EntityAnimationChannel {
            bone: 0,
            property: EntityAnimationProperty::Rotation,
            first_keyframe: 0,
            keyframe_count: 1,
        }]
        .into_boxed_slice(),
        animation_keyframes: vec![EntityAnimationKeyframe {
            time_seconds: entity::EntityGeometryScalar::new(0.0).unwrap(),
            value: [
                entity::EntityGeometryScalar::new(0.0).unwrap(),
                entity::EntityGeometryScalar::new(15.0).unwrap(),
                entity::EntityGeometryScalar::new(0.0).unwrap(),
            ],
            interpolation: EntityAnimationInterpolation::Linear,
        }]
        .into_boxed_slice(),
        molang_symbols: vec![
            MolangSymbol {
                kind: MolangSymbolKind::Name,
                identifier: "default".into(),
            },
            MolangSymbol {
                kind: MolangSymbolKind::Query,
                identifier: "query.anim_time".into(),
            },
            MolangSymbol {
                kind: MolangSymbolKind::Variable,
                identifier: "variable.speed".into(),
            },
            MolangSymbol {
                kind: MolangSymbolKind::Temporary,
                identifier: "temp.scratch".into(),
            },
        ]
        .into_boxed_slice(),
        molang_expressions: vec![CompiledMolangExpression {
            first_op: 0,
            op_count: 1,
            max_stack: 1,
        }]
        .into_boxed_slice(),
        molang_ops: vec![MolangOp::Push(
            entity::EntityGeometryScalar::new(1.0).unwrap(),
        )]
        .into_boxed_slice(),
        molang_collections: vec![MolangCollection {
            first_item: 0,
            item_count: 1,
        }]
        .into_boxed_slice(),
        molang_collection_items: vec![MolangCollectionItem {
            value: entity::EntityGeometryScalar::new(0.0).unwrap(),
        }]
        .into_boxed_slice(),
        controllers: vec![EntityAnimationController {
            symbol: 3,
            first_state: 0,
            state_count: 1,
            initial_state: 0,
        }]
        .into_boxed_slice(),
        controller_states: vec![EntityControllerState {
            name: 0,
            first_animation: 0,
            animation_count: 1,
            first_transition: 0,
            transition_count: 1,
            on_entry: Some(0),
            on_exit: None,
        }]
        .into_boxed_slice(),
        controller_animations: vec![EntityControllerAnimation {
            clip: 0,
            weight: Some(0),
        }]
        .into_boxed_slice(),
        controller_transitions: vec![EntityControllerTransition {
            target_state: 0,
            condition: 0,
        }]
        .into_boxed_slice(),
        rig_bindings: vec![EntityRigBinding {
            entity_symbol: 0,
            render_controller: 4,
            first_geometry: 0,
            geometry_count: 1,
            default_texture: None,
            fallback: EntityRigFallback::GeometryOnly,
        }]
        .into_boxed_slice(),
        rig_geometries: vec![EntityRigGeometryBinding {
            geometry: 0,
            condition: None,
            first_animation: 0,
            animation_count: 1,
            first_controller: 0,
            controller_count: 1,
        }]
        .into_boxed_slice(),
        rig_animations: vec![EntityRigAnimationBinding { name: 0, clip: 0 }].into_boxed_slice(),
        rig_controllers: vec![EntityRigControllerBinding {
            name: 0,
            controller: 0,
        }]
        .into_boxed_slice(),
        rig_textures: Box::new([]),
        item_visuals: vec![ItemVisualDefinition {
            key: ItemVisualKey {
                identifier: "minecraft:allay_spawn_egg".into(),
                metadata: 0,
            },
            source: 6,
            route: ItemVisualDefinitionRoute::BlockItem {
                block_visual: LeafBlockVisualId(7),
            },
            first_person: identity_transform(),
            third_person: identity_transform(),
            dropped: identity_transform(),
        }]
        .into_boxed_slice(),
        item_visual_aliases: vec![ItemVisualAlias {
            key: ItemVisualKey {
                identifier: "minecraft:allay_spawn_egg_alias".into(),
                metadata: 0,
            },
            visual: LeafItemVisualId(0),
        }]
        .into_boxed_slice(),
    }
}

#[test]
fn carrier_v5_round_trips_every_extended_section_byte_identically() {
    let compiled = carrier_v4_fixture();
    let encoded = entity::encode_entity_blob(&compiled).expect("encode version-5 carrier");
    assert_eq!(&encoded[..8], b"MCBEENT3");
    assert_eq!(u32::from_le_bytes(encoded[8..12].try_into().unwrap()), 5);

    let runtime = RuntimeEntityAssetsV4::decode(&encoded).expect("decode version-4 carrier");
    assert_eq!(runtime.animation_clips(), compiled.animation_clips.as_ref());
    assert_eq!(runtime.controllers(), compiled.controllers.as_ref());
    assert_eq!(runtime.rig_bindings(), compiled.rig_bindings.as_ref());
    assert_eq!(runtime.item_visuals(), compiled.item_visuals.as_ref());
    assert_eq!(runtime.summary().animation_keyframes, 1);
    assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_ref());
}

#[test]
fn carrier_v5_rejects_versions_three_and_four_and_hashes_extended_payload() {
    let encoded = entity::encode_entity_blob(&carrier_v4_fixture()).unwrap();
    for version in [3_u32, 4] {
        let mut wrong = encoded.to_vec();
        wrong[8..12].copy_from_slice(&version.to_le_bytes());
        assert!(RuntimeEntityAssetsV4::decode(&wrong).is_err());
    }

    let mut corrupt = encoded.to_vec();
    let extended_field = br#""max_stack":1"#;
    let field_start = corrupt[80..corrupt.len() - 32]
        .windows(extended_field.len())
        .position(|window| window == extended_field)
        .map(|offset| 80 + offset)
        .expect("extended JSON contains the Molang stack field");
    corrupt[field_start + extended_field.len() - 1] = b'2';
    let error = RuntimeEntityAssetsV4::decode(&corrupt).unwrap_err();
    assert!(error.to_string().contains("envelope hash mismatch"));
}

#[test]
fn carrier_v4_header_bounds_precede_hash_or_payload_allocation_and_counts_match() {
    let encoded = entity::encode_entity_blob(&carrier_v4_fixture()).unwrap();
    let mut excessive = encoded.to_vec();
    excessive[64..68].copy_from_slice(&(MAX_ENTITY_ANIMATION_CLIPS as u32 + 1).to_le_bytes());
    let error = RuntimeEntityAssetsV4::decode(&excessive).unwrap_err();
    assert!(error.to_string().contains("header counts exceed bounds"));

    let mut mismatch = encoded.to_vec();
    mismatch[64..68].copy_from_slice(&0_u32.to_le_bytes());
    let payload_end = mismatch.len() - 32;
    let digest = Sha256::digest(&mismatch[..payload_end]);
    mismatch[payload_end..].copy_from_slice(&digest);
    let error = RuntimeEntityAssetsV4::decode(&mismatch).unwrap_err();
    assert!(error.to_string().contains("counts do not match header"));
}

#[test]
fn current_entity_carrier_diagnostics_name_schema_five() {
    let error = RuntimeEntityAssets::decode(&[]).unwrap_err();
    assert!(error.to_string().contains("MCBEENT5"));
    assert!(!error.to_string().contains("MCBEENT4"));
}

#[test]
fn carrier_v4_accepts_exact_animation_and_controller_bounds() {
    let mut compiled = carrier_v4_fixture();
    compiled.animation_clips = (0..MAX_ENTITY_ANIMATION_CLIPS)
        .map(|index| EntityAnimationClip {
            first_channel: u32::from(index != 0),
            channel_count: u32::from(index == 0),
            ..compiled.animation_clips[0]
        })
        .collect();
    assert!(compiled.validate().is_ok());

    compiled = carrier_v4_fixture();
    compiled.animation_channels = (0..MAX_ENTITY_ANIMATION_CHANNELS)
        .map(|index| EntityAnimationChannel {
            first_keyframe: u32::from(index != 0),
            keyframe_count: u32::from(index == 0),
            ..compiled.animation_channels[0]
        })
        .collect();
    compiled.animation_clips[0].channel_count = MAX_ENTITY_ANIMATION_CHANNELS as u32;
    assert!(compiled.validate().is_ok());

    compiled = carrier_v4_fixture();
    compiled.animation_keyframes =
        vec![compiled.animation_keyframes[0]; MAX_ENTITY_ANIMATION_KEYFRAMES].into_boxed_slice();
    compiled.animation_channels[0].keyframe_count = MAX_ENTITY_ANIMATION_KEYFRAMES as u32;
    assert!(compiled.validate().is_ok());

    compiled = carrier_v4_fixture();
    let state = compiled.controller_states[0];
    compiled.controller_states = (0..MAX_ENTITY_CONTROLLERS)
        .map(|index| EntityControllerState {
            first_animation: u32::from(index != 0),
            animation_count: u16::from(index == 0),
            first_transition: u32::from(index != 0),
            transition_count: u16::from(index == 0),
            ..state
        })
        .collect();
    compiled.controllers = (0..MAX_ENTITY_CONTROLLERS)
        .map(|index| EntityAnimationController {
            first_state: index as u32,
            ..compiled.controllers[0]
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    assert!(compiled.validate().is_ok());

    compiled = carrier_v4_fixture();
    compiled.controller_states = (0..MAX_ENTITY_CONTROLLER_STATES)
        .map(|index| EntityControllerState {
            first_animation: u32::from(index != 0),
            animation_count: u16::from(index == 0),
            first_transition: u32::from(index != 0),
            transition_count: u16::from(index == 0),
            ..compiled.controller_states[0]
        })
        .collect();
    compiled.controllers[0].state_count = MAX_ENTITY_CONTROLLER_STATES as u16;
    assert!(compiled.validate().is_ok());

    compiled = carrier_v4_fixture();
    compiled.controller_transitions =
        vec![compiled.controller_transitions[0]; MAX_ENTITY_CONTROLLER_TRANSITIONS]
            .into_boxed_slice();
    compiled.controller_states[0].transition_count = MAX_ENTITY_CONTROLLER_TRANSITIONS as u16;
    assert!(compiled.validate().is_ok());
}

#[test]
fn carrier_v4_rejects_animation_and_controller_limits_plus_one() {
    let mut cases = Vec::new();
    let mut compiled = carrier_v4_fixture();
    compiled.animation_clips =
        vec![compiled.animation_clips[0]; MAX_ENTITY_ANIMATION_CLIPS + 1].into_boxed_slice();
    cases.push(compiled);

    let mut compiled = carrier_v4_fixture();
    compiled.animation_channels =
        vec![compiled.animation_channels[0]; MAX_ENTITY_ANIMATION_CHANNELS + 1].into_boxed_slice();
    cases.push(compiled);

    let mut compiled = carrier_v4_fixture();
    compiled.animation_keyframes =
        vec![compiled.animation_keyframes[0]; MAX_ENTITY_ANIMATION_KEYFRAMES + 1]
            .into_boxed_slice();
    cases.push(compiled);

    let mut compiled = carrier_v4_fixture();
    compiled.controllers =
        vec![compiled.controllers[0]; MAX_ENTITY_CONTROLLERS + 1].into_boxed_slice();
    cases.push(compiled);

    let mut compiled = carrier_v4_fixture();
    compiled.controller_states =
        vec![compiled.controller_states[0]; MAX_ENTITY_CONTROLLER_STATES + 1].into_boxed_slice();
    cases.push(compiled);

    let mut compiled = carrier_v4_fixture();
    compiled.controller_transitions =
        vec![compiled.controller_transitions[0]; MAX_ENTITY_CONTROLLER_TRANSITIONS + 1]
            .into_boxed_slice();
    cases.push(compiled);

    for compiled in cases {
        assert!(compiled.validate().is_err());
    }
}

#[test]
fn carrier_v4_enforces_molang_and_rig_bounds_and_all_indices() {
    let mut compiled = carrier_v4_fixture();
    compiled.molang_ops = vec![compiled.molang_ops[0]; MAX_MOLANG_EXPRESSIONS].into_boxed_slice();
    compiled.molang_expressions = (0..MAX_MOLANG_EXPRESSIONS)
        .map(|index| CompiledMolangExpression {
            first_op: index as u32,
            op_count: 1,
            max_stack: 1,
        })
        .collect();
    assert!(compiled.validate().is_ok());
    compiled.molang_expressions =
        vec![compiled.molang_expressions[0]; MAX_MOLANG_EXPRESSIONS + 1].into_boxed_slice();
    assert!(compiled.validate().is_err());

    let mut compiled = carrier_v4_fixture();
    compiled.molang_ops = std::iter::once(compiled.molang_ops[0])
        .chain((0..127).flat_map(|_| [compiled.molang_ops[0], MolangOp::Add]))
        .chain(std::iter::once(MolangOp::Abs))
        .collect();
    compiled.molang_expressions[0].op_count = MAX_MOLANG_OPS_PER_EXPRESSION as u16;
    compiled.molang_expressions[0].max_stack = 2;
    assert!(compiled.validate().is_ok());
    compiled.molang_expressions[0].max_stack = MAX_MOLANG_STACK_DEPTH + 1;
    assert!(compiled.validate().is_err());
    compiled.molang_expressions[0].max_stack = 2;
    compiled.molang_expressions[0].op_count = MAX_MOLANG_OPS_PER_EXPRESSION as u16 + 1;
    assert!(compiled.validate().is_err());

    let mut compiled = carrier_v4_fixture();
    compiled.molang_ops =
        vec![compiled.molang_ops[0]; MAX_MOLANG_STACK_DEPTH as usize + 1].into_boxed_slice();
    compiled.molang_expressions[0].op_count = MAX_MOLANG_STACK_DEPTH as u16 + 1;
    compiled.molang_expressions[0].max_stack = MAX_MOLANG_STACK_DEPTH;
    assert!(compiled.validate().is_err());

    let mut compiled = carrier_v4_fixture();
    compiled.controller_animations = vec![compiled.controller_animations[0]; 33].into_boxed_slice();
    compiled.controller_states[0].animation_count = 33;
    assert!(compiled.validate().is_ok());
    compiled = carrier_v4_fixture();
    compiled.molang_collection_items =
        vec![compiled.molang_collection_items[0]; MAX_MOLANG_COLLECTION_ITEMS].into_boxed_slice();
    compiled.molang_collections[0].item_count = MAX_MOLANG_COLLECTION_ITEMS as u8;
    assert!(compiled.validate().is_ok());
    compiled.molang_collection_items =
        vec![compiled.molang_collection_items[0]; MAX_MOLANG_COLLECTION_ITEMS + 1]
            .into_boxed_slice();
    compiled.molang_collections[0].item_count = (MAX_MOLANG_COLLECTION_ITEMS + 1) as u8;
    assert!(compiled.validate().is_err());

    let mut compiled = carrier_v4_fixture();
    compiled.rig_geometries = (0..MAX_ENTITY_RIG_BINDINGS)
        .map(|index| EntityRigGeometryBinding {
            geometry: 0,
            condition: None,
            first_animation: u32::from(index != 0),
            animation_count: u16::from(index == 0),
            first_controller: u32::from(index != 0),
            controller_count: u16::from(index == 0),
        })
        .collect();
    compiled.rig_bindings = (0..MAX_ENTITY_RIG_BINDINGS)
        .map(|index| EntityRigBinding {
            first_geometry: index as u32,
            geometry_count: 1,
            ..compiled.rig_bindings[0]
        })
        .collect();
    assert!(compiled.validate().is_ok());
    compiled.rig_bindings =
        vec![compiled.rig_bindings[0]; MAX_ENTITY_RIG_BINDINGS + 1].into_boxed_slice();
    assert!(compiled.validate().is_err());

    let mut compiled = carrier_v4_fixture();
    compiled.rig_geometries[0].geometry = u32::MAX;
    assert!(compiled.validate().is_err());
    let mut compiled = carrier_v4_fixture();
    compiled.controller_transitions[0].condition = u32::MAX;
    assert!(compiled.validate().is_err());
    let mut compiled = carrier_v4_fixture();
    compiled.animation_keyframes[0].value[0] =
        serde_json::from_str(&f32::NAN.to_bits().to_string()).unwrap();
    assert!(compiled.validate().is_err());
}

#[test]
fn carrier_v4_represents_task3_fixed_arity_math_and_collection_selection() {
    let unary = [
        MolangOp::Abs,
        MolangOp::Ceil,
        MolangOp::Floor,
        MolangOp::Round,
        MolangOp::Sqrt,
        MolangOp::Sin,
        MolangOp::Cos,
        MolangOp::SelectCollection(0),
    ];
    for operation in unary {
        let mut compiled = carrier_v4_fixture();
        compiled.molang_ops = vec![
            MolangOp::Push(entity::EntityGeometryScalar::new(1.0).unwrap()),
            operation,
        ]
        .into_boxed_slice();
        compiled.molang_expressions[0].op_count = 2;
        assert!(compiled.validate().is_ok(), "unary operation {operation:?}");
    }

    for operation in [MolangOp::Modulo, MolangOp::Min, MolangOp::Max] {
        let mut compiled = carrier_v4_fixture();
        compiled.molang_ops = vec![
            MolangOp::Push(entity::EntityGeometryScalar::new(1.0).unwrap()),
            MolangOp::Push(entity::EntityGeometryScalar::new(2.0).unwrap()),
            operation,
        ]
        .into_boxed_slice();
        compiled.molang_expressions[0].op_count = 3;
        compiled.molang_expressions[0].max_stack = 2;
        assert!(
            compiled.validate().is_ok(),
            "binary operation {operation:?}"
        );
    }

    for operation in [MolangOp::Clamp, MolangOp::Lerp] {
        let mut compiled = carrier_v4_fixture();
        compiled.molang_ops = vec![
            MolangOp::Push(entity::EntityGeometryScalar::new(1.0).unwrap()),
            MolangOp::Push(entity::EntityGeometryScalar::new(2.0).unwrap()),
            MolangOp::Push(entity::EntityGeometryScalar::new(3.0).unwrap()),
            operation,
        ]
        .into_boxed_slice();
        compiled.molang_expressions[0].op_count = 4;
        compiled.molang_expressions[0].max_stack = 3;
        assert!(
            compiled.validate().is_ok(),
            "ternary operation {operation:?}"
        );
    }
}

#[test]
fn carrier_v4_requires_exact_valid_molang_program_stack_contracts() {
    let mut zero = carrier_v4_fixture();
    zero.molang_expressions[0] = CompiledMolangExpression {
        first_op: 0,
        op_count: 0,
        max_stack: 0,
    };
    zero.molang_ops = Box::new([]);
    assert!(zero.validate().is_err());

    let mut underflow = carrier_v4_fixture();
    underflow.molang_ops = vec![MolangOp::Add].into_boxed_slice();
    underflow.molang_expressions[0].max_stack = 0;
    assert!(underflow.validate().is_err());

    let mut final_two = carrier_v4_fixture();
    final_two.molang_ops = vec![final_two.molang_ops[0]; 2].into_boxed_slice();
    final_two.molang_expressions[0].op_count = 2;
    final_two.molang_expressions[0].max_stack = 2;
    assert!(final_two.validate().is_err());

    let mut dishonest = carrier_v4_fixture();
    dishonest.molang_expressions[0].max_stack = 2;
    assert!(dishonest.validate().is_err());

    let mut exact_depth = carrier_v4_fixture();
    exact_depth.molang_ops = std::iter::repeat_n(exact_depth.molang_ops[0], 32)
        .chain(std::iter::repeat_n(MolangOp::Add, 31))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    exact_depth.molang_expressions[0].op_count = 63;
    exact_depth.molang_expressions[0].max_stack = 32;
    assert!(exact_depth.validate().is_ok());

    let mut dishonest_depth = exact_depth;
    dishonest_depth.molang_expressions[0].max_stack = 31;
    assert!(dishonest_depth.validate().is_err());
}

#[test]
fn carrier_v4_round_trips_selectable_geometry_metadata_and_texture_variant() {
    let mut compiled = carrier_v4_fixture();
    compiled.molang_ops = vec![
        MolangOp::Push(entity::EntityGeometryScalar::new(0.0).unwrap()),
        MolangOp::Push(entity::EntityGeometryScalar::new(0.0).unwrap()),
        MolangOp::Equal,
    ]
    .into_boxed_slice();
    compiled.molang_expressions[0].op_count = 3;
    compiled.molang_expressions[0].max_stack = 2;
    compiled.rig_bindings[0].geometry_count = 2;
    compiled.rig_geometries = vec![
        compiled.rig_geometries[0],
        EntityRigGeometryBinding {
            geometry: 0,
            condition: Some(0),
            first_animation: 1,
            animation_count: 0,
            first_controller: 1,
            controller_count: 0,
        },
    ]
    .into_boxed_slice();
    compiled.item_visuals[0].key = ItemVisualKey {
        identifier: "minecraft:allay_spawn_egg".into(),
        metadata: u32::MAX,
    };
    compiled.item_visuals[0].route = ItemVisualDefinitionRoute::Sprite {
        texture: ItemTextureReference {
            source: 7,
            variant: 7,
        },
    };
    compiled.item_visual_aliases[0].key = ItemVisualKey {
        identifier: "minecraft:allay_spawn_egg_alias".into(),
        metadata: u32::MAX,
    };

    let bytes = encode_entity_blob(&compiled).unwrap();
    let runtime = RuntimeEntityAssetsV4::decode(&bytes).unwrap();
    assert_eq!(runtime.rig_bindings()[0].geometry_count, 2);
    assert_eq!(runtime.rig_geometries()[1].condition, Some(0));
    assert_eq!(runtime.item_visuals()[0].key.metadata, u32::MAX);
    assert!(matches!(
        runtime.item_visuals()[0].route,
        ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference {
                source: 7,
                variant: 7
            }
        }
    ));
    assert_eq!(runtime.item_visual_aliases()[0].key.metadata, u32::MAX);
    assert_eq!(runtime.encode().unwrap().as_ref(), bytes.as_ref());
}

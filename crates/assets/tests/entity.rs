use assets::{
    CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EntityAssetSymbol, EntityDependency,
    EntityDependencyKind, EntityDependencyResolution, EntityGeometry, EntityGeometryBone,
    EntityGeometryCube, EntityGeometryFaceUv, EntityGeometryFaceUvs, EntityGeometryScalar,
    EntityGeometryUv, RuntimeEntityAssets, encode_entity_blob,
};
use sha2::{Digest, Sha256};

fn fixture() -> CompiledEntityAssets {
    let source_bytes = br#"{"format_version":"1.10.0"}"#;
    CompiledEntityAssets {
        source_manifest_sha256: [0x11; 32],
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
            pivot: [scalar(0.5), scalar(4.0), scalar(1.0)],
            rotation: [scalar(0.0), scalar(15.0), scalar(0.0)],
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
fn entity_carrier_round_trips_canonical_catalog_and_provenance() {
    let compiled = fixture();
    let first = encode_entity_blob(&compiled).expect("encode MCBEENT3");
    let second = encode_entity_blob(&compiled).expect("encode MCBEENT3 twice");
    assert_eq!(first, second);
    assert_eq!(&first[..8], b"MCBEENT3");

    let runtime = RuntimeEntityAssets::decode(&first).expect("decode MCBEENT3");
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

use assets::{
    CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EntityAssetSymbol, EntityDependency,
    EntityDependencyKind, RuntimeEntityAssets, encode_entity_blob,
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
                },
                EntityDependency {
                    kind: EntityDependencyKind::Texture,
                    identifier: "textures/entity/allay/allay".into(),
                },
            ]
            .into_boxed_slice(),
        }]
        .into_boxed_slice(),
    }
}

#[test]
fn entity_carrier_round_trips_canonical_catalog_and_provenance() {
    let compiled = fixture();
    let first = encode_entity_blob(&compiled).expect("encode MCBEENT1");
    let second = encode_entity_blob(&compiled).expect("encode MCBEENT1 twice");
    assert_eq!(first, second);
    assert_eq!(&first[..8], b"MCBEENT1");

    let runtime = RuntimeEntityAssets::decode(&first).expect("decode MCBEENT1");
    assert_eq!(runtime.source_manifest_sha256(), [0x11; 32]);
    assert_eq!(runtime.sources(), compiled.sources.as_ref());
    assert_eq!(runtime.symbols(), compiled.symbols.as_ref());
    assert_eq!(
        runtime.symbol(EntityAssetKind::Entity, "minecraft:allay"),
        compiled.symbols.first()
    );
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

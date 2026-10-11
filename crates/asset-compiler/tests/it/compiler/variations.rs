use {
    super::support::*,
    assets::{BlobProvenance, BlockFace, BlockFlags, RuntimeAssets, encode_blob},
    tempfile::TempDir,
};

#[test]
fn weighted_paths_survive_compilation_and_carrier_round_trip() {
    let directory = TempDir::new().unwrap();
    write_pack(
        directory.path(),
        r#"{"stone":{"textures":"stone"}}"#,
        r#"{"texture_data":{"stone":{"textures":{"variations":[{"path":"textures/blocks/a","weight":1},{"path":"textures/blocks/b","weight":3}]}}}}"#,
        "[]",
    );
    for (name, color) in [("a", [255, 0, 0, 255]), ("b", [0, 255, 0, 255])] {
        write_png(
            directory.path(),
            &format!("textures/blocks/{name}"),
            16,
            16,
            &solid(16, 16, color),
        );
    }
    let records = [record(
        0,
        10,
        "minecraft:stone",
        "{}",
        BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
    )];
    let mut compiled = compile_pack(directory.path(), &records).unwrap();
    let id = compiled.visuals[0].faces[BlockFace::Up as usize];
    let selector = compiled.materials[id as usize];
    assert_eq!(selector.variation_count, 2);
    let first = compiled.materials[selector.variation_start as usize];
    let second = compiled.materials[selector.variation_start as usize + 1];
    assert_ne!(first.texture, second.texture);
    assert_eq!(f32::from_bits(first.variation_weight), 0.25);
    assert_eq!(f32::from_bits(second.variation_weight), 0.75);
    compiled.provenance = BlobProvenance {
        source_manifest_sha256: [1; 32],
        block_registry_sha256: [2; 32],
        light_registry_sha256: [3; 32],
        biome_registry_sha256: [4; 32],
    };
    let runtime = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    assert_eq!(runtime.materials(), &*compiled.materials);
    compiled.materials[id as usize].variation_start = u32::MAX;
    assert!(encode_blob(&compiled).is_err());
}

#[test]
fn nested_variations_do_not_mix_pools_from_distinct_block_states() {
    let directory = TempDir::new().unwrap();
    let mut states = vec![serde_json::json!("textures/blocks/a"); 16];
    for (index, alternate) in ["b", "c"].into_iter().enumerate() {
        states[index] = serde_json::json!({"variations": [
            {"path": "textures/blocks/a", "weight": 1},
            {"path": format!("textures/blocks/{alternate}"), "weight": 3}
        ]});
    }
    write_pack(
        directory.path(),
        r#"{"brown_mushroom_block":{"textures":"mushroom_brown_top"}}"#,
        &serde_json::json!({"texture_data": {
            "mushroom_brown_top": {"textures": states}
        }})
        .to_string(),
        "[]",
    );
    for (name, color) in [
        ("a", [255, 0, 0, 255]),
        ("b", [0, 255, 0, 255]),
        ("c", [0, 0, 255, 255]),
    ] {
        write_png(
            directory.path(),
            &format!("textures/blocks/{name}"),
            16,
            16,
            &solid(16, 16, color),
        );
    }
    let records = (0..2)
        .map(|index| {
            record(
                index,
                10 + index,
                "minecraft:brown_mushroom_block",
                &format!(r#"{{"huge_mushroom_bits":{{"type":"int","value":{index}}}}}"#),
                BlockFlags::CUBE_GEOMETRY,
            )
        })
        .collect::<Vec<_>>();
    let compiled = compile_pack(directory.path(), &records).unwrap();
    let selectors = [0, 1].map(|index| material_for_face(&compiled, index, BlockFace::Up));
    assert!(
        selectors
            .iter()
            .all(|selector| selector.variation_count == 2)
    );
    assert_ne!(selectors[0].variation_start, selectors[1].variation_start);
    let leaves = selectors.map(|selector| {
        let start = selector.variation_start as usize;
        &compiled.materials[start..start + selector.variation_count as usize]
    });
    assert_eq!(leaves[0][0].texture, leaves[1][0].texture);
    assert_ne!(leaves[0][1].texture, leaves[1][1].texture);
}

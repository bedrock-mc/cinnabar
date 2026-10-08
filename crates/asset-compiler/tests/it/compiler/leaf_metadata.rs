use super::support::*;

pub(super) fn assert_agnostic_world_copy(compiled: &CompiledAssets, leaf: usize) {
    let visual = &compiled.visuals[leaf];
    assert_ne!(
        visual.variant & assets::BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF,
        0
    );
    let base = (visual.variant & assets::BLOCK_VISUAL_VARIANT_MATERIAL_MASK) as usize;
    for (offset, world) in compiled.materials
        [base..base + assets::SEASONAL_LEAF_MATERIAL_COUNT as usize]
        .iter()
        .enumerate()
    {
        let carried = compiled.materials[visual.faces[offset % BlockFace::ALL.len()] as usize];
        let deep = offset >= assets::SEASONAL_LEAF_DEEP_OFFSET as usize;
        assert_ne!(world.flags & assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR, 0);
        assert_eq!(world.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0, !deep);
        assert_eq!(world.flags & assets::MATERIAL_FLAG_TWO_SIDED != 0, !deep);
        assert_eq!(world.flags & MATERIAL_FLAG_ALPHA_BLEND, 0);
        assert_eq!(world.flags & assets::MATERIAL_FLAG_SEASONAL_FOLIAGE, 0);
        assert_eq!(carried.flags & assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR, 0);
        assert_ne!(world.texture, carried.texture);
        assert_eq!(
            mip_layer(compiled, 0, world.texture.layer()),
            mip_layer(compiled, 0, carried.texture.layer()),
            "native mip copies must preserve the authored base pixels"
        );
    }
}

fn fixture(blocks: &str, terrain: &str) -> TempDir {
    let directory = TempDir::new().unwrap();
    write_pack(directory.path(), blocks, terrain, "[]");
    for (name, colour) in [
        ("leaves", [80, 160, 40, 255]),
        ("alternate", [30, 60, 20, 255]),
    ] {
        write_png(
            directory.path(),
            &format!("textures/blocks/{name}"),
            TILE_SIZE,
            TILE_SIZE,
            &solid(TILE_SIZE, TILE_SIZE, colour),
        );
    }
    directory
}

fn leaf(id: u32, name: &str) -> RegistryRecord {
    record(
        id,
        100 + id,
        name,
        "{}",
        BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
    )
}

#[test]
fn pack_authored_leaf_face_metadata_survives_world_groups_and_runtime_only() {
    let directory = fixture(
        r#"{
            "cherry_leaves":{"textures":"leaves","ambient_occlusion_exponent":0.8,"isotropic":{"down":true,"up":true}},
            "azalea_leaves":{"textures":"leaves","ambient_occlusion_exponent":0.6,"isotropic":{"side":true,"west":false}},
            "azalea_leaves_flowered":{"textures":"leaves"}
        }"#,
        r#"{"texture_data":{"leaves":{"textures":"textures/blocks/leaves"}}}"#,
    );
    let records = [
        leaf(0, "minecraft:cherry_leaves"),
        leaf(1, "minecraft:azalea_leaves"),
        leaf(2, "minecraft:azalea_leaves_flowered"),
    ];
    let mut compiled = compile_pack(directory.path(), &records).unwrap();
    assert_eq!(compiled.visuals[0].faces, compiled.visuals[1].faces);
    assert_eq!(compiled.visuals[1].faces, compiled.visuals[2].faces);
    assert_ne!(compiled.visuals[0].variant, compiled.visuals[1].variant);
    assert_ne!(compiled.visuals[1].variant, compiled.visuals[2].variant);
    for (id, exponent) in [0.8_f32, 0.6, 1.0].into_iter().enumerate() {
        let visual = &compiled.visuals[id];
        let base = (visual.variant & assets::BLOCK_VISUAL_VARIANT_MATERIAL_MASK) as usize;
        for offset in 0..assets::SEASONAL_LEAF_MATERIAL_COUNT as usize {
            let face = BlockFace::ALL[offset % BlockFace::ALL.len()];
            let world = compiled.materials[base + offset];
            assert_eq!(
                assets::material_leaf_ao_exponent(world.flags).to_bits(),
                exponent.to_bits()
            );
            let isotropic = match id {
                0 => matches!(face, BlockFace::Down | BlockFace::Up),
                1 => face.is_horizontal(),
                _ => false,
            };
            assert_eq!(
                world.flags & assets::MATERIAL_FLAG_LEAF_ISOTROPIC != 0,
                isotropic
            );
            assert_eq!(
                compiled.materials[visual.faces[face as usize] as usize].flags
                    & assets::MATERIAL_LEAF_METADATA_MASK,
                0
            );
        }
    }
    compiled.provenance = BlobProvenance {
        source_manifest_sha256: [1; 32],
        block_registry_sha256: [2; 32],
        light_registry_sha256: [3; 32],
        biome_registry_sha256: [4; 32],
    };
    let baseline = encode_blob(&compiled).unwrap();
    let runtime = RuntimeAssets::decode(&baseline).unwrap();
    assert_eq!(runtime.materials(), &*compiled.materials);
    for seed in 0..20 {
        let mut reordered =
            compile_pack(directory.path(), &shuffled_records(&records, seed)).unwrap();
        reordered.provenance = compiled.provenance;
        assert_eq!(encode_blob(&reordered).unwrap(), baseline);
    }
    let carried = compiled.visuals[0].faces[0] as usize;
    compiled.materials[carried].flags |= assets::MATERIAL_FLAG_LEAF_ISOTROPIC;
    assert!(
        encode_blob(&compiled).is_err(),
        "world-only metadata must require the native leaf material flag"
    );
}

#[test]
fn world_leaf_variations_receive_face_metadata_without_changing_carried_choices() {
    let directory = fixture(
        r#"{"cherry_leaves":{"textures":"leaves","ambient_occlusion_exponent":0.8,"isotropic":true}}"#,
        r#"{"texture_data":{"leaves":{"textures":{"variations":[{"path":"textures/blocks/leaves","weight":1},{"path":"textures/blocks/alternate","weight":3}]}}}}"#,
    );
    let compiled = compile_pack(directory.path(), &[leaf(0, "minecraft:cherry_leaves")]).unwrap();
    let visual = &compiled.visuals[0];
    let base = (visual.variant & assets::BLOCK_VISUAL_VARIANT_MATERIAL_MASK) as usize;
    let carried = compiled.materials[visual.faces[0] as usize];
    assert_eq!(carried.variation_count, 2);
    let carried_choices =
        &compiled.materials[carried.variation_start as usize..][..carried.variation_count as usize];
    assert!(
        carried_choices
            .iter()
            .all(|material| material.flags & assets::MATERIAL_LEAF_METADATA_MASK == 0)
    );
    for world in &compiled.materials[base..base + assets::SEASONAL_LEAF_MATERIAL_COUNT as usize] {
        assert_eq!(world.variation_count, carried.variation_count);
        let choices =
            &compiled.materials[world.variation_start as usize..][..world.variation_count as usize];
        for (choice, carried_choice) in choices.iter().zip(carried_choices) {
            assert_eq!(choice.variation_weight, carried_choice.variation_weight);
            assert_eq!(
                choice.flags & assets::MATERIAL_LEAF_METADATA_MASK,
                world.flags & assets::MATERIAL_LEAF_METADATA_MASK
            );
            assert_eq!(
                assets::material_leaf_ao_exponent(choice.flags).to_bits(),
                0.8_f32.to_bits()
            );
            assert_ne!(choice.texture, carried_choice.texture);
        }
    }
}

#[test]
fn unsupported_custom_leaf_exponents_fail_explicitly_instead_of_quantizing() {
    for exponent in [0.0, -0.8, 0.805, 2.56] {
        let blocks = serde_json::json!({"cherry_leaves": {"textures":"leaves", "ambient_occlusion_exponent": exponent}}).to_string();
        let directory = fixture(
            &blocks,
            r#"{"texture_data":{"leaves":{"textures":"textures/blocks/leaves"}}}"#,
        );
        let error =
            compile_pack(directory.path(), &[leaf(0, "minecraft:cherry_leaves")]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported world leaf ambient_occlusion_exponent"),
            "{error}"
        );
    }
}

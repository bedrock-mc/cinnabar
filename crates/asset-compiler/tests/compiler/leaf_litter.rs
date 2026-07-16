use super::support::*;

const LEAF_LITTER_HASHES: [u32; 32] = [
    0x834b_5ffc,
    0xa149_5c55,
    0x576a_f652,
    0x5bdd_1223,
    0xb79b_b5b0,
    0x590c_4349,
    0xe883_5116,
    0x4261_2d47,
    0x200a_28f1,
    0x6d49_e4fe,
    0xaa10_c7fb,
    0x0564_5e28,
    0xc43e_17cd,
    0xd627_b70a,
    0x9484_bf17,
    0xc665_5304,
    0xfba6_861a,
    0xc19c_81d7,
    0xeb43_47dc,
    0xdc27_3719,
    0x3930_96fe,
    0x3626_336b,
    0x2c41_9290,
    0x2f3d_304d,
    0xabb9_eec3,
    0xd98b_f63c,
    0x00a5_5ba9,
    0x58b1_545a,
    0x790d_b0a7,
    0x7909_09c0,
    0x5f78_9f1d,
    0x3518_cb9e,
];

fn leaf_litter_records() -> Vec<RegistryRecord> {
    read_registry(include_bytes!(
        "../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed protocol-1001 registry")
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:leaf_litter")
    .collect()
}

fn write_leaf_litter_pack(root: &Path) {
    write_pack(
        root,
        r#"{"leaf_litter":{"carried_textures":"leaf_litter_carried","sound":"leaf_litter","textures":"leaf_litter"}}"#,
        r#"{"texture_data":{"leaf_litter":{"textures":["textures/blocks/leaf_litter"]},"leaf_litter_carried":{"textures":["textures/items/leaf_litter"]}}}"#,
        "[]",
    );
    let pixels = (0..TILE_SIZE * TILE_SIZE)
        .map(|index| {
            let alpha = if index % 3 == 0 { 0 } else { 255 };
            [
                128 + (index % 96) as u8,
                128 + (index % 96) as u8,
                128 + (index % 96) as u8,
                alpha,
            ]
        })
        .collect::<Vec<_>>();
    write_png(
        root,
        "textures/blocks/leaf_litter",
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
    );
    write_png(
        root,
        "textures/items/leaf_litter",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [111, 111, 111, 255]),
    );
}

fn tagged_state(record: &RegistryRecord) -> (u32, String) {
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .expect("typed leaf-litter state");
    assert_eq!(state.len(), 2);
    let growth = state["growth"]["value"].as_u64().unwrap() as u32;
    assert_eq!(state["growth"]["type"], "int");
    let direction = state["minecraft:cardinal_direction"]["value"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(state["minecraft:cardinal_direction"]["type"], "string");
    (growth, direction)
}

#[test]
fn protocol_1001_leaf_litter_inventory_is_exact_collisionless_and_hash_pinned() {
    let records = leaf_litter_records();
    assert_eq!(records.len(), 32);
    let directions = ["south", "west", "north", "east"];
    for (index, record) in records.iter().enumerate() {
        let orientation = index / 8;
        let growth = index % 8;
        assert_eq!(record.sequential_id, 46 + index as u32);
        assert_eq!(record.network_hash, LEAF_LITTER_HASHES[index]);
        assert_eq!(
            tagged_state(record),
            (growth as u32, directions[orientation].to_owned())
        );
        assert_eq!(record.model_family, ModelFamily::Layer);
        assert_eq!(record.contributor_role, ContributorRole::Primary);
        assert_eq!(record.flags, BlockFlags::empty());
        assert_eq!(record.face_coverage, 0);
        assert_eq!(record.model_state.mask(), 0x21);
        assert_eq!(
            record.model_state.get(ModelStateField::Orientation),
            Some(orientation as u32)
        );
        assert_eq!(
            record.model_state.get(ModelStateField::Growth),
            Some(growth as u32)
        );
        assert_eq!(record.collision_seed.shape_id, 0);
        assert_eq!(
            record.collision_seed.confidence,
            CollisionConfidence::CollisionOnly
        );
        assert!(record.collision_seed.boxes.is_empty());
        assert_eq!(
            record.provenance,
            RegistryProvenance::PMMP
                | RegistryProvenance::DRAGONFLY
                | RegistryProvenance::PRISMARINE
                | RegistryProvenance::VALENTINE
        );
    }
}

#[test]
fn compiler_emits_pinned_leaf_litter_layouts_with_dry_foliage_cutout_and_hash_parity() {
    let directory = tempfile::tempdir().expect("create leaf-litter fixture");
    write_leaf_litter_pack(directory.path());
    let records = leaf_litter_records();
    let compiled = compile_pack(directory.path(), &records).expect("compile exact leaf litter");

    assert_eq!(compiled.materials.len(), 2, "diagnostic plus leaf litter");
    assert_eq!(compiled.model_templates.len(), 16);
    assert_eq!(compiled.model_quads.len(), 20);
    for record in &records {
        let (growth, _) = tagged_state(record);
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model);
        assert_eq!(visual.flags, BlockFlags::empty());
        assert_ne!(visual.model_template, assets::NO_MODEL_TEMPLATE);
        assert!(visual.faces.iter().all(|&face| face == visual.faces[0]));
        let material = compiled.materials[visual.faces[0] as usize];
        assert_eq!(
            material.flags,
            MATERIAL_FLAG_ALPHA_CUTOUT | MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_DRY_FOLIAGE
        );
        assert_eq!(material.animation, assets::NO_ANIMATION);
        let quads = template_quads(&compiled, visual.model_template);
        assert_eq!(quads.len(), if growth == 2 { 2 } else { 1 });
        assert!(quads.iter().all(|quad| {
            quad.material == visual.faces[0]
                && quad.flags == MODEL_QUAD_FLAG_TWO_SIDED
                && quad.positions.iter().all(|position| position[1] == 4)
                && quad.positions.iter().all(|position| {
                    (0..=256).contains(&position[0]) && (0..=256).contains(&position[2])
                })
        }));
    }

    let north = &records[16..24];
    let expected = [
        vec![(
            [[0, 4, 0], [128, 4, 0], [128, 4, 128], [0, 4, 128]],
            [[0, 0], [2048, 0], [2048, 2048], [0, 2048]],
        )],
        vec![(
            [[0, 4, 0], [128, 4, 0], [128, 4, 256], [0, 4, 256]],
            [[0, 0], [2048, 0], [2048, 4096], [0, 4096]],
        )],
        vec![
            (
                [[0, 4, 0], [128, 4, 0], [128, 4, 256], [0, 4, 256]],
                [[0, 0], [2048, 0], [2048, 4096], [0, 4096]],
            ),
            (
                [[128, 4, 128], [256, 4, 128], [256, 4, 256], [128, 4, 256]],
                [[2048, 2048], [4096, 2048], [4096, 4096], [2048, 4096]],
            ),
        ],
        vec![(
            [[0, 4, 0], [256, 4, 0], [256, 4, 256], [0, 4, 256]],
            [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        )],
    ];
    for growth in 0..4 {
        let visual = compiled.visuals[north[growth].sequential_id as usize];
        let quads = template_quads(&compiled, visual.model_template);
        assert_eq!(quads.len(), expected[growth].len());
        for (quad, (positions, uvs)) in quads.iter().zip(&expected[growth]) {
            assert_eq!(quad.positions, *positions, "growth={growth}");
            assert_eq!(quad.uvs, *uvs, "growth={growth}");
        }
    }
    for growth in 4..8 {
        assert_eq!(
            compiled.visuals[north[growth].sequential_id as usize].model_template,
            compiled.visuals[north[3].sequential_id as usize].model_template
        );
    }

    let bytes = encode_blob(&compiled).expect("encode leaf-litter assets");
    let runtime = RuntimeAssets::decode(&bytes).expect("decode leaf-litter assets");
    for record in &records {
        let sequential = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let hashed = runtime.resolve(NetworkIdMode::Hashed, record.network_hash);
        assert_eq!(sequential.kind(), VisualKind::Model);
        assert_eq!(hashed.kind(), VisualKind::Model);
        assert_eq!(sequential.model_template(), hashed.model_template());
        assert_eq!(
            sequential.face(BlockFace::Up).material_id(),
            hashed.face(BlockFace::Up).material_id()
        );
    }
    assert_eq!(runtime.missing_count(), 0);
    let mut reversed = records.clone();
    reversed.reverse();
    let reversed = compile_pack(directory.path(), &reversed).expect("compile reversed leaf litter");
    assert_eq!(encode_blob(&reversed).unwrap(), bytes);
}

#[test]
fn compiler_leaf_litter_admission_is_atomic_for_registry_and_pack_evidence() {
    let directory = tempfile::tempdir().expect("create leaf-litter fixture");
    write_leaf_litter_pack(directory.path());
    let records = leaf_litter_records();
    let mut cases = Vec::new();
    let mut missing = records.clone();
    missing.pop();
    cases.push(("missing", missing));
    let mut wrong_hash = records.clone();
    wrong_hash[0].network_hash ^= 1;
    cases.push(("hash", wrong_hash));
    let mut wrong_type = records.clone();
    wrong_type[0].canonical_state = r#"{"growth":{"type":"byte","value":0},"minecraft:cardinal_direction":{"type":"string","value":"south"}}"#.into();
    cases.push(("type", wrong_type));
    let mut extra = records.clone();
    extra[0].canonical_state = r#"{"extra":{"type":"int","value":0},"growth":{"type":"int","value":0},"minecraft:cardinal_direction":{"type":"string","value":"south"}}"#.into();
    cases.push(("extra", extra));
    let mut projection = records.clone();
    projection[0].model_state = projection[1].model_state;
    cases.push(("projection", projection));
    let mut collision = records.clone();
    collision[0].collision_seed.shape_id = 1;
    cases.push(("collision", collision));
    let mut family = records.clone();
    family[0].model_family = ModelFamily::FlowerBed;
    cases.push(("family", family));
    let mut flags = records.clone();
    flags[0].flags = BlockFlags::CUBE_GEOMETRY;
    cases.push(("flags", flags));
    for (label, family) in cases {
        let compiled = compile_pack(directory.path(), &family).expect("compile rejected family");
        assert!(
            family.iter().all(
                |record| compiled.visuals[record.sequential_id as usize].kind
                    == VisualKind::Diagnostic
            ),
            "invalid leaf-litter family `{label}` was partially admitted"
        );
        assert!(compiled.model_templates.is_empty(), "{label}");
        assert!(compiled.model_quads.is_empty(), "{label}");
    }

    for (label, blocks, terrain, flipbooks) in [
        (
            "wrong selector",
            r#"{"leaf_litter":{"carried_textures":"leaf_litter_carried","sound":"leaf_litter","textures":"wrong"}}"#,
            r#"{"texture_data":{"leaf_litter":{"textures":["textures/blocks/leaf_litter"]},"wrong":{"textures":["textures/blocks/leaf_litter"]}}}"#,
            "[]",
        ),
        (
            "missing carried",
            r#"{"leaf_litter":{"sound":"leaf_litter","textures":"leaf_litter"}}"#,
            r#"{"texture_data":{"leaf_litter":{"textures":["textures/blocks/leaf_litter"]}}}"#,
            "[]",
        ),
        (
            "static terrain",
            r#"{"leaf_litter":{"carried_textures":"leaf_litter_carried","sound":"leaf_litter","textures":"leaf_litter"}}"#,
            r#"{"texture_data":{"leaf_litter":{"textures":"textures/blocks/leaf_litter"}}}"#,
            "[]",
        ),
        (
            "terrain metadata",
            r#"{"leaf_litter":{"carried_textures":"leaf_litter_carried","sound":"leaf_litter","textures":"leaf_litter"}}"#,
            r#"{"texture_data":{"leaf_litter":{"textures":[{"path":"textures/blocks/leaf_litter","alias":"wrong"}]}}}"#,
            "[]",
        ),
        (
            "flipbook",
            r#"{"leaf_litter":{"carried_textures":"leaf_litter_carried","sound":"leaf_litter","textures":"leaf_litter"}}"#,
            r#"{"texture_data":{"leaf_litter":{"textures":["textures/blocks/leaf_litter"]}}}"#,
            r#"[{"flipbook_texture":"textures/blocks/leaf_litter","atlas_tile":"leaf_litter"}]"#,
        ),
    ] {
        let malformed = tempfile::tempdir().expect("create malformed leaf-litter pack");
        write_leaf_litter_pack(malformed.path());
        write_pack(malformed.path(), blocks, terrain, flipbooks);
        let compiled = compile_pack(malformed.path(), &records).expect("compile malformed pack");
        assert!(
            records.iter().all(
                |record| compiled.visuals[record.sequential_id as usize].kind
                    == VisualKind::Diagnostic
            ),
            "nonexact pack `{label}` was admitted"
        );
    }

    let opaque = tempfile::tempdir().expect("create opaque leaf-litter pack");
    write_leaf_litter_pack(opaque.path());
    write_png(
        opaque.path(),
        "textures/blocks/leaf_litter",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [150, 150, 150, 255]),
    );
    let compiled = compile_pack(opaque.path(), &records).expect("compile opaque leaf litter");
    assert!(records.iter().all(
        |record| compiled.visuals[record.sequential_id as usize].kind == VisualKind::Diagnostic
    ));
}

#[test]
#[ignore = "requires PINNED_VANILLA_PACK pointing at ignored Bedrock 1.26.30.32 resource pack"]
fn compiler_real_pinned_pack_admits_all_leaf_litter_states() {
    let pack = std::env::var_os("PINNED_VANILLA_PACK")
        .expect("set PINNED_VANILLA_PACK to the ignored pinned vanilla resource pack");
    let records = leaf_litter_records();
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned leaf litter");
    assert!(records.iter().all(|record| {
        compiled.visuals[record.sequential_id as usize].kind == VisualKind::Model
    }));
}

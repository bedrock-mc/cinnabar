use {
    super::support::*,
    assets::{
        BlockFlags, ContributorRole, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND,
        MATERIAL_FLAG_ALPHA_CUTOUT, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE, ModelFamily, VisualKind,
        encode_blob, read_registry,
    },
    std::path::Path,
    tempfile::TempDir,
};

#[test]
fn compiler_emits_exact_checked_copper_grate_models() {
    let directory = tempfile::tempdir().expect("create copper-grate fixture");
    let mut blocks = serde_json::Map::new();
    let mut terrain = serde_json::Map::new();
    for (pair_index, (unwaxed, waxed)) in COPPER_GRATE_ALIAS_PAIRS.iter().enumerate() {
        let texture = format!("copper_grate_{pair_index}");
        let path = format!("textures/blocks/{texture}");
        terrain.insert(texture.clone(), serde_json::json!({ "textures": path }));
        for name in [unwaxed, waxed] {
            blocks.insert(
                name.strip_prefix("minecraft:").unwrap().into(),
                serde_json::json!({ "textures": texture }),
            );
        }
        write_png(
            directory.path(),
            &path,
            TILE_SIZE,
            TILE_SIZE,
            &solid(
                TILE_SIZE,
                TILE_SIZE,
                [35 + pair_index as u8 * 40, 90, 130, 64],
            ),
        );
    }
    write_pack(
        directory.path(),
        &serde_json::Value::Object(blocks).to_string(),
        &serde_json::json!({ "texture_data": terrain }).to_string(),
        "[]",
    );

    let mut records = COPPER_GRATE_NAMES
        .iter()
        .enumerate()
        .map(|(id, name)| {
            let mut record =
                model_record(id as u32, 92_000 + id as u32, name, "{}", ModelFamily::Cube);
            record.flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
            record
        })
        .collect::<Vec<_>>();
    let admitted_count = records.len();
    let mut wrong_state = model_record(
        admitted_count as u32,
        92_100,
        "minecraft:copper_grate",
        r#"{"extra":{"type":"byte","value":0}}"#,
        ModelFamily::Cube,
    );
    wrong_state.flags = BlockFlags::CUBE_GEOMETRY;
    records.push(wrong_state);
    records.push(model_record(
        records.len() as u32,
        92_101,
        "minecraft:copper_grate",
        "{}",
        ModelFamily::Pane,
    ));
    let mut wrong_role = model_record(
        records.len() as u32,
        92_102,
        "minecraft:copper_grate",
        "{}",
        ModelFamily::Cube,
    );
    wrong_role.contributor_role = ContributorRole::LiquidAdditional;
    records.push(wrong_role);
    for name in [
        "minecraft:cut_copper_grate",
        "minecraft:copper_bars",
        "minecraft:copper_bulb",
        "minecraft:copper_door",
        "minecraft:copper_trapdoor",
        "minecraft:slime",
        "minecraft:glass",
        "minecraft:red_stained_glass_pane",
        "minecraft:invisible_bedrock",
    ] {
        records.push(model_record(
            records.len() as u32,
            92_000 + records.len() as u32,
            name,
            "{}",
            ModelFamily::Cube,
        ));
    }

    let compiled = compile_pack(directory.path(), &records).expect("compile copper grates");
    for (id, record) in records.iter().take(admitted_count).enumerate() {
        let visual = compiled.visuals[id];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.name);
        assert!(!visual.flags.intersects(
            BlockFlags::AIR
                | BlockFlags::CUBE_GEOMETRY
                | BlockFlags::OCCLUDES_FULL_FACE
                | BlockFlags::LEAF_MODEL
        ));
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE);
        assert_eq!(template.quad_count, 6);
        let quads = template_quads(&compiled, visual.model_template);
        assert_eq!(model_bounds(quads), ([0, 0, 0], [256, 256, 256]));
        assert!(quads.iter().enumerate().all(|(face, quad)| {
            quad.material == visual.faces[face]
                && quad.flags == [3, 4, 1, 2, 5, 6][face]
                && compiled.materials[quad.material as usize].flags & MATERIAL_FLAG_ALPHA_CUTOUT
                    != 0
                && compiled.materials[quad.material as usize].flags & MATERIAL_FLAG_ALPHA_BLEND == 0
        }));
    }
    for (unwaxed, waxed) in COPPER_GRATE_ALIAS_PAIRS {
        let faces = |name: &str| {
            let index = records
                .iter()
                .position(|record| record.name.as_ref() == name)
                .unwrap();
            compiled.visuals[index].faces
        };
        assert_eq!(faces(unwaxed), faces(waxed), "alias pair {unwaxed}/{waxed}");
    }
    for (record, visual) in records[admitted_count..]
        .iter()
        .zip(&compiled.visuals[admitted_count..records.len()])
    {
        // Invisible bedrock is a known no-draw block, not a diagnostic.
        if record.name.as_ref() == "minecraft:invisible_bedrock" {
            assert_eq!(visual.kind, VisualKind::Invisible);
            continue;
        }
        assert!(
            visual.kind == VisualKind::Diagnostic && visual.faces == [DIAGNOSTIC_MATERIAL; 6],
            "{} {:?} {:?}",
            record.name,
            record.model_family,
            visual.kind
        );
    }

    let baseline = encode_blob(&compiled).expect("encode copper grates");
    records.reverse();
    let reversed =
        compile_pack(directory.path(), &records).expect("compile reversed copper grates");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

fn single_copper_grate_fixture() -> TempDir {
    let directory = tempfile::tempdir().expect("create single copper-grate fixture");
    write_pack(
        directory.path(),
        r#"{"copper_grate":{"textures":"copper_grate"}}"#,
        r#"{"texture_data":{"copper_grate":{"textures":"textures/blocks/copper_grate"}}}"#,
        "[]",
    );
    write_png(
        directory.path(),
        "textures/blocks/copper_grate",
        TILE_SIZE,
        TILE_SIZE,
        &solid(TILE_SIZE, TILE_SIZE, [184, 115, 51, 64]),
    );
    directory
}

#[test]
fn compiler_rejects_exact_copper_grate_marked_air() {
    let directory = single_copper_grate_fixture();
    let mut record = model_record(0, 92_200, "minecraft:copper_grate", "{}", ModelFamily::Cube);
    record.flags = BlockFlags::AIR;

    let compiled = compile_pack(directory.path(), &[record]).expect("compile air copper grate");

    assert_eq!(compiled.visuals[0].kind, VisualKind::Diagnostic);
    assert_eq!(compiled.visuals[0].faces, [DIAGNOSTIC_MATERIAL; 6]);
}

#[test]
fn compiler_rejects_exact_copper_grate_with_flags_zero() {
    let directory = single_copper_grate_fixture();
    let record = model_record(0, 92_201, "minecraft:copper_grate", "{}", ModelFamily::Cube);

    let compiled =
        compile_pack(directory.path(), &[record]).expect("compile flags-zero copper grate");

    assert_eq!(compiled.visuals[0].kind, VisualKind::Diagnostic);
    assert_eq!(compiled.visuals[0].faces, [DIAGNOSTIC_MATERIAL; 6]);
}

#[test]
fn compiler_real_pinned_pack_admits_only_exact_copper_grate_records() {
    let Some(pack) = crate::fixture_input::env_path("PINNED_VANILLA_PACK") else {
        return;
    };
    let all = read_registry(include_bytes!(
        "../../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode committed generated registry");
    let copper_grates = all
        .iter()
        .filter(|record| {
            COPPER_GRATE_NAMES
                .binary_search(&record.name.as_ref())
                .is_ok()
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(copper_grates.len(), 8);
    assert!(copper_grates.iter().all(|record| {
        record.canonical_state.as_ref() == "{}"
            && record.model_family == ModelFamily::Cube
            && record.contributor_role == ContributorRole::Primary
            && record.flags == BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
    }));
    let excluded = all
        .iter()
        .filter(|record| {
            record.name.contains("copper_grate")
                && COPPER_GRATE_NAMES
                    .binary_search(&record.name.as_ref())
                    .is_err()
                || matches!(
                    record.name.as_ref(),
                    "minecraft:slime" | "minecraft:invisible_bedrock"
                )
                || record.name.starts_with("minecraft:hard_")
                    && record.name.ends_with("_stained_glass")
        })
        .cloned()
        .collect::<Vec<_>>();
    assert!(!excluded.is_empty());

    let admitted_count = copper_grates.len();
    let mut records = copper_grates
        .into_iter()
        .chain(excluded)
        .collect::<Vec<_>>();
    for (id, record) in records.iter_mut().enumerate() {
        record.sequential_id = id as u32;
        record.network_hash = 93_000 + id as u32;
    }
    let compiled = compile_pack(Path::new(&pack), &records).expect("compile pinned copper grates");
    let fixture_air = fixture_air_id(&records) as usize;
    for (id, visual) in compiled.visuals.iter().enumerate() {
        if id == fixture_air {
            assert_eq!(visual.kind, VisualKind::Invisible, "appended fixture air");
        } else if id < admitted_count {
            assert_eq!(visual.kind, VisualKind::Model, "{}", records[id].name);
            let template = compiled.model_templates[visual.model_template as usize];
            assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE);
            assert_eq!(template.quad_count, 6);
            assert!(visual.faces.iter().all(|&material| {
                material != DIAGNOSTIC_MATERIAL
                    && compiled.materials[material as usize].flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0
                    && compiled.materials[material as usize].flags & MATERIAL_FLAG_ALPHA_BLEND == 0
            }));
            assert!(!visual.flags.intersects(
                BlockFlags::AIR
                    | BlockFlags::CUBE_GEOMETRY
                    | BlockFlags::OCCLUDES_FULL_FACE
                    | BlockFlags::LEAF_MODEL
            ));
        } else {
            assert_eq!(visual.kind, VisualKind::Diagnostic, "{}", records[id].name);
        }
    }
    for (unwaxed, waxed) in COPPER_GRATE_ALIAS_PAIRS {
        let faces = |name: &str| {
            let index = records
                .iter()
                .position(|record| record.name.as_ref() == name)
                .unwrap();
            compiled.visuals[index].faces
        };
        assert_eq!(faces(unwaxed), faces(waxed), "alias pair {unwaxed}/{waxed}");
    }
    let baseline = encode_blob(&compiled).expect("encode pinned copper grates");
    records.reverse();
    let reversed =
        compile_pack(Path::new(&pack), &records).expect("compile reversed pinned copper grates");
    assert_eq!(encode_blob(&reversed).unwrap(), baseline);
}

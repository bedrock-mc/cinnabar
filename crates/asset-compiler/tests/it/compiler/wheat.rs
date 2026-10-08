use super::support::*;

fn wheat_records() -> Vec<RegistryRecord> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("assets/bedrock-target.json")).unwrap())
            .unwrap();
    let bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let mut records = assets::read_registry_for_protocol(
        &bytes,
        target["wire_protocol"].as_u64().unwrap() as u32,
    )
    .unwrap()
    .into_iter()
    .filter(|record| record.name.as_ref() == "minecraft:wheat")
    .collect::<Vec<_>>();
    records.sort_unstable_by_key(|record| record.model_state.get(ModelStateField::Growth).unwrap());
    assert_eq!(records.len(), 8);
    records
}

#[test]
fn compiler_wheat_rows_meet_farmland_and_preserve_all_growth_textures() {
    let directory = tempfile::tempdir().unwrap();
    let paths = (0..8)
        .map(|stage| format!("textures/blocks/wheat{stage}"))
        .collect::<Vec<_>>();
    write_pack(
        directory.path(),
        r#"{"wheat":{"textures":"wheat"},"yellow_flower":{"textures":"wheat"}}"#,
        &serde_json::json!({"texture_data":{"wheat":{"textures":paths}}}).to_string(),
        "[]",
    );
    let images = (0..8)
        .map(|stage| {
            let pixels = (0..TILE_SIZE * TILE_SIZE)
                .map(|pixel| {
                    [
                        stage + 1,
                        (pixel % TILE_SIZE) as u8,
                        (pixel / TILE_SIZE) as u8,
                        255,
                    ]
                })
                .collect::<Vec<_>>();
            write_png(
                directory.path(),
                &paths[stage as usize],
                TILE_SIZE,
                TILE_SIZE,
                &pixels,
            );
            pixels.into_iter().flatten().collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut records = wheat_records();
    let mut flower = records[0].clone();
    flower.sequential_id = records
        .iter()
        .map(|record| record.sequential_id)
        .max()
        .unwrap()
        + 1;
    flower.network_hash = u32::MAX;
    flower.name = "minecraft:dandelion".into();
    flower.model_family = ModelFamily::Cross;
    records.push(flower);
    let compiled = compile_pack(directory.path(), &records).unwrap();
    let runtime = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    for (stage, record) in records[..8].iter().enumerate() {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Cross);
        assert_eq!(visual.variant, stage as u32);
        let quads = template_quads(&compiled, visual.model_template);
        assert_eq!(quads.len(), 4);
        let mut rows = HashSet::new();
        for quad in quads {
            assert_eq!(quad.flags, MODEL_QUAD_FLAG_TWO_SIDED);
            assert_eq!(quad.uvs, [[0, 0], [0, 4096], [4096, 4096], [4096, 0]]);
            assert_eq!(quad.positions.iter().map(|p| p[1]).min(), Some(-16));
            assert_eq!(quad.positions.iter().map(|p| p[1]).max(), Some(240));
            let axis = usize::from(quad.positions.iter().any(|p| p[0] != quad.positions[0][0])) * 2;
            let across = 2 - axis;
            assert!(
                quad.positions
                    .iter()
                    .all(|p| p[axis] == quad.positions[0][axis])
            );
            assert_eq!(quad.positions.iter().map(|p| p[across]).min(), Some(0));
            assert_eq!(quad.positions.iter().map(|p| p[across]).max(), Some(256));
            rows.insert((axis, quad.positions[0][axis]));
            let material = compiled.materials[quad.material as usize];
            assert_eq!(material.flags, MATERIAL_FLAG_ALPHA_CUTOUT);
            let mip = &compiled.texture_pages[material.texture.page() as usize]
                .texture
                .mips[0];
            let start = material.texture.layer() as usize * (TILE_SIZE * TILE_SIZE * 4) as usize;
            assert_eq!(
                &mip.rgba8[start..start + images[stage].len()],
                images[stage]
            );
        }
        assert_eq!(rows, HashSet::from([(0, 64), (0, 192), (2, 64), (2, 192)]));
        for (mode, id) in [
            (NetworkIdMode::Sequential, record.sequential_id),
            (NetworkIdMode::Hashed, record.network_hash),
        ] {
            assert_eq!(
                runtime.resolve(mode, id).model_template(),
                Some(visual.model_template)
            );
        }
    }
    let flower = compiled.visuals[records[8].sequential_id as usize];
    let wheat = compiled.visuals[records[0].sequential_id as usize];
    assert_eq!(flower.faces, wheat.faces, "fixture must share a material");
    assert_ne!(
        flower.model_template, wheat.model_template,
        "geometry participates in cache identity"
    );
    assert_eq!(template_quads(&compiled, flower.model_template).len(), 2);
    records.reverse();
    assert_eq!(
        encode_blob(&compiled).unwrap(),
        encode_blob(&compile_pack(directory.path(), &records).unwrap()).unwrap()
    );
}

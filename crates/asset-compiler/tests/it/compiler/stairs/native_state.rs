use {
    super::*,
    assets::{MODEL_QUAD_FLAG_FACE_MASK, RegistryRecord, RuntimeAssets, VisualKind, encode_blob},
    std::fs,
    std::path::Path,
};

fn native_stair_records() -> Vec<RegistryRecord> {
    let data = include_bytes!("../../../../../assets/data/block-registry-v2193.bin");
    let protocol = assets::registry_header_protocol(data).unwrap();
    assets::read_registry_for_protocol(data, protocol)
        .unwrap()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:oak_stairs" | "minecraft:birch_stairs"
            )
        })
        .enumerate()
        .map(|(id, mut record)| {
            record.sequential_id = id as u32;
            record
        })
        .collect()
}

fn native_stair_pack(root: &Path) {
    write_stair_pack(root);
    fs::write(
        root.join("blocks.json"),
        r#"{
        "oak_stairs":{"textures":{"down":"stair_down","side":"stair_side","up":"stair_up"}},
        "birch_stairs":{"textures":{"down":"stair_down","side":"stair_side","up":"stair_up"}}
    }"#,
    )
    .unwrap();
}

#[test]
fn native_stair_corner_state_and_raw_direction_select_all_occupied_quadrants() {
    let records = native_stair_records();
    let directory = tempfile::tempdir().unwrap();
    native_stair_pack(directory.path());
    let compiled = compile_pack(directory.path(), &records).unwrap();
    // Native step + optional inner-piece AABBs, bits x + 2*z. These are
    // independent vanilla step and inner-piece witnesses, not the compiler selector.
    let occupied = [
        [10_u8, 11, 14, 2, 8],
        [5, 13, 7, 4, 1],
        [12, 14, 13, 8, 4],
        [3, 7, 11, 1, 2],
    ];
    let corners = [
        "none",
        "inner_left",
        "inner_right",
        "outer_left",
        "outer_right",
    ];
    let mut states = 0;
    for record in &records {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let raw = state["weirdo_direction"]["value"].as_u64().unwrap() as usize;
        let half = state["upside_down_bit"]["value"].as_u64().unwrap();
        let corner = corners
            .iter()
            .position(|name| state["minecraft:corner"]["value"] == *name)
            .unwrap();
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model, "{}", record.canonical_state);
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(
            template.flags, 0,
            "native state must not re-infer its corner"
        );
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        for layer in 0..2 {
            let face = if layer == 0 { 1 } else { 2 };
            let outer_y = if layer == 0 { 0 } else { 256 };
            for quadrant in 0..4 {
                let [x, z] = [64 + (quadrant & 1) * 128, 64 + (quadrant >> 1) * 128];
                let actual = quads.iter().any(|quad| {
                    let positions = quad
                        .positions
                        .map(|point| rotate_stair_position(point, visual.variant));
                    quad.flags & MODEL_QUAD_FLAG_FACE_MASK == face
                        && positions.iter().all(|point| point[1] == outer_y)
                        && positions.iter().map(|point| point[0]).min().unwrap() < x
                        && x < positions.iter().map(|point| point[0]).max().unwrap()
                        && positions.iter().map(|point| point[2]).min().unwrap() < z
                        && z < positions.iter().map(|point| point[2]).max().unwrap()
                });
                let expected = layer == half || occupied[raw][corner] & (1 << quadrant) != 0;
                assert_eq!(
                    actual, expected,
                    "{} {} layer={layer} quadrant={quadrant}",
                    record.name, record.canonical_state
                );
            }
        }
        states += 1;
    }
    assert_eq!(states, records.len());
    assert_eq!(records.len(), 80);
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
}

#[test]
fn odd_modern_stair_corner_states_remain_diagnostic_not_legacy_shapes() {
    let directory = tempfile::tempdir().unwrap();
    native_stair_pack(directory.path());
    let mut record = native_stair_records().remove(0);
    let original: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
    for alteration in [
        "unknown corner",
        "wrong type",
        "direction mismatch",
        "half mismatch",
        "extra property",
    ] {
        let mut state = original.clone();
        match alteration {
            "unknown corner" => state["minecraft:corner"]["value"] = "diagonal".into(),
            "wrong type" => state["minecraft:corner"]["type"] = "int".into(),
            "direction mismatch" => state["weirdo_direction"]["value"] = 99.into(),
            "half mismatch" => state["upside_down_bit"]["value"] = 99.into(),
            _ => state["unknown"] = serde_json::json!({"type":"byte", "value":0}),
        }
        record.canonical_state = state.to_string().into();
        let compiled = compile_pack(directory.path(), &[record.clone()]).unwrap();
        assert_eq!(
            compiled.visuals[0].kind,
            VisualKind::Diagnostic,
            "{alteration}"
        );
    }
}

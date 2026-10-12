use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles both shipped column states with distinct synthetic bubble and auxiliary art.
fn compiled_columns(state: Option<&str>) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let mut records: Vec<_> = registry
        .into_vec()
        .into_iter()
        .filter(|r| {
            matches!(
                r.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:bubble_column" | "minecraft:water"
            )
        })
        .collect();
    if let Some(state) = state {
        for record in records
            .iter_mut()
            .filter(|r| r.name.as_ref() == "minecraft:bubble_column")
        {
            record.canonical_state = state.into();
        }
    }
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{
        "bubble_column":{"textures":{"up":"up","down":"down","north":"outer","south":"inner","west":"pumpkin","east":"pumpkin"}},
        "water":{"textures":"water"},"stone":{"textures":"pumpkin"}
    }"#,
    );
    let mut textures = serde_json::Map::new();
    for (key, count) in [
        ("up", 4),
        ("down", 4),
        ("outer", 8),
        ("inner", 2),
        ("pumpkin", 1),
        ("water", 1),
    ] {
        let paths: Vec<_> = (0..count)
            .map(|i| {
                let path = format!("textures/{key}_{i}");
                let color = match key {
                    "pumpkin" => [255, 80, 0, 255],
                    "water" => [40, 80, 180, 160],
                    "outer" => [20 + i * 20, 150, 220, 160],
                    "inner" => [180 + i * 20, 150, 220, 160],
                    "up" => [60 + i * 20, 150, 220, 160],
                    "down" => [100 + i * 20, 150, 220, 160],
                    _ => unreachable!("fixture texture key"),
                };
                let pixels: Vec<_> = (0..TILE_SIZE * TILE_SIZE).flat_map(|_| color).collect();
                write_png(
                    directory.path().join(format!("{path}.png")),
                    TILE_SIZE,
                    TILE_SIZE,
                    &pixels,
                );
                path
            })
            .collect();
        textures.insert(key.into(), serde_json::json!({"textures":paths}));
    }
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data":textures})).unwrap(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

#[test]
fn bubble_columns_use_bubble_sides_and_leave_the_water_layer_independent() {
    let (compiled, records) = compiled_columns(None);
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let columns: Vec<_> = records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:bubble_column")
        .collect();
    assert_eq!(columns.len(), 2);
    for record in columns {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        assert!(
            !visual
                .flags()
                .contains(assets::BlockFlags::OCCLUDES_FULL_FACE)
        );
        let template = &runtime.model_templates()[visual.model_template().unwrap() as usize];
        assert_eq!(
            template.quad_count, 8,
            "submerged columns have two open-sided shells"
        );
        let quads = &runtime.model_quads()
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        for quad in quads {
            let material = &runtime.materials()[quad.material as usize];
            assert_ne!(material.flags & assets::MATERIAL_FLAG_WATER_TINT, 0);
            assert_ne!(material.flags & assets::MATERIAL_FLAG_ALPHA_BLEND, 0);
            assert!(
                quad.positions
                    .iter()
                    .all(|p| (16..=240).contains(&p[0]) && (16..=240).contains(&p[2]))
            );
        }
        let down: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let down = down["drag_down"]["value"].as_u64().unwrap() as u8;
        for offset in 0..assets::BUBBLE_COLUMN_TEMPLATE_COUNT {
            let template = runtime.model_templates()
                [visual.model_template().unwrap() as usize + offset as usize];
            assert_eq!(
                template.quad_count,
                assets::bubble_column_quad_count(offset)
            );
            let quads = &runtime.model_quads()[template.quad_start as usize..]
                [..template.quad_count as usize];
            for (index, quad) in quads.iter().enumerate() {
                let material = runtime.materials()[quad.material as usize];
                let mip = &runtime.texture_pages()[material.texture.page() as usize]
                    .texture
                    .mips[0];
                let pixel = &mip.rgba8
                    [material.texture.layer() as usize * (TILE_SIZE * TILE_SIZE * 4) as usize..]
                    [..4];
                let variant = (offset % assets::BUBBLE_COLUMN_POSITION_VARIANTS) as u8;
                let expected = if index < 4 {
                    20 + (variant + down * 4) * 20
                } else if index < 8 {
                    180 + down * 20
                } else {
                    60 + down * 40 + variant * 20
                };
                assert_eq!(
                    pixel,
                    [expected, 150, 220, 160],
                    "direction={down},offset={offset},quad={index}"
                );
            }
        }
    }
    for water in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:water")
    {
        let visual = runtime.resolve(NetworkIdMode::Sequential, water.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Liquid);
        assert_ne!(
            visual.face(BlockFace::Up).material_id(),
            assets::DIAGNOSTIC_MATERIAL
        );
    }
}

#[test]
fn bubble_column_malformed_directions_stay_diagnostic() {
    for state in [
        "{}",
        r#"{"drag_down":{"type":"int","value":1}}"#,
        r#"{"drag_down":{"type":"byte","value":2}}"#,
        r#"{"drag_down":{"type":"byte","value":0},"extra":{"type":"byte","value":0}}"#,
    ] {
        let (compiled, records) = compiled_columns(Some(state));
        let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
        for record in records
            .iter()
            .filter(|r| r.name.as_ref() == "minecraft:bubble_column")
        {
            assert_eq!(
                runtime
                    .resolve(NetworkIdMode::Sequential, record.sequential_id)
                    .kind(),
                VisualKind::Diagnostic
            );
        }
    }
}

#[test]
fn bubble_column_carriers_reject_truncated_groups_and_interior_references() {
    let (compiled, records) = compiled_columns(None);
    let id = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:bubble_column")
        .unwrap()
        .sequential_id as usize;
    let mut interior = compiled.clone();
    interior.visuals[id].model_template += 1;
    assert!(assets::encode_blob(&interior).is_err());
    let mut truncated = compiled.clone();
    truncated.model_templates =
        truncated.model_templates[..truncated.model_templates.len() - 1].into();
    assert!(assets::encode_blob(&truncated).is_err());
    let canonical = assets::encode_blob(&compiled).unwrap();
    let mut corrupted = canonical.to_vec();
    let visual_table = u64::from_le_bytes(corrupted[192..200].try_into().unwrap()) as usize;
    let reference = visual_table + id * assets::blob::VISUAL_BYTES + 32;
    let value = u32::from_le_bytes(corrupted[reference..reference + 4].try_into().unwrap()) + 1;
    corrupted[reference..reference + 4].copy_from_slice(&value.to_le_bytes());
    use sha2::Digest;
    let digest = sha2::Sha256::digest(&corrupted[..corrupted.len() - 32]);
    let len = corrupted.len();
    corrupted[len - 32..].copy_from_slice(&digest);
    let error = RuntimeAssets::decode(&corrupted)
        .err()
        .expect("interior group reference rejected");
    assert!(format!("{error:?}").contains("topology-group base"));
}

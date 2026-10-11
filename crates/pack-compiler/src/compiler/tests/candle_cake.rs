use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Selects every active candle cake and builds an original pack with separate lit sprites.
fn fixture(root: &Path) -> (Vec<RegistryRecord>, Box<[assets::LightProperties]>, u32) {
    let (all, lights, protocol) = super::shelf_cake::active_records(&[]);
    let records = all
        .into_iter()
        .filter(|record| {
            matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
                || record.name.ends_with("candle_cake")
        })
        .collect::<Vec<_>>();
    super::shelf_cake::cake_pack(root);
    let mut blocks: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("blocks.json")).unwrap()).unwrap();
    let mut terrain: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("textures/terrain_texture.json")).unwrap())
            .unwrap();
    blocks["stone"] = serde_json::json!({"textures":"stone"});
    terrain["texture_data"]["stone"] = serde_json::json!({"textures":"textures/blocks/stone"});
    write_png(
        root.join("textures/blocks/stone.png"),
        16,
        16,
        &[100, 100, 100, 255].repeat(256),
    );
    for (index, record) in records
        .iter()
        .filter(|record| record.name.ends_with("candle_cake"))
        .enumerate()
    {
        let name = record.name.strip_prefix("minecraft:").unwrap();
        let candle = name.strip_suffix("_cake").unwrap();
        blocks[name] = blocks["cake"].clone();
        blocks[candle] = serde_json::json!({"textures":candle});
        terrain["texture_data"][candle] = serde_json::json!({"textures":[format!("textures/fixture/{candle}"),format!("textures/fixture/{candle}_lit")]});
        for (suffix, color) in [
            ("", [40 + index as u8 * 5, 90, 60, 255]),
            ("_lit", [40 + index as u8 * 5, 180, 60, 255]),
        ] {
            write_png(
                root.join(format!("textures/fixture/{candle}{suffix}.png")),
                16,
                16,
                &color.repeat(256),
            );
        }
    }
    write(
        root.join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    write(
        root.join("textures/terrain_texture.json"),
        serde_json::to_vec(&terrain).unwrap(),
    );
    (records, lights, protocol)
}

/// Compiles a bounded record set and exercises the carrier decoder.
fn compile(
    root: &Path,
    records: &[RegistryRecord],
    lights: &[assets::LightProperties],
    protocol: u32,
) -> RuntimeAssets {
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        root,
        records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap()
}

#[test]
fn all_candle_cakes_keep_cake_candle_and_lit_texture_through_the_carrier() {
    let directory = tempfile::tempdir().unwrap();
    let (records, lights, protocol) = fixture(directory.path());
    assert_eq!(
        records
            .iter()
            .filter(|record| record.name.ends_with("candle_cake"))
            .count(),
        34
    );
    let runtime = compile(directory.path(), &records, &lights, protocol);
    let mut variants = BTreeMap::new();
    for record in records
        .iter()
        .filter(|record| record.name.ends_with("candle_cake"))
    {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            VisualKind::Model,
            "{} {}",
            record.name,
            record.canonical_state
        );
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(
            quads.len(),
            16,
            "cake, candle body, two double-sided wick planes"
        );
        assert_eq!(visual.support(), VisualSupport::Exact);
        assert!(
            quads
                .iter()
                .all(|quad| quad.material != assets::DIAGNOSTIC_MATERIAL)
        );
        let cake = &quads[..6];
        assert_eq!(
            cake.iter()
                .flat_map(|quad| quad.positions)
                .map(|p| p[1])
                .max(),
            Some(128)
        );
        let body = &quads[6..12];
        for (axis, min, max) in [(0, 112, 144), (1, 128, 224), (2, 112, 144)] {
            assert_eq!(
                body.iter()
                    .flat_map(|quad| quad.positions)
                    .map(|p| p[axis])
                    .min(),
                Some(min)
            );
            assert_eq!(
                body.iter()
                    .flat_map(|quad| quad.positions)
                    .map(|p| p[axis])
                    .max(),
                Some(max)
            );
        }
        assert!(body.iter().all(|quad| quad.material == body[0].material));
        for (face, quad) in assets::BlockFace::ALL.into_iter().zip(body) {
            let (min_v, max_v) = match face {
                assets::BlockFace::Down => (14 * 256, 16 * 256),
                assets::BlockFace::Up => (6 * 256, 8 * 256),
                _ => (8 * 256, 14 * 256),
            };
            assert_eq!(quad.uvs.iter().map(|uv| uv[0]).min(), Some(0));
            assert_eq!(quad.uvs.iter().map(|uv| uv[0]).max(), Some(2 * 256));
            assert_eq!(quad.uvs.iter().map(|uv| uv[1]).min(), Some(min_v));
            assert_eq!(quad.uvs.iter().map(|uv| uv[1]).max(), Some(max_v));
        }
        assert!(
            quads[12..]
                .iter()
                .all(|quad| quad.material == body[0].material)
        );
        assert!(
            quads[12..]
                .iter()
                .flat_map(|quad| quad.uvs)
                .all(|uv| uv[0] <= 256 && (5 * 256..=6 * 256).contains(&uv[1]))
        );
        assert_eq!(
            quads[12..]
                .iter()
                .flat_map(|quad| quad.positions)
                .map(|p| p[1])
                .max(),
            Some(240)
        );
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        variants.insert(
            (record.name.clone(), state["lit"]["value"].as_u64().unwrap()),
            body[0].material,
        );
        assert!(
            quads
                .iter()
                .all(|quad| runtime.material(quad.material).flags
                    & assets::MATERIAL_FLAG_ALPHA_CUTOUT
                    != 0)
        );
    }
    assert_eq!(variants.len(), 34);
    assert_eq!(
        variants
            .values()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        34,
        "every color/lit pair uses its own authored sprite"
    );
    for ((name, lit), material) in &variants {
        assert_ne!(
            *material,
            variants[&(name.clone(), 1 - lit)],
            "lit texture must switch"
        );
    }
}

#[test]
fn malformed_candle_cakes_cannot_inherit_a_collision_cube() {
    let directory = tempfile::tempdir().unwrap();
    let (records, lights, protocol) = fixture(directory.path());
    let valid = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:white_candle_cake")
        .unwrap();
    for state in [
        r#"{}"#,
        r#"{"lit":{"type":"int","value":0}}"#,
        r#"{"lit":{"type":"byte","value":2}}"#,
        r#"{"lit":{"type":"byte","value":0},"bite_counter":{"type":"int","value":1}}"#,
    ] {
        let mut changed = records.clone();
        changed
            .iter_mut()
            .find(|record| record.sequential_id == valid.sequential_id)
            .unwrap()
            .canonical_state = state.into();
        let runtime = compile(directory.path(), &changed, &lights, protocol);
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Sequential, valid.sequential_id)
                .kind(),
            VisualKind::Diagnostic,
            "{state}"
        );
    }
    let mut changed = records.clone();
    changed
        .iter_mut()
        .find(|record| record.sequential_id == valid.sequential_id)
        .unwrap()
        .collision_seed
        .boxes[0]
        .max_y += 1;
    let runtime = compile(directory.path(), &changed, &lights, protocol);
    assert_eq!(
        runtime
            .resolve(NetworkIdMode::Sequential, valid.sequential_id)
            .kind(),
        VisualKind::Diagnostic
    );
}

#[test]
fn candle_cakes_respect_static_aliases_and_missing_color_routes() {
    let directory = tempfile::tempdir().unwrap();
    let (records, lights, protocol) = fixture(directory.path());
    let path = directory.path().join("blocks.json");
    let mut blocks: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    blocks["white_candle"]["textures"] = serde_json::json!("shared_candle");
    blocks.as_object_mut().unwrap().remove("pink_candle");
    write(&path, serde_json::to_vec(&blocks).unwrap());
    let path = directory.path().join("textures/terrain_texture.json");
    let mut terrain: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    terrain["texture_data"]["shared_candle"] =
        serde_json::json!({"textures":"textures/fixture/white_candle"});
    write(path, serde_json::to_vec(&terrain).unwrap());
    let runtime = compile(directory.path(), &records, &lights, protocol);
    for record in records.iter().filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:white_candle_cake" | "minecraft:pink_candle_cake"
        )
    }) {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            if record.name.as_ref() == "minecraft:pink_candle_cake" {
                VisualKind::Diagnostic
            } else {
                VisualKind::Model
            }
        );
    }
}

#[test]
fn lighting_candle_cakes_preserves_weighted_static_texture_aliases() {
    for singleton_array in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let (records, lights, protocol) = fixture(directory.path());
        let path = directory.path().join("textures/terrain_texture.json");
        let mut terrain: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let group = serde_json::json!({"variations":[
            {"path":"textures/fixture/white_candle","weight":1},
            {"path":"textures/fixture/yellow_candle","weight":3}
        ]});
        terrain["texture_data"]["white_candle"]["textures"] = if singleton_array {
            serde_json::json!([group])
        } else {
            group
        };
        write(path, serde_json::to_vec(&terrain).unwrap());
        let runtime = compile(directory.path(), &records, &lights, protocol);
        for record in records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:white_candle_cake")
        {
            let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
            let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
            let material =
                runtime.material(runtime.model_quads()[template.quad_start as usize + 6].material);
            assert_eq!(
                material.variation_count, 2,
                "{} singleton_array={singleton_array}",
                record.canonical_state
            );
            let alternatives = &runtime.materials()[material.variation_start as usize..][..2];
            assert_ne!(alternatives[0].texture, alternatives[1].texture);
            assert_eq!(f32::from_bits(alternatives[0].variation_weight), 0.25);
            assert_eq!(f32::from_bits(alternatives[1].variation_weight), 0.75);
        }
    }
}

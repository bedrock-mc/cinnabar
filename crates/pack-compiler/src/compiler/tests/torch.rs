use super::super::classification::is_torch;
use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Every torch-shaped block compiles to a template lit flat by its own cell, with materials that
/// skip ambient occlusion and face dimming, as the native torch tessellation does.
#[test]
fn torches_compile_flat_lit_undimmed_templates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry_bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&registry_bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry_bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            is_torch(record) || matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let blocks = records
        .iter()
        .filter_map(|record| record.name.strip_prefix("minecraft:"))
        .filter(|name| *name != "air")
        .map(|name| (name.to_owned(), serde_json::json!({"textures": "art"})))
        .collect::<serde_json::Map<_, _>>();
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        &serde_json::Value::Object(blocks).to_string(),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"art":{"textures":"textures/blocks/art"}}}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    write_png(
        directory.path().join("textures/blocks/art.png"),
        TILE_SIZE,
        TILE_SIZE,
        &[200, 160, 40, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
    );
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let torches = records.iter().filter(|record| is_torch(record)).count();
    assert!(torches > 0);
    let flat = assets::MATERIAL_FLAG_DISABLE_AO | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING;
    for record in records.iter().filter(|record| is_torch(record)) {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        assert_eq!(
            template.flags,
            assets::MODEL_TEMPLATE_FLAG_TORCH,
            "{} {}",
            record.name,
            record.canonical_state
        );
        let start = template.quad_start as usize;
        for quad in &runtime.model_quads()[start..start + template.quad_count as usize] {
            assert_eq!(
                runtime.material(quad.material).flags & flat,
                flat,
                "{} {}",
                record.name,
                record.canonical_state
            );
        }
    }
}

use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

#[test]
fn current_heavy_core_uses_the_fixed_vanilla_model() {
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
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:heavy_core"
            )
        })
        .collect::<Vec<_>>();
    let heavy_core = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:heavy_core")
        .unwrap();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{"stone":{"textures":"stone"},"heavy_core":{"textures":"heavy_core"}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"stone":{"textures":"textures/blocks/stone"},"heavy_core":{"textures":"textures/blocks/heavy_core"}}}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    write_png(
        directory.path().join("textures/blocks/heavy_core.png"),
        TILE_SIZE,
        TILE_SIZE,
        &[40, 20, 60, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
    );
    write_png(
        directory.path().join("textures/blocks/stone.png"),
        TILE_SIZE,
        TILE_SIZE,
        &[80, 80, 80, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
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
    let visual = runtime.resolve(NetworkIdMode::Sequential, heavy_core.sequential_id);
    assert_eq!(visual.kind(), VisualKind::Model);
    assert_eq!(visual.support(), VisualSupport::Exact);
    let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
    assert_eq!(template.quad_count, 6);
    let quads = &runtime.model_quads()[template.quad_start as usize..][..6];
    for axis in 0..3 {
        assert_eq!(
            quads
                .iter()
                .flat_map(|quad| quad.positions.map(|position| position[axis]))
                .min(),
            Some([64, 0, 64][axis])
        );
        assert_eq!(
            quads
                .iter()
                .flat_map(|quad| quad.positions.map(|position| position[axis]))
                .max(),
            Some([192, 128, 192][axis])
        );
    }
    for quad in quads {
        assert_eq!(quad.flags & assets::MODEL_QUAD_FLAG_CULL_FACE_MASK, 0);
    }
}

use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

#[test]
fn lantern_body_samples_its_authored_sprite_after_carrier_roundtrip() {
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
                "minecraft:air"
                    | "minecraft:stone"
                    | "minecraft:lantern"
                    | "minecraft:soul_lantern"
            )
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{"stone":{"textures":"stone"},"lantern":{"textures":"lantern"},"soul_lantern":{"textures":"lantern"}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"stone":{"textures":"textures/blocks/stone"},"lantern":{"textures":"textures/blocks/lantern"}}}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    write_png(
        directory.path().join("textures/blocks/stone.png"),
        TILE_SIZE,
        TILE_SIZE,
        &[100, 100, 100, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
    );
    let mut pixels = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
    let body = [255, 120, 40, 255];
    for y in 2..9 {
        for x in 0..6 {
            let offset = ((y * TILE_SIZE + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&body);
        }
    }
    write_png(
        directory.path().join("textures/blocks/lantern.png"),
        TILE_SIZE,
        TILE_SIZE,
        &pixels,
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
    for record in records.iter().filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:lantern" | "minecraft:soul_lantern"
        )
    }) {
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(visual.support(), VisualSupport::VanillaFallback);
        assert_eq!(
            runtime
                .resolve(NetworkIdMode::Hashed, record.network_hash)
                .model_template(),
            visual.model_template()
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let face = runtime.model_quads()[template.quad_start as usize];
        let uv = [0, 1].map(|axis| face.uvs.iter().map(|uv| u32::from(uv[axis])).sum::<u32>() / 4);
        let [x, y] = uv.map(|value| value * TILE_SIZE / 4096);
        let offset = ((y * TILE_SIZE + x) * 4) as usize;
        assert_eq!(
            &pixels[offset..offset + 4],
            &body,
            "{} {}: body face samples an opaque lantern pixel",
            record.name,
            record.canonical_state,
        );
        assert!(
            template.quad_count > 6,
            "cap and handle survive compilation"
        );
    }
}

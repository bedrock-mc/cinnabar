use std::{fs, path::Path};

use image::{Rgba, RgbaImage};
use serde_json::Value;

use super::*;

#[test]
fn current_registry_ice_supersedes_fallback_alpha_and_geometry() {
    let target: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../assets/bedrock-target.json"
    ))
    .expect("active target manifest");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
    let registry = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .expect("active committed registry");
    let records = registry
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:ice" | "minecraft:frosted_ice" | "minecraft:air" | "minecraft:stone"
            )
        })
        .collect::<Vec<_>>();
    let fallback = inventory(protocol).expect("active fallback table");
    let ice = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:ice")
        .expect("real ice identity");
    // Invented hashes miss this stale cutout envelope and cannot reproduce
    // the opaque ice seen in play. Keep the shipped identity as the witness.
    assert_eq!(fallback.entry_unfiltered(ice).unwrap().2, ALPHA_CUTOUT);

    let directory = tempfile::tempdir().expect("original translucent cube pack");
    fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
    let blocks = records
        .iter()
        .filter(|record| record.name.as_ref() != "minecraft:air")
        .map(|record| {
            (
                record.name.strip_prefix("minecraft:").unwrap(),
                serde_json::json!({"textures": if record.name.as_ref() == "minecraft:stone" { "stone" } else { "test_cube" }}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    fs::write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"test_cube":{"textures":["textures/blocks/test_cube","textures/blocks/test_cube","textures/blocks/test_cube","textures/blocks/test_cube"]},"stone":{"textures":"textures/blocks/stone"}}}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    )
    .unwrap();
    let source_alpha = 190;
    RgbaImage::from_pixel(
        assets::TILE_SIZE,
        assets::TILE_SIZE,
        Rgba([120, 140, 180, source_alpha]),
    )
    .save(directory.path().join("textures/blocks/test_cube.png"))
    .unwrap();
    RgbaImage::from_pixel(
        assets::TILE_SIZE,
        assets::TILE_SIZE,
        Rgba([100, 100, 100, u8::MAX]),
    )
    .save(directory.path().join("textures/blocks/stone.png"))
    .unwrap();
    let lights = vec![
        assets::LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap()
    ];
    let (compiled, _) = compile_pack_inner(
        directory.path(),
        &records,
        &lights,
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .expect("compile real translucent cube identities");
    for record in
        std::iter::once(ice).chain(records.iter().filter(|record| {
            is_translucent_cube(record) && record.name.as_ref() != "minecraft:ice"
        }))
    {
        let visual = compiled.visuals[record.sequential_id as usize];
        let expected = translucent_cube_material_flags(&record.name);
        for material in visual.faces {
            assert_eq!(
                compiled.materials[material as usize].flags
                    & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT),
                expected,
                "{} must use its reviewed material, not its fallback envelope",
                record.name
            );
        }
        assert_eq!(fallback.entry(record), None);
        assert_eq!(fallback.material_flags(record), None);
        assert_ne!(visual.support, VisualSupport::VanillaFallback);
        assert_eq!(
            visual.kind,
            VisualKind::Model,
            "{} {}",
            record.name,
            record.canonical_state
        );
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(template.flags, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE);
        assert_eq!(template.quad_count, 6);
    }
    let decoded = assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let face = decoded
        .resolve(assets::NetworkIdMode::Hashed, ice.network_hash)
        .face(BlockFace::Up);
    let material = decoded.material(face.material_id());
    let texture = material.texture;
    let mip = &decoded.texture_pages()[texture.page() as usize]
        .texture
        .mips[0];
    let layer_bytes = (mip.size * mip.size * 4) as usize;
    let start = texture.layer() as usize * layer_bytes;
    assert!(pixels_alpha(
        &mip.rgba8[start..start + layer_bytes],
        source_alpha
    ));
}

fn pixels_alpha(pixels: &[u8], expected: u8) -> bool {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == expected)
}

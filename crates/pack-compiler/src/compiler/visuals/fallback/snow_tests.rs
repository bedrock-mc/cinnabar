use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::Value;

use super::*;

#[test]
fn current_registry_snow_bypasses_matching_blended_fallback_geometry_and_materials() {
    let target: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../assets/bedrock-target.json"
    ))
    .expect("active target manifest");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry_path = root.join(target["artifacts"]["block_registry"].as_str().unwrap());
    let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
    let registry = assets::read_registry_for_protocol(&fs::read(registry_path).unwrap(), protocol)
        .expect("active committed registry");
    let records = registry
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:snow_layer" | "minecraft:stone" | "minecraft:air"
            )
        })
        .collect::<Vec<_>>();
    let snow = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:snow_layer")
        .collect::<Vec<_>>();
    assert!(!snow.is_empty());
    let fallback = inventory(protocol).expect("active fallback table");
    for record in &snow {
        // The actual network hash/fingerprint must hit the old envelope, or
        // this test would merely repeat synthetic fixtures that missed the bug.
        assert_eq!(fallback.entry_unfiltered(record).unwrap().2, ALPHA_BLEND);
        assert_eq!(fallback.entry(record), None);
        assert_eq!(fallback.material_flags(record), None);
    }
    let directory = tempfile::tempdir().expect("original opaque snow test pack");
    fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
    fs::write(
        directory.path().join("blocks.json"),
        r#"{"snow_layer":{"textures":"snow"},"stone":{"textures":"snow"}}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"snow":{"textures":"textures/blocks/snow"}}}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    )
    .unwrap();
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&[255; 16 * 16 * 4], 16, 16, ExtendedColorType::Rgba8)
        .unwrap();
    fs::write(directory.path().join("textures/blocks/snow.png"), png).unwrap();
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
    .expect("compile actual snow fingerprints");
    for record in snow {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_ne!(visual.support, VisualSupport::VanillaFallback);
        for material in visual.faces {
            assert_eq!(
                compiled.materials[material as usize].flags
                    & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT),
                0
            );
        }
        let height = canonical_state_u32(&record.canonical_state, "height").unwrap();
        if height + 1 == u32::from(assets::TOP_SNOW_LAYER_COUNT) {
            assert_eq!(visual.kind, VisualKind::Cube);
            assert!(visual.flags.contains(BlockFlags::OCCLUDES_FULL_FACE));
        } else {
            assert_eq!(visual.kind, VisualKind::Model);
            assert_eq!(
                compiled.model_templates[visual.model_template as usize].flags,
                assets::MODEL_TEMPLATE_FLAG_SNOW_LAYER
            );
        }
    }
    let decoded = assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    assert!(!decoded.model_templates().is_empty());
}

use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::Value;

use super::*;

#[test]
fn current_registry_leaf_litter_takes_dry_foliage_tint_on_its_fallback_envelope() {
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
                "minecraft:leaf_litter" | "minecraft:stone" | "minecraft:air"
            )
        })
        .collect::<Vec<_>>();
    let litter = records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:leaf_litter")
        .collect::<Vec<_>>();
    assert!(!litter.is_empty());

    let directory = tempfile::tempdir().expect("leaf litter test pack");
    fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
    fs::write(
        directory.path().join("blocks.json"),
        r#"{"leaf_litter":{"textures":"leaf_litter"},"stone":{"textures":"stone"}}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "leaf_litter":{"textures":["textures/blocks/leaf_litter"]},
            "stone":{"textures":"textures/blocks/stone"}
        }}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    )
    .unwrap();
    for (name, alpha) in [("leaf_litter", 0), ("stone", 255)] {
        let mut rgba = [160; 16 * 16 * 4];
        for (index, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            pixel[3] = if index % 2 == 0 { 255 } else { alpha };
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .unwrap();
        fs::write(
            directory.path().join(format!("textures/blocks/{name}.png")),
            png,
        )
        .unwrap();
    }
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
    .expect("compile actual leaf litter fingerprints");
    for record in litter {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.support, VisualSupport::VanillaFallback);
        for material in visual.faces {
            let flags = compiled.materials[material as usize].flags;
            assert_eq!(
                flags
                    & (assets::MATERIAL_FLAG_TINT_MASK | assets::MATERIAL_FLAG_FOLIAGE_CLASS_MASK),
                MATERIAL_FLAG_FOLIAGE_TINT | MATERIAL_FLAG_DRY_FOLIAGE,
                "{} must take the biome dry-foliage colour",
                record.canonical_state
            );
        }
    }
}

use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::Value;

use super::*;

#[test]
fn current_registry_leaf_litter_uses_state_driven_patches_with_dry_foliage_tint() {
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
        assert_eq!(visual.support, VisualSupport::Exact);
        assert_eq!(visual.kind, VisualKind::Model);
        let growth = canonical_state_u32(&record.canonical_state, "growth").unwrap();
        let template = compiled.model_templates[visual.model_template as usize];
        assert_eq!(
            template.quad_count,
            growth.min(3) + 1,
            "{}",
            record.canonical_state
        );
        let quads =
            &compiled.model_quads[template.quad_start as usize..][..template.quad_count as usize];
        for quad in quads {
            assert!(quad.positions.iter().all(|position| position[1] == 4));
            assert_eq!(quad.flags, MODEL_QUAD_FLAG_TWO_SIDED);
            let min_x = quad
                .positions
                .iter()
                .map(|position| position[0])
                .min()
                .unwrap();
            let max_x = quad
                .positions
                .iter()
                .map(|position| position[0])
                .max()
                .unwrap();
            let min_z = quad
                .positions
                .iter()
                .map(|position| position[2])
                .min()
                .unwrap();
            let max_z = quad
                .positions
                .iter()
                .map(|position| position[2])
                .max()
                .unwrap();
            assert_eq!([max_x - min_x, max_z - min_z], [128; 2]);
        }
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

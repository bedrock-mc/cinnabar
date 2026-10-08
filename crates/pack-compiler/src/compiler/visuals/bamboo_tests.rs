use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::Value;

use super::*;

#[test]
fn bamboo_stem_uses_the_stem_selector_on_every_surface() {
    let target: Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
    let records = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap()
    .into_iter()
    .filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:bamboo" | "minecraft:stone" | "minecraft:air"
        )
    })
    .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
    fs::write(
        directory.path().join("blocks.json"),
        r#"{"bamboo":{"textures":{"north":"stem","west":"stem","east":"single_leaf","south":"small_leaf","up":"leaf","down":"sapling"}},"stone":{"textures":"stem"}}"#,
    )
    .unwrap();
    let mut texture_data = serde_json::Map::new();
    for (index, key) in ["stem", "single_leaf", "small_leaf", "leaf", "sapling"]
        .into_iter()
        .enumerate()
    {
        texture_data.insert(
            key.into(),
            serde_json::json!({"textures":format!("textures/blocks/{key}")}),
        );
        let mut png = Vec::new();
        let pixel = [40 + index as u8 * 30, 150, 50, 255];
        PngEncoder::new(&mut png)
            .write_image(&pixel.repeat(16 * 16), 16, 16, ExtendedColorType::Rgba8)
            .unwrap();
        fs::write(
            directory.path().join(format!("textures/blocks/{key}.png")),
            png,
        )
        .unwrap();
    }
    fs::write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data":texture_data})).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    )
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
    .unwrap();
    let mut witnessed = 0;
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:bamboo")
    {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.support, VisualSupport::VanillaFallback);
        let stem = visual.faces[BlockFace::North as usize];
        assert!(
            visual.faces.iter().all(|&material| material == stem),
            "{}",
            record.canonical_state
        );
        let template = compiled.model_templates[visual.model_template as usize];
        let quads =
            &compiled.model_quads[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(
            quads.len(),
            if record.canonical_state.contains("no_leaves") {
                6
            } else {
                10
            }
        );
        assert!(quads[..6].iter().all(|quad| quad.material == stem));
        assert_eq!(template.flags, assets::MODEL_TEMPLATE_FLAG_BAMBOO);
        let width = if record.canonical_state.contains("thin") {
            32
        } else {
            48
        };
        let stem_points = quads[..6].iter().flat_map(|quad| quad.positions);
        assert_eq!(stem_points.clone().map(|point| point[0]).min(), Some(128));
        assert_eq!(stem_points.map(|point| point[0]).max(), Some(128 + width));
        if quads.len() > 6 {
            assert!(
                quads[6..]
                    .iter()
                    .all(|quad| quad.material != stem && quad.flags & 8 != 0)
            );
            assert!(
                quads[6..]
                    .iter()
                    .all(|quad| quad.material == quads[6].material)
            );
            let large = record.canonical_state.contains("large_leaves");
            assert_eq!(
                quads[6].positions[1][0],
                128 + width + if large { 112 } else { 80 }
            );
            assert_eq!(quads[6].uvs[1], [if large { 0 } else { 512 }, 0]);
            assert_eq!(quads[7].uvs[1], [if large { 4096 } else { 3584 }, 0]);
        }
        assert!(!visual.flags.contains(BlockFlags::OCCLUDES_FULL_FACE));
        witnessed += 1;
    }
    assert!(witnessed > 0);
}

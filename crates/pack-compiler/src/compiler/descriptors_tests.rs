use assets::{NetworkIdMode, RuntimeAssets};
use image::{Rgba, RgbaImage};
use std::fs;
use {super::*, assets::BlockFace};

#[test]
fn pots_and_lanterns_keep_cutout_materials_after_carrier_roundtrip() {
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../assets/bedrock-target.json")).unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let records = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap()
    .into_iter()
    .filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:air"
                | "minecraft:stone"
                | "minecraft:dirt"
                | "minecraft:flower_pot"
                | "minecraft:lantern"
                | "minecraft:soul_lantern"
        )
    })
    .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("textures/blocks")).unwrap();
    fs::write(root.join("blocks.json"), r#"{"stone":{"textures":"solid"},"dirt":{"textures":"solid"},"flower_pot":{"textures":"model"},"lantern":{"textures":"model"},"soul_lantern":{"textures":"model"}}"#).unwrap();
    fs::write(root.join("textures/terrain_texture.json"), r#"{"texture_data":{"solid":{"textures":"textures/blocks/solid"},"model":{"textures":"textures/blocks/model"}}}"#).unwrap();
    fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    RgbaImage::from_pixel(
        assets::TILE_SIZE,
        assets::TILE_SIZE,
        Rgba([90, 65, 40, 255]),
    )
    .save(root.join("textures/blocks/solid.png"))
    .unwrap();
    let mut sprite = RgbaImage::from_pixel(
        assets::TILE_SIZE,
        assets::TILE_SIZE,
        Rgba([180, 95, 40, 255]),
    );
    for x in 0..assets::TILE_SIZE {
        sprite.put_pixel(x, 0, Rgba([0, 0, 0, 0]));
    }
    sprite.save(root.join("textures/blocks/model.png")).unwrap();
    let lights = vec![
        assets::LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap()
    ];
    let (compiled, _) = compile_pack_inner(
        root,
        &records,
        &lights,
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for name in [
        "minecraft:flower_pot",
        "minecraft:lantern",
        "minecraft:soul_lantern",
    ] {
        let models = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert!(!models.is_empty(), "fixture must contain {name}");
        for record in models {
            let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
            let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
            let quads = &runtime.model_quads()[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            let body = BlockFace::ALL.map(|face| visual.face(face).material_id());
            let surfaces = quads
                .iter()
                .filter(|quad| body.contains(&quad.material))
                .collect::<Vec<_>>();
            assert!(!surfaces.is_empty(), "{name} must contain model surfaces");
            for quad in surfaces {
                let alpha = runtime.material(quad.material).flags
                    & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT);
                assert_eq!(
                    alpha, MATERIAL_FLAG_ALPHA_CUTOUT,
                    "{} {} needs alpha testing",
                    record.name, record.canonical_state
                );
                assert_ne!(
                    quad.flags & MODEL_QUAD_FLAG_TWO_SIDED,
                    0,
                    "{} {} must draw both sides of alpha-tested surfaces",
                    record.name,
                    record.canonical_state
                );
            }
        }
    }
}

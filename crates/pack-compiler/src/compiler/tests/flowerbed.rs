use super::*;
use assets::{CompiledBiomeAssets, MATERIAL_FLAG_GRASS_TINT, ModelFamily, RuntimeAssets};

#[test]
fn flowerbed_stems_use_grass_tint_while_petals_keep_their_sprite_colours() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap();
    let records: Vec<_> = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            record.model_family == ModelFamily::FlowerBed || record.name.as_ref() == "minecraft:air"
        })
        .collect();
    assert!(
        records
            .iter()
            .any(|record| record.model_family == ModelFamily::FlowerBed)
    );
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{"wildflowers":{"textures":"flower"},"pink_petals":{"textures":"flower"}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"flower":{"textures":["textures/petals","textures/stems"]}}}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let petals = [220, 120, 80, 255];
    let stems = [255, 255, 255, 255];
    for (name, colour) in [("petals", petals), ("stems", stems)] {
        write_png(
            directory.path().join(format!("textures/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &colour.repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &vec![assets::LightProperties::default(); span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    for record in records
        .iter()
        .filter(|record| record.model_family == ModelFamily::FlowerBed)
    {
        let visual = compiled.visuals[record.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Model);
        let template = compiled.model_templates[visual.model_template as usize];
        let quads = &compiled.model_quads
            [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
        let mut saw_stem = false;
        let mut saw_petals = false;
        for quad in quads {
            let material = compiled.materials[quad.material as usize];
            let reference = material.texture;
            let mip = &compiled.texture_pages[reference.page() as usize]
                .texture
                .mips[0];
            let offset = (reference.layer() * mip.size * mip.size * 4) as usize;
            let colour = &mip.rgba8[offset..offset + 4];
            if colour == stems {
                saw_stem = true;
                assert_ne!(material.flags & MATERIAL_FLAG_GRASS_TINT, 0, "{record:?}");
            } else {
                saw_petals = true;
                assert_eq!(colour, petals);
                assert_eq!(material.flags & MATERIAL_FLAG_GRASS_TINT, 0, "{record:?}");
            }
        }
        assert!(saw_stem && saw_petals);
    }
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    assert_eq!(runtime.materials(), compiled.materials.as_ref());
}

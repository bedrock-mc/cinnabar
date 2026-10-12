use super::*;
use assets::{BlockVisualId, CompiledAssets, RegistryRecord};
use std::fs;

/// Compiles real barrel selectors with original opaque sprite artwork for held-consumer checks.
pub(crate) fn barrels() -> (CompiledAssets, Vec<RegistryRecord>) {
    barrels_with_size(assets::TILE_SIZE)
}

/// Compiles original sprites at the requested source resolution.
pub(crate) fn barrels_with_size(side: u32) -> (CompiledAssets, Vec<RegistryRecord>) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("biomes")).unwrap();
    fs::create_dir_all(root.join("behavior/biomes")).unwrap();
    fs::create_dir_all(root.join("textures/colormap")).unwrap();
    fs::write(
        root.join("biomes/plain.client_biome.json"),
        r#"{"minecraft:client_biome":{"description":{"identifier":"plain"},"components":{}}}"#,
    )
    .unwrap();
    fs::write(root.join("behavior/biomes/plain.biome.json"),r#"{"minecraft:biome":{"description":{"identifier":"plain"},"components":{"minecraft:climate":{"temperature":0.8,"downfall":0.4}}}}"#).unwrap();
    fs::write(root.join("blocks.json"), r#"{"barrel":{"textures":"art"}}"#).unwrap();
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"art":{"textures":"textures/art"}}}"#,
    )
    .unwrap();
    fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    image::RgbaImage::from_fn(side, side, |x, y| {
        image::Rgba([20, (x * 192 / side) as u8, (y * 192 / side) as u8, 255])
    })
    .save(root.join("textures/art.png"))
    .unwrap();
    for map in assets::TintMapId::ALL {
        image::RgbaImage::from_pixel(
            assets::TINT_MAP_SIZE,
            assets::TINT_MAP_SIZE,
            image::Rgba([255; 4]),
        )
        .save(root.join(format!("textures/colormap/{}.png", map.source_name())))
        .unwrap();
    }
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .unwrap()
            .into_vec()
            .into_iter()
            .filter(|r| matches!(r.name.as_ref(), "minecraft:air" | "minecraft:barrel"))
            .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let compiled = pack_compiler::compile_pack_with_biomes(
        root,
        &root.join("behavior"),
        &records,
        &[],
        &vec![assets::LightProperties::default(); span],
        protocol,
    )
    .unwrap();
    (compiled, records)
}

#[test]
fn opaque_barrel_models_remain_available_in_first_person() {
    let (compiled, records) = barrels();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut count = 0;
    for record in records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:barrel")
    {
        let (geometry, skin) =
            ViewmodelGeometry::opaque_cube(&runtime, BlockVisualId(record.sequential_id))
                .expect("opaque full-cube barrel models retain held geometry");
        assert_eq!(geometry.vertices.len(), 36);
        assert!(geometry.cube_origin);
        assert!(
            skin.rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] == 255)
        );
        let visual = runtime.resolve(assets::NetworkIdMode::Sequential, record.sequential_id);
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        for vertex in geometry.vertices.iter() {
            let pixel = vertex
                .uv
                .map(|coordinate| coordinate * VIEWMODEL_TEXTURE_SIDE as f32);
            let slot = pixel.map(|coordinate| coordinate as usize / TILE_PITCH);
            let face = slot[1] * TILE_COLUMNS + slot[0];
            let uv = std::array::from_fn::<_, 2, _>(|axis| {
                ((pixel[axis] - (slot[axis] * TILE_PITCH + 1) as f32) / TILE_SIDE as f32 * 4096.0)
                    .round() as u16
            });
            let position = (opaque_cube_pose()
                .inverse()
                .transform_point3(Vec3::from_array(vertex.position))
                + Vec3::splat(0.5))
            .to_array()
            .map(|component| (component * 256.0).round() as i16);
            let corner = quads[face]
                .positions
                .iter()
                .position(|&world| world == position)
                .expect("held vertex remains on its world cube face");
            assert_eq!(
                uv, quads[face].uvs[corner],
                "held UVs preserve the sprite at the same geometric corner"
            );
        }
        count += 1;
    }
    assert_eq!(count, 12);
}

#[test]
fn held_barrel_cube_admission_rejects_inset_or_folded_models() {
    let (compiled, records) = barrels();
    let record = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:barrel")
        .unwrap();
    let id = assets::BlockVisualId(record.sequential_id);
    let template =
        compiled.model_templates[compiled.visuals[id.0 as usize].model_template as usize];
    for folded in [false, true] {
        let mut invalid = compiled.clone();
        let quad = &mut invalid.model_quads[template.quad_start as usize];
        if folded {
            quad.uvs[1] = quad.uvs[0];
        } else {
            for point in &mut quad.positions {
                point[0] = 16;
            }
        }
        let runtime = RuntimeAssets::decode(&assets::encode_blob(&invalid).unwrap()).unwrap();
        assert!(ViewmodelGeometry::opaque_cube(&runtime, id).is_none());
    }
}

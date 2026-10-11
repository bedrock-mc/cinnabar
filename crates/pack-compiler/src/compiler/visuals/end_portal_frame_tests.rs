use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::Value;

use {super::*, assets::BlockFace};

fn compile_current_frames() -> (CompiledAssets, Vec<RegistryRecord>) {
    let target: Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
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
            is_record(record)
                || matches!(
                    record.name.as_ref(),
                    "minecraft:air"
                        | "minecraft:stone"
                        | assets::END_PORTAL_IDENTIFIER
                        | assets::END_GATEWAY_IDENTIFIER
                )
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
    fs::write(directory.path().join("blocks.json"), r#"{
        "end_portal_frame":{"carried_textures":"endframe_eye","textures":{"west":"endframe_side","east":"endframe_side","down":"endframe_bottom","up":"endframe_top","north":"endframe_side","south":"endframe_side"}},
        "stone":{"textures":"stone"}
    }"#).unwrap();
    fs::write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{
        "endframe_side":{"textures":"textures/blocks/endframe_side"},
        "endframe_bottom":{"textures":"textures/blocks/endframe_bottom"},
        "endframe_top":{"textures":"textures/blocks/endframe_top"},
        "endframe_eye":{"textures":"textures/blocks/endframe_eye"},
        "stone":{"textures":"textures/blocks/stone"}
    }}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    )
    .unwrap();
    // Original fixture art, with a unique eye color and opaque pixels.
    for (index, name) in [
        "endframe_side",
        "endframe_bottom",
        "endframe_top",
        "endframe_eye",
        "stone",
    ]
    .into_iter()
    .enumerate()
    {
        let pixel = [40 + index as u8 * 40, 180, 100, 255];
        let pixels = pixel.repeat((assets::TILE_SIZE * assets::TILE_SIZE) as usize);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(
                &pixels,
                assets::TILE_SIZE,
                assets::TILE_SIZE,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        fs::write(
            directory.path().join(format!("textures/blocks/{name}.png")),
            png,
        )
        .unwrap();
    }
    let (compiled, _) = compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

#[test]
fn current_end_portal_frame_states_compile_eye_geometry_and_carried_art() {
    let (compiled, records) = compile_current_frames();
    let runtime = assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut states = Vec::new();
    for record in records.iter().filter(|record| is_record(record)) {
        let direction = frame_direction(record).unwrap();
        let eye = canonical_state_u32(&record.canonical_state, "end_portal_eye_bit").unwrap();
        states.push((direction, eye));
        let visual = runtime.resolve(assets::NetworkIdMode::Sequential, record.sequential_id);
        let hashed = runtime.resolve(assets::NetworkIdMode::Hashed, record.network_hash);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(visual.support(), VisualSupport::Exact);
        assert_eq!(visual.model_template(), hashed.model_template());
        assert!(
            !visual
                .flags()
                .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        );
        let template = compiled.model_templates[visual.model_template().unwrap() as usize];
        let quads =
            &compiled.model_quads[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), if eye == 1 { 12 } else { 6 });
        assert_eq!(
            quads[BlockFace::Up as usize].positions.map(|p| p[1]),
            [208; 4]
        );
        if eye == 1 {
            let eye_quads = &quads[6..];
            let positions = eye_quads
                .iter()
                .flat_map(|quad| quad.positions)
                .collect::<Vec<_>>();
            for (axis, bounds) in [(0, (64, 192)), (1, (208, 256)), (2, (64, 192))] {
                assert_eq!(positions.iter().map(|p| p[axis]).min(), Some(bounds.0));
                assert_eq!(positions.iter().map(|p| p[axis]).max(), Some(bounds.1));
            }
            let eye_material = eye_quads[0].material;
            assert!(eye_quads.iter().all(|quad| quad.material == eye_material));
            assert_ne!(eye_material, quads[BlockFace::Up as usize].material);
            assert_eq!(
                compiled.materials[eye_material as usize].flags,
                MATERIAL_FLAG_ALPHA_CUTOUT
            );
            let texture_ref = compiled.materials[eye_material as usize].texture;
            let tile = &compiled.texture_pages[texture_ref.page() as usize]
                .texture
                .mips[0]
                .rgba8;
            let offset =
                texture_ref.layer() as usize * (assets::TILE_SIZE * assets::TILE_SIZE * 4) as usize;
            assert_eq!(&tile[offset..offset + 4], &[160, 180, 100, 255]);
        }
    }
    states.sort_unstable();
    assert_eq!(
        states,
        [
            (0, 0),
            (0, 1),
            (1, 0),
            (1, 1),
            (2, 0),
            (2, 1),
            (3, 0),
            (3, 1)
        ]
    );
}

#[test]
fn end_surfaces_never_compile_an_occluding_terrain_cube() {
    let (compiled, records) = compile_current_frames();
    let runtime = assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for record in records.iter().filter(|record| {
        matches!(
            record.name.as_ref(),
            assets::END_PORTAL_IDENTIFIER | assets::END_GATEWAY_IDENTIFIER
        )
    }) {
        for (mode, id) in [
            (assets::NetworkIdMode::Sequential, record.sequential_id),
            (assets::NetworkIdMode::Hashed, record.network_hash),
        ] {
            let visual = runtime.resolve(mode, id);
            assert_eq!(visual.kind(), VisualKind::Invisible);
            assert!(
                !visual
                    .flags()
                    .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
            );
        }
    }
}

#[test]
fn frame_top_and_eye_uvs_follow_native_direction_transform() {
    for direction in 0..4 {
        for (min, max) in [
            ([0, 0, 0], [256, 208, 256]),
            ([64, 208, 64], [192, 256, 192]),
        ] {
            let quads = frame_cuboid([1; 6], min, max, direction);
            let top = quads[BlockFace::Up as usize];
            let [west, east] = [min[0] as u16 * 16, max[0] as u16 * 16];
            let [north, south] = [min[2] as u16 * 16, max[2] as u16 * 16];
            let expected = match direction {
                0 => [
                    [4096 - west, 4096 - north],
                    [4096 - west, 4096 - south],
                    [4096 - east, 4096 - south],
                    [4096 - east, 4096 - north],
                ],
                1 => [
                    [4096 - north, west],
                    [4096 - south, west],
                    [4096 - south, east],
                    [4096 - north, east],
                ],
                2 => [[west, north], [west, south], [east, south], [east, north]],
                _ => [
                    [north, 4096 - west],
                    [south, 4096 - west],
                    [south, 4096 - east],
                    [north, 4096 - east],
                ],
            };
            assert_eq!(top.uvs, expected);
            assert_eq!(
                top.flags & assets::MODEL_QUAD_FLAG_CULL_FACE_MASK != 0,
                max[1] == 256
            );
        }
    }
}

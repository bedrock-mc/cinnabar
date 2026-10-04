use std::{fs, path::Path};

use assets::{NetworkIdMode, RegistryRecord, RuntimeAssets, TOP_SNOW_LAYER_COUNT};
use bevy::math::Vec3;
use render::{BlockSelectionFrame, BlockSelectionTarget, CrackShape, crack_shape_from_template};

fn fixture() -> (Vec<RegistryRecord>, RuntimeAssets) {
    let data = include_bytes!("../../assets/data/block-registry-v2193.bin");
    let protocol = assets::registry_header_protocol(data).unwrap();
    let records: Vec<_> = assets::read_registry_for_protocol(data, protocol)
        .unwrap()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:birch_stairs" | "minecraft:snow_layer"
            )
        })
        .enumerate()
        .map(|(id, mut record)| {
            record.sequential_id = id as u32;
            record
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    write_pack(directory.path());
    let lights = vec![assets::LightProperties::default(); records.len()];
    let compiled = pack_compiler::compile_pack(directory.path(), &records, &lights).unwrap();
    let assets = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    (records, assets)
}

fn write_pack(root: &Path) {
    fs::create_dir_all(root.join("textures/blocks")).unwrap();
    fs::write(
        root.join("blocks.json"),
        r#"{"birch_stairs":{"textures":"test"},"snow_layer":{"textures":"test"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"test":{"textures":"textures/blocks/test"}}}"#,
    )
    .unwrap();
    fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([127, 127, 127, 255]))
        .save(root.join("textures/blocks/test.png"))
        .unwrap();
}

fn selected_shape(
    assets: &RuntimeAssets,
    record: &RegistryRecord,
    mode: NetworkIdMode,
) -> CrackShape {
    let id = match mode {
        NetworkIdMode::Sequential => record.sequential_id,
        NetworkIdMode::Hashed => record.network_hash,
    };
    let visual = assets.resolve(mode, id);
    crack_shape_from_template(assets, visual.model_template().unwrap(), visual.variant()).unwrap()
}

#[test]
fn current_stair_selection_surface_uses_authoritative_corner_rotation_and_half() {
    let (records, assets) = fixture();
    // Native step/inner AABB witnesses, quadrant bits x+2*z.
    let occupied = [
        [10_u8, 11, 14, 2, 8],
        [5, 13, 7, 4, 1],
        [12, 14, 13, 8, 4],
        [3, 7, 11, 1, 2],
    ];
    let corners = [
        "none",
        "inner_left",
        "inner_right",
        "outer_left",
        "outer_right",
    ];
    for record in records
        .iter()
        .filter(|record| record.name.ends_with("_stairs"))
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let raw = state["weirdo_direction"]["value"].as_u64().unwrap() as usize;
        let half = state["upside_down_bit"]["value"].as_u64().unwrap() as usize;
        let corner = corners
            .iter()
            .position(|corner| state["minecraft:corner"]["value"] == *corner)
            .unwrap();
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let shape = selected_shape(&assets, record, mode);
            let CrackShape::Quads(quads) = &shape else {
                panic!("stairs must retain model faces")
            };
            for layer in 0..2 {
                for quadrant in 0..4 {
                    let [x, z] = [
                        0.25 + (quadrant & 1) as f32 * 0.5,
                        0.25 + (quadrant >> 1) as f32 * 0.5,
                    ];
                    let face_found = quads.iter().any(|quad| {
                        quad.corners.iter().all(|point| point[1] == layer as f32)
                            && quad
                                .corners
                                .iter()
                                .map(|p| p[0])
                                .fold(f32::INFINITY, f32::min)
                                < x
                            && x < quad
                                .corners
                                .iter()
                                .map(|p| p[0])
                                .fold(f32::NEG_INFINITY, f32::max)
                            && quad
                                .corners
                                .iter()
                                .map(|p| p[2])
                                .fold(f32::INFINITY, f32::min)
                                < z
                            && z < quad
                                .corners
                                .iter()
                                .map(|p| p[2])
                                .fold(f32::NEG_INFINITY, f32::max)
                    });
                    assert_eq!(
                        face_found,
                        layer == half || occupied[raw][corner] & (1 << quadrant) != 0,
                        "{}",
                        record.canonical_state
                    );
                }
            }
            let quad_count = quads.len();
            let target = BlockSelectionTarget {
                block: [0; 3],
                bounds: [[0.0; 3], [1.0; 3]],
                shape,
            };
            let mut frame = BlockSelectionFrame::default();
            frame.update(Some(&target), Vec3::splat(3.0), Vec3::NEG_Z, true);
            // Named native StairBlock::getOutline deliberately uses one full box.
            assert_eq!(frame.outline.len(), 12 * 6);
            frame.update(Some(&target), Vec3::splat(3.0), Vec3::NEG_Z, false);
            assert_eq!(frame.highlight.len(), quad_count * 6);
        }
    }
}

#[test]
fn every_partial_native_snow_highlight_top_survives_above_the_surface() {
    let (records, assets) = fixture();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:snow_layer")
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let layer = state["height"]["value"].as_u64().unwrap() + 1;
        if layer == u64::from(TOP_SNOW_LAYER_COUNT) {
            continue;
        }
        let height = layer as f32 / f32::from(TOP_SNOW_LAYER_COUNT);
        let shape = selected_shape(&assets, record, NetworkIdMode::Sequential);
        let target = BlockSelectionTarget {
            block: [0; 3],
            bounds: [[0.0; 3], [1.0, height, 1.0]],
            shape,
        };
        let mut frame = BlockSelectionFrame::default();
        frame.update(Some(&target), Vec3::new(0.5, 2.0, 0.5), Vec3::NEG_Y, false);
        let top = frame
            .highlight
            .iter()
            .filter(|vertex| vertex.position[1] > height)
            .count();
        assert_eq!(
            top, 6,
            "{} top must be pushed outside snow, never inside",
            record.canonical_state
        );
        frame.update(Some(&target), Vec3::new(0.5, 2.0, 0.5), Vec3::NEG_Y, true);
        assert_eq!(frame.outline.len(), 12 * 6);
    }
}

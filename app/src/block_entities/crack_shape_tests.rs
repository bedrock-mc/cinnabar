use super::*;

#[test]
fn bamboo_cracks_follow_columns_and_reuse_unchanged_geometry() {
    let data = crate::asset_startup::pinned_block_registry_bytes();
    let protocol = assets::registry_header_protocol(data).unwrap();
    let records: Vec<_> = assets::read_registry_for_protocol(data, protocol)
        .unwrap()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:bamboo"
            )
        })
        .enumerate()
        .map(|(id, mut record)| {
            record.sequential_id = id as u32;
            record
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("textures/blocks")).unwrap();
    std::fs::write(
        root.join("blocks.json"),
        r#"{"stone":{"textures":"test"},"bamboo":{"textures":"test"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"test":{"textures":"textures/blocks/test"}}}"#,
    )
    .unwrap();
    std::fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([127, 127, 127, 255]))
        .save(root.join("textures/blocks/test.png"))
        .unwrap();
    let lights = vec![assets::LightProperties::default(); records.len()];
    let compiled = pack_compiler::compile_pack(root, &records, &lights).unwrap();
    let assets = assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let bamboo = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:bamboo")
        .unwrap()
        .sequential_id;
    let mut shapes = HashMap::new();
    let shape = |cache: &mut HashMap<_, _>, position| {
        let CrackShape::Quads(quads) = crack_shape(
            cache,
            &assets,
            assets::NetworkIdMode::Sequential,
            Some(bamboo),
            position,
        ) else {
            panic!("bamboo model surface");
        };
        quads
    };
    let origin = shape(&mut shapes, [0, 2, 0]);
    let another_column = shape(&mut shapes, [1, 2, 0]);
    assert_ne!(
        origin, another_column,
        "runtime ID alone must not reuse another column's offsets"
    );
    let same_column = shape(&mut shapes, [0, 7, 0]);
    assert!(
        std::sync::Arc::ptr_eq(&origin, &same_column),
        "same column reuses geometry without rebuilding"
    );
    assert_eq!(shapes.len(), 2);
    let stone = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .unwrap()
        .sequential_id;
    for position in [[0; 3], [100, 20, -100]] {
        assert_eq!(
            crack_shape(
                &mut shapes,
                &assets,
                assets::NetworkIdMode::Sequential,
                Some(stone),
                position
            ),
            CrackShape::Cube
        );
    }
    assert_eq!(
        shapes.len(),
        3,
        "position-independent shapes share one entry"
    );
}

#[test]
fn custom_compound_cracks_follow_each_admitted_column_displacement() {
    let quad = assets::ModelQuad {
        positions: [[0, 0, 0], [256, 0, 0], [256, 256, 0], [0, 256, 0]],
        uvs: [[0, 0]; 4],
        material: 0,
        flags: 0,
    };
    let overlay = assets::BlockOverlay {
        visuals: vec![assets::BlockVisual {
            faces: [0; 6],
            flags: assets::BlockFlags::empty(),
            kind: assets::VisualKind::Model,
            support: assets::VisualSupport::VanillaFallback,
            contributor_role: assets::ContributorRole::Primary,
            model_template: 0,
            animation: assets::NO_ANIMATION,
            variant: 0,
        }],
        light_properties: vec![assets::LightProperties::default()],
        materials: vec![assets::Material {
            texture: assets::TextureRef::new(1, 0).unwrap(),
            animation: assets::NO_ANIMATION,
            ..assets::Material::unvaried()
        }],
        model_templates: vec![
            assets::ModelTemplate {
                quad_start: 0,
                quad_count: 1,
                flags: assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
            },
            assets::ModelTemplate {
                quad_start: 1,
                quad_count: 1,
                flags: 0,
            },
        ],
        model_quads: vec![quad; 2],
        model_random_offsets: vec![(1, block_transform::random_offset::BAMBOO)],
        hashes: vec![Some(0xdead_beef)],
        texture: Some(assets::TextureArray {
            layers: 1,
            mips: assets::build_texture_mip_chain(vec![255; 16 * 16 * 4].into(), 16).unwrap(),
        }),
        ..Default::default()
    };
    let assets = assets::RuntimeAssets::diagnostic()
        .with_block_overlay(1, &overlay)
        .unwrap();
    for (mode, id) in [
        (assets::NetworkIdMode::Sequential, 1),
        (assets::NetworkIdMode::Hashed, 0xdead_beef),
    ] {
        let mut cache = HashMap::new();
        let CrackShape::Quads(first) = crack_shape(&mut cache, &assets, mode, Some(id), [0, 2, 0])
        else {
            panic!("compound model");
        };
        let CrackShape::Quads(second) = crack_shape(&mut cache, &assets, mode, Some(id), [1, 2, 0])
        else {
            panic!("compound model");
        };
        assert_ne!(
            first[1].corners, second[1].corners,
            "different columns cannot reuse the first component displacement"
        );
        let CrackShape::Quads(same_column) =
            crack_shape(&mut cache, &assets, mode, Some(id), [1, 7, 0])
        else {
            panic!("compound model");
        };
        assert!(
            std::sync::Arc::ptr_eq(&second, &same_column),
            "height changes reuse the same column shape"
        );
        assert_eq!(
            cache.len(),
            1,
            "continuous column displacement keeps bounded cache entries per runtime transform"
        );
    }
}

use super::*;
use assets::*;

fn cube_source(flags: u32) -> CompiledAssets {
    let side = u32::from(BLOCK_ITEM_FACE_SIDE);
    CompiledAssets {
        visuals: vec![
            BlockVisual::diagnostic(BlockFlags::empty(), ContributorRole::Primary),
            BlockVisual {
                faces: [1; BlockFace::ALL.len()],
                flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
                kind: VisualKind::Cube,
                support: VisualSupport::Exact,
                contributor_role: ContributorRole::Primary,
                model_template: NO_MODEL_TEMPLATE,
                animation: NO_ANIMATION,
                variant: 0,
            },
        ]
        .into(),
        light_properties: vec![LightProperties::default(); 2].into(),
        hashed: Box::new([]),
        materials: vec![
            Material::unvaried(),
            Material {
                texture: TextureRef::new(0, 1).unwrap(),
                flags,
                ..Material::unvaried()
            },
        ]
        .into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 2,
            mips: std::iter::successors(Some(side), |size| (*size > 1).then_some(size / 2))
                .map(|size| {
                    let mut rgba8 = [40, 80, 120, 255].repeat((size * size * 2) as usize);
                    rgba8[(size * size * 4) as usize] = 91;
                    TextureMip {
                        size,
                        rgba8: rgba8.into(),
                    }
                })
                .collect::<Vec<_>>()
                .into(),
        })]
        .into(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    }
}

fn cube_world(flags: u32) -> RuntimeAssets {
    let source = cube_source(flags);
    RuntimeAssets::decode(&encode_blob(&source).unwrap()).unwrap()
}

#[test]
fn world_isotropy_preserves_short_model_icons() {
    let mut source = cube_source(MATERIAL_FLAG_ALPHA_CUTOUT);
    source.visuals[1].kind = VisualKind::Model;
    source.visuals[1].flags = BlockFlags::empty();
    source.visuals[1].model_template = 0;
    source.model_templates = vec![ModelTemplate {
        quad_start: 0,
        quad_count: 1,
        flags: 0,
    }]
    .into();
    source.model_quads = vec![ModelQuad {
        positions: [[0, 240, 0], [0, 240, 256], [256, 240, 256], [256, 240, 0]],
        uvs: [[0, 0], [0, 4096], [4096, 4096], [4096, 0]],
        material: 1,
        flags: BlockFace::Up.model_quad_face_id(),
    }]
    .into();
    let plain = RuntimeAssets::decode(&encode_blob(&source).unwrap()).unwrap();
    let expected = model::Model::read(&plain, BlockVisualId(1))
        .unwrap()
        .raster();
    source.materials[1].flags |= MATERIAL_FLAG_ISOTROPIC;
    let isotropic = RuntimeAssets::decode(&encode_blob(&source).unwrap()).unwrap();
    let actual = model::Model::read(&isotropic, BlockVisualId(1))
        .unwrap()
        .raster();
    assert_eq!(actual.rgba8, expected.rgba8);
}

#[test]
fn world_isotropy_preserves_opaque_cube_icons() {
    let plain = cube_world(0);
    let isotropic = cube_world(MATERIAL_FLAG_ISOTROPIC);
    let expected = cube::Cube::read(&plain, BlockVisualId(1)).unwrap();
    let actual = cube::Cube::read(&isotropic, BlockVisualId(1))
        .expect("world rotation preserves opaque icon admission");
    assert!(actual.same_source(&expected));
    assert_eq!(actual.raster().rgba8, expected.raster().rgba8);
    let unsupported = cube_world(MATERIAL_FLAG_ISOTROPIC | MATERIAL_FLAG_ROTATE_UV);
    assert!(cube::Cube::read(&unsupported, BlockVisualId(1)).is_err());
}

#[test]
fn world_isotropy_preserves_general_cube_icons() {
    for flags in [0, MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT] {
        let plain = cube_world(flags);
        let isotropic = cube_world(flags | MATERIAL_FLAG_ISOTROPIC);
        let expected = model::Model::read(&plain, BlockVisualId(1))
            .unwrap()
            .raster();
        let actual = model::Model::read(&isotropic, BlockVisualId(1))
            .expect("world rotation preserves general icon admission")
            .raster();
        assert_eq!(actual.rgba8, expected.rgba8);
    }
    let unsupported = cube_world(MATERIAL_FLAG_ISOTROPIC | MATERIAL_FLAG_GRASS_TINT);
    assert!(model::Model::read(&unsupported, BlockVisualId(1)).is_err());
}

#[test]
fn review_identical_thumbnails_keep_distinct_full_resolution_models() {
    let mut source = cube_source(0);
    source.visuals[1].kind = VisualKind::Model;
    source.visuals[1].flags = BlockFlags::empty();
    source.visuals[1].model_template = 0;
    let mut second = source.visuals[1];
    second.model_template = 1;
    source.visuals = vec![source.visuals[0], source.visuals[1], second].into();
    source.light_properties = vec![LightProperties::default(); 3].into();
    source.materials = vec![
        source.materials[0],
        source.materials[1],
        Material {
            texture: TextureRef::new(0, 2).unwrap(),
            ..source.materials[1]
        },
    ]
    .into();
    source.model_templates = (0..2)
        .map(|quad_start| ModelTemplate {
            quad_start,
            quad_count: 1,
            flags: 0,
        })
        .collect();
    source.model_quads = (1..=2)
        .map(|material| ModelQuad {
            positions: [[0, 128, 0], [0, 128, 256], [16, 128, 256], [16, 128, 0]],
            uvs: [[0, 0], [0, 4096], [4096, 4096], [4096, 0]],
            material,
            flags: BlockFace::Up.model_quad_face_id(),
        })
        .collect();
    source.texture_pages = vec![TexturePage::new(TextureArray {
        layers: 3,
        mips: std::iter::successors(Some(TILE_SIZE), |size| (*size > 1).then_some(size / 2))
            .map(|size| {
                let rgba8 = [40, 80, 120, 255].repeat((size * size * 3) as usize);
                TextureMip {
                    size,
                    rgba8: rgba8.into(),
                }
            })
            .collect(),
    })]
    .into();
    let side = TILE_SIZE as usize;
    for layer in 1..=2 {
        for pixel in 0..side * side {
            let start = (layer * side * side + pixel) * 4;
            source.texture_pages[0].texture.mips[0].rgba8[start..start + 4].copy_from_slice(&[
                (pixel % side * 16) as u8,
                (pixel / side * 16) as u8,
                120,
                255,
            ]);
        }
    }
    let world = RuntimeAssets::decode(&encode_blob(&source).unwrap()).unwrap();
    let raster = model::Model::read(&world, BlockVisualId(1))
        .unwrap()
        .raster();
    let shade = assets::gui_item::CUBE_FACES[0].3;
    let unseen = (0..side * side)
        .find(|&pixel| {
            let color = [
                ((pixel % side * 16) as f32 * shade).round() as u8,
                ((pixel / side * 16) as f32 * shade).round() as u8,
                (120.0 * shade).round() as u8,
                255,
            ];
            !raster.rgba8.as_chunks::<4>().0.contains(&color)
        })
        .expect("the thin thumbnail leaves some source texels unsampled");
    source.texture_pages[0].texture.mips[0].rgba8[(2 * side * side + unseen) * 4] = 255;
    let world = RuntimeAssets::decode(&encode_blob(&source).unwrap()).unwrap();
    assert!(
        raster
            == model::Model::read(&world, BlockVisualId(2))
                .unwrap()
                .raster(),
        "a change to an unsampled texel preserves the thumbnail"
    );
    let compiled = crate::compile_entity_pack(vec![
        ("entity/test.json".into(), br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","geometry":{"default":"geometry.test"},"render_controllers":["controller.render.test"]}}}"#.to_vec()),
        ("models/entity/test.geo.json".into(), br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test"},"bones":[{"name":"root"}]}]}"#.to_vec()),
        ("render_controllers/test.json".into(), br#"{"format_version":"1.8.0","render_controllers":{"controller.render.test":{"geometry":"Geometry.default"}}}"#.to_vec()),
    ]).unwrap().unwrap().assets;
    let root = tempfile::tempdir().unwrap();
    let mut sprites = vec![raster];
    let baked = bake::run(
        root.path(),
        Some(&world),
        None,
        &BTreeMap::new(),
        &BTreeMap::from([
            (1, Err(cube::Reject::Geometry)),
            (2, Err(cube::Reject::Geometry)),
        ]),
        &compiled,
        &mut sprites,
    )
    .unwrap();
    assert_ne!(baked.model_sprites[&(1, 0)], baked.model_sprites[&(2, 0)]);
    assert_ne!(
        baked.model_sprites[&(1, 0)],
        0,
        "a flat thumbnail must not acquire a model binding"
    );
    assert_eq!(baked.block_models.len(), 2);
}

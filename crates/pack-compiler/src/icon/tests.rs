use super::*;
use assets::*;

fn cube_world(flags: u32) -> RuntimeAssets {
    let side = u32::from(BLOCK_ITEM_FACE_SIDE);
    let source = CompiledAssets {
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
    };
    RuntimeAssets::decode(&encode_blob(&source).unwrap()).unwrap()
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

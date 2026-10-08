#[test]
fn positional_materials_keep_one_cell_per_quad_on_every_cube_face() {
    let base = RuntimeAssets::diagnostic();
    let texture = TextureRef::new(1, 0).unwrap();
    let mut page = base.texture_array().clone();
    page.layers = 2;
    for mip in &mut page.mips {
        mip.rgba8 = mip.rgba8.repeat(2).into();
    }
    let mut overlay = assets::BlockOverlay {
        visuals: vec![BlockVisual {
            faces: [0; 6],
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            support: assets::VisualSupport::VanillaFallback,
            contributor_role: assets::ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }],
        light_properties: vec![assets::LightProperties::OPAQUE_DARK],
        materials: vec![
            Material {
                texture,
                variation_start: 1,
                variation_count: 2,
                ..Material::unvaried()
            },
            Material {
                texture,
                variation_weight: 0.5_f32.to_bits(),
                ..Material::unvaried()
            },
            Material {
                texture: TextureRef::new(1, 1).unwrap(),
                variation_weight: 0.5_f32.to_bits(),
                ..Material::unvaried()
            },
        ],
        texture: Some(page),
        ..Default::default()
    };
    let sub = uniform(1);
    for positional in [true, false] {
        if !positional {
            overlay.materials[0].variation_start = 0;
            overlay.materials[0].variation_count = 0;
        }
        let assets = base.with_block_overlay(1, &overlay).unwrap();
        let mesh = mesh_sub_chunk(
            &classifier(),
            &assets,
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &sub,
        );
        assert_eq!(mesh.quad_count(), if positional { 6 * 16 * 16 } else { 6 });
        if positional {
            assert!(mesh.quads().iter().all(|q| q.width() == 1 && q.height() == 1));
        }
    }
}

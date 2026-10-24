use super::*;

/// Keeps two cube materials and a fixed table size while assigning one a valid choice range.
fn runtime_with_selector(selector: usize, count: u16) -> RuntimeAssets {
    let mut assets = RuntimeAssets::diagnostic();
    assets.materials = vec![Material::unvaried(); 6].into();
    assets.materials[selector].variation_start = if count == 0 { 0 } else { 3 };
    assets.materials[selector].variation_count = u32::from(count);
    for index in 0..count {
        assets.materials[3 + usize::from(index)].variation_weight =
            (1.0 / f32::from(count)).to_bits();
    }
    let mut cube = BlockVisual::diagnostic(BlockFlags::CUBE_GEOMETRY, ContributorRole::Primary);
    cube.kind = VisualKind::Cube;
    cube.support = VisualSupport::Exact;
    cube.faces = [1; 6];
    let mut second = cube;
    second.faces = [2; 6];
    assets.visuals = vec![cube, second].into();
    assets.light_properties = vec![LightProperties::OPAQUE_DARK; 2].into();
    assets
}

#[test]
fn swapping_positional_and_static_materials_invalidates_equal_sized_geometry() {
    let before = runtime_with_selector(1, 2);
    let after = runtime_with_selector(2, 2);
    assert_eq!(before.materials.len(), after.materials.len());
    assert!(
        before
            .materials
            .iter()
            .all(|material| material.flags & crate::MATERIAL_FLAG_ISOTROPIC == 0)
    );
    assert!(!before.has_same_geometry(&after));
    assert!(!after.has_same_geometry(&before));
}

#[test]
fn changes_within_one_variation_mode_keep_geometry_reusable() {
    let scalar = runtime_with_selector(1, 0);
    let single = runtime_with_selector(1, 1);
    assert!(scalar.has_same_geometry(&single));
    assert!(single.has_same_geometry(&scalar));

    let two_choices = runtime_with_selector(1, 2);
    let three_choices = runtime_with_selector(1, 3);
    assert!(!single.has_same_geometry(&two_choices));
    assert!(!two_choices.has_same_geometry(&single));
    assert!(two_choices.has_same_geometry(&three_choices));
    assert!(three_choices.has_same_geometry(&two_choices));

    let mut changed_weights = runtime_with_selector(1, 2);
    changed_weights.materials[3].variation_weight = 0.25_f32.to_bits();
    changed_weights.materials[4].variation_weight = 0.75_f32.to_bits();
    assert!(two_choices.has_same_geometry(&changed_weights));
}

#[test]
fn material_texels_copy_one_layer_and_reject_the_diagnostic_material() {
    let mut assets = RuntimeAssets::diagnostic();
    let layer = |shade: u8| [shade, 0, 0, 255].repeat(4);
    assets.texture_pages = vec![TexturePage::new(TextureArray {
        layers: 2,
        mips: vec![TextureMip {
            size: 2,
            rgba8: [layer(10), layer(20)].concat().into(),
        }]
        .into(),
    })]
    .into();
    assets.materials = vec![
        Material::unvaried(),
        Material {
            texture: TextureRef::new(0, 1).unwrap(),
            ..Material::unvaried()
        },
    ]
    .into();
    let (size, texels) = assets.material_texels(1).expect("second layer");
    assert_eq!(size, [2, 2]);
    assert_eq!(&*texels, layer(20).as_slice());
    assert!(assets.material_texels(DIAGNOSTIC_MATERIAL).is_none());
    assert!(assets.material_texels(2).is_none());
}

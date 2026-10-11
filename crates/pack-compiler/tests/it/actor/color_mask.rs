use super::*;

#[test]
fn a_custom_sheep_material_does_not_unlock_unverified_fractional_alpha() {
    let pack = pack(3, "sheep", false);
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.textures, 0);
    assert_eq!(compiled.report.bindings, 0);
}

#[test]
#[ignore = "requires the downloaded pinned vanilla pack; set CINNABAR_VANILLA_ACTOR_ROOT"]
fn pinned_sheep_rasters_and_all_geometry_bindings_retain_native_color_mask_alpha() {
    let root = std::env::var_os("CINNABAR_VANILLA_ACTOR_ROOT").unwrap();
    let root = Path::new(&root);
    let entities = compile_entity_assets(root, MANIFEST).unwrap();
    let entity_bytes = encode_entity_blob(&entities).unwrap();
    let compiled = compile_actor_assets(root, MANIFEST).unwrap();
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&entity_bytes).unwrap(),
    )
    .unwrap();
    let sheep_rigs: Vec<_> = entities
        .rig_bindings
        .iter()
        .enumerate()
        .filter(|(_, rig)| {
            entities.symbols[rig.entity_symbol as usize]
                .identifier
                .as_ref()
                == "minecraft:sheep"
        })
        .collect();
    assert!(!sheep_rigs.is_empty());
    for (rig_index, rig) in sheep_rigs {
        let count = catalog
            .bindings()
            .iter()
            .filter(|binding| binding.rig as usize == rig_index)
            .count();
        assert_eq!(count, usize::from(rig.geometry_count));
        assert!(
            !compiled
                .report
                .fallbacks
                .iter()
                .any(|entry| entry.rig as usize == rig_index)
        );
    }
    let mut masks = 0;
    for (index, texture) in catalog.textures().iter().enumerate() {
        let source = &entities.sources[texture.source as usize];
        if !assets::native_actor_texture_uses_color_mask(source) {
            assert!(!catalog.texture_uses_color_mask(index));
            continue;
        }
        masks += 1;
        assert!(catalog.texture_uses_color_mask(index));
        let raster = image::open(root.join(source.path.as_ref()))
            .unwrap()
            .into_rgba8()
            .into_raw();
        assert_eq!(
            texture.rgba8.as_ref(),
            raster,
            "mask alpha is not quantized"
        );
        assert!(
            raster
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| !matches!(pixel[3], 0 | 255))
        );
    }
    assert_eq!(
        masks, 2,
        "both adult and baby rasters are independently witnessed"
    );
}

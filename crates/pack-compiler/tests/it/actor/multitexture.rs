use super::*;

#[test]
fn a_custom_llama_material_does_not_unlock_unverified_fractional_alpha() {
    let pack = pack(3, "llama", false);
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.textures, 0);
    assert_eq!(compiled.report.bindings, 0);
}

#[test]
#[ignore = "requires the downloaded pinned vanilla pack; set CINNABAR_VANILLA_ACTOR_ROOT"]
fn pinned_llama_body_and_decor_rasters_retain_three_sampler_alpha() {
    let root = std::env::var_os("CINNABAR_VANILLA_ACTOR_ROOT").unwrap();
    let root = Path::new(&root);
    let entities = compile_entity_assets(root, MANIFEST).unwrap();
    let entity_bytes = encode_entity_blob(&entities).unwrap();
    let runtime_entities = assets::RuntimeEntityAssets::decode(&entity_bytes).unwrap();
    let compiled = compile_actor_assets(root, MANIFEST).unwrap();
    let catalog = RuntimeActorCatalog::decode(&compiled.bytes, &runtime_entities).unwrap();
    let used: std::collections::BTreeSet<_> = entities
        .render
        .candidates
        .iter()
        .map(|candidate| candidate.source as usize)
        .collect();
    let mut witnessed = 0;
    let mut fractional = 0;
    for (source_index, source) in entities.sources.iter().enumerate() {
        if !used.contains(&source_index) || !assets::native_actor_texture_uses_multitexture(source)
        {
            continue;
        }
        let (index, texture) = catalog
            .textures()
            .iter()
            .enumerate()
            .find(|(_, texture)| texture.source as usize == source_index)
            .expect("every native base, decoration and empty sampler must be admitted");
        assert!(catalog.texture_uses_multitexture(index));
        assert!(!catalog.texture_uses_color_mask(index));
        let raster = image::open(root.join(source.path.as_ref()))
            .unwrap()
            .into_rgba8()
            .into_raw();
        assert_eq!(
            texture.rgba8.as_ref(),
            raster,
            "native alpha is never quantized"
        );
        witnessed += 1;
        fractional += usize::from(
            raster
                .chunks_exact(4)
                .any(|pixel| !matches!(pixel[3], 0 | 255)),
        );
    }
    assert!(witnessed > 0 && fractional > 0);
    let mut rigs = 0;
    for (index, rig) in entities.rig_bindings.iter().enumerate() {
        if !assets::native_actor_uses_multitexture(&runtime_entities, index) {
            continue;
        }
        rigs += 1;
        assert_eq!(
            catalog
                .bindings()
                .iter()
                .filter(|binding| binding.rig as usize == index)
                .count(),
            usize::from(rig.geometry_count)
        );
        assert!(
            !compiled
                .report
                .fallbacks
                .iter()
                .any(|entry| entry.rig as usize == index)
        );
    }
    assert!(rigs > 0);
}

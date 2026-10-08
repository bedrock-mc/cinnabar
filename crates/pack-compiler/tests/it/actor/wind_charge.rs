use super::*;

#[test]
fn native_wind_charge_raster_and_both_projectile_bindings_survive_actor_compilation() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ACTOR_ROOT") else {
        eprintln!("missing fixture: CINNABAR_VANILLA_ACTOR_ROOT pinned vanilla pack");
        return;
    };
    let root = Path::new(&root);
    let entities = compile_entity_assets(root, MANIFEST).unwrap();
    let runtime_entities =
        assets::RuntimeEntityAssets::decode(&encode_entity_blob(&entities).unwrap()).unwrap();
    let compiled = compile_actor_assets(root, MANIFEST).unwrap();
    let runtime = RuntimeActorCatalog::decode(&compiled.bytes, &runtime_entities).unwrap();
    for identifier in [
        "minecraft:wind_charge_projectile",
        "minecraft:breeze_wind_charge_projectile",
    ] {
        let (rig_index, rig) = entities
            .rig_bindings
            .iter()
            .enumerate()
            .find(|(_, rig)| {
                entities.symbols[rig.entity_symbol as usize]
                    .identifier
                    .as_ref()
                    == identifier
            })
            .unwrap();
        let binding = runtime
            .bindings()
            .iter()
            .find(|binding| binding.rig as usize == rig_index)
            .unwrap();
        let layer = runtime_entities
            .render_data()
            .layers
            .iter()
            .find(|layer| layer.rig as usize == rig_index)
            .unwrap();
        let state = layer.material_state.unwrap();
        assert!(state.blend);
        assert!(!(state.alpha_test || state.cull || state.depth_write));
        assert!(layer.uv_anim.is_some());
        let texture = &runtime.textures()[binding.texture as usize];
        let source = &entities.sources[texture.source as usize];
        assert!(assets::native_actor_texture_preserves_fractional_alpha(
            source
        ));
        let raster = image::open(root.join(source.path.as_ref()))
            .unwrap()
            .into_rgba8()
            .into_raw();
        assert_eq!(texture.rgba8.as_ref(), raster);
        assert!(
            raster
                .chunks_exact(4)
                .any(|pixel| !matches!(pixel[3], 0 | 255))
        );
        assert!(
            !compiled
                .report
                .fallbacks
                .iter()
                .any(|fallback| fallback.rig as usize == rig_index)
        );
        assert!(rig.geometry_count > 0);
    }
}

#[test]
fn custom_wind_material_does_not_claim_the_pinned_fractional_alpha_contract() {
    let fixture = pack(127, "breeze_wind", false);
    let compiled = compile_actor_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.bindings, 0);
    assert_eq!(compiled.report.textures, 0);
}

use super::*;

#[test]
fn sulfur_cube_native_shell_pixels_survive_actor_compilation() {
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
    let rig = entities
        .rig_bindings
        .iter()
        .position(|rig| {
            entities.symbols[rig.entity_symbol as usize]
                .identifier
                .as_ref()
                == "minecraft:sulfur_cube"
        })
        .unwrap();
    assert!(
        runtime
            .bindings()
            .iter()
            .any(|binding| binding.rig as usize == rig)
    );
    assert!(runtime_entities.render_data().layers.iter().any(|layer| {
        layer.rig as usize == rig
            && layer.material_state.is_some_and(|state| {
                state.blend && state.cull && state.depth_write && !state.alpha_test
            })
    }));
    for variant in ["outer", "outer_small"] {
        let path = format!("textures/entity/sulfur_cube/sulfur_cube_{variant}.png");
        let texture = runtime
            .textures()
            .iter()
            .find(|texture| entities.sources[texture.source as usize].path.as_ref() == path)
            .unwrap_or_else(|| panic!("missing sulfur cube shell {path}"));
        let source = &entities.sources[texture.source as usize];
        assert!(assets::native_actor_texture_preserves_fractional_alpha(
            source
        ));
        let raster = image::open(root.join(&path))
            .unwrap()
            .into_rgba8()
            .into_raw();
        assert_eq!(texture.rgba8.as_ref(), raster);
        assert!(
            raster
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| !matches!(pixel[3], 0 | 255))
        );
    }
}

#[test]
fn custom_sulfur_material_does_not_admit_unverified_fractional_alpha() {
    let fixture = pack(127, "sulfur_cube_outer", false);
    let compiled = compile_actor_assets(fixture.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.bindings, 0);
    assert_eq!(compiled.report.textures, 0);
}

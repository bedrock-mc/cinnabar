use super::*;

/// Gives the synthetic pack the crystal's native material identity.
fn crystal_pack(alpha: u8) -> TempDir {
    let pack = pack(alpha, "ender_crystal", false);
    let path = pack.path().join("entity/example.entity.json");
    let mut entity: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    entity["minecraft:client_entity"]["description"]["identifier"] =
        serde_json::json!("minecraft:ender_crystal");
    fs::write(path, serde_json::to_vec(&entity).unwrap()).unwrap();
    pack
}

#[test]
fn crystal_alpha_test_keeps_the_actor_and_bakes_native_half_alpha_coverage() {
    for (alpha, coverage) in [(0, 0), (127, 0), (128, 255), (255, 255)] {
        let pack = crystal_pack(alpha);
        let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
        assert_eq!(
            compiled.report.bindings, 1,
            "{:?}",
            compiled.report.fallbacks
        );
        let entities =
            encode_entity_blob(&compile_entity_assets(pack.path(), MANIFEST).unwrap()).unwrap();
        let catalog = RuntimeActorCatalog::decode(
            &compiled.bytes,
            &assets::RuntimeEntityAssets::decode(&entities).unwrap(),
        )
        .unwrap();
        assert_eq!(catalog.textures()[0].rgba8[PROBE_ALPHA], coverage);
    }
}

#[test]
fn installed_crystal_art_compiles_with_its_nested_frames_base_and_animation() {
    let manifest: serde_json::Value = serde_json::from_slice(MANIFEST).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(manifest["cache_dir"].as_str().unwrap())
        .join("resource_pack");
    if !root.is_dir() {
        eprintln!(
            "skipping installed crystal fixture: {} is absent",
            root.display()
        );
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    for path in [
        "entity/ender_crystal.entity.json",
        "models/entity/ender_crystal.geo.json",
        "animations/ender_crystal.animation.json",
        "render_controllers/ender_crystal.render_controllers.json",
        "textures/entity/endercrystal/endercrystal.png",
    ] {
        write(scratch.path(), path, &fs::read(root.join(path)).unwrap());
    }
    let beam = format!("{}.png", assets::CRYSTAL_BEAM_TEXTURE);
    write(scratch.path(), &beam, &fs::read(root.join(&beam)).unwrap());
    for directory in ["animation_controllers", "attachables"] {
        fs::create_dir_all(scratch.path().join(directory)).unwrap();
    }
    let entities = compile_entity_assets(scratch.path(), MANIFEST).unwrap();
    let compiled = compile_actor_assets(scratch.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.report.bindings, 1,
        "{:?}",
        compiled.report.fallbacks
    );
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&encode_entity_blob(&entities).unwrap()).unwrap(),
    )
    .unwrap();
    let geometry = &entities.geometries[catalog.bindings()[0].geometry as usize];
    assert_eq!(geometry.bones.len(), 4);
    assert_eq!(geometry.bones[1].parent.as_deref(), Some("outerglass"));
    assert_eq!(geometry.bones[2].parent.as_deref(), Some("innerglass"));
    assert_eq!(geometry.bones[3].name.as_ref(), "base");
    assert!(!entities.animation_channels.is_empty());
    assert_eq!(entities.render.visibility.len(), 1);
    let original = image::open(root.join("textures/entity/endercrystal/endercrystal.png"))
        .unwrap()
        .into_rgba8();
    assert!(original.pixels().any(|pixel| pixel[3] == 127));
    for (original, actual) in original
        .pixels()
        .zip(catalog.textures()[0].rgba8.as_chunks::<4>().0.iter())
    {
        assert_eq!(&actual[..3], &original.0[..3]);
        assert_eq!(actual[3], if original[3] >= 128 { 255 } else { 0 });
    }
}

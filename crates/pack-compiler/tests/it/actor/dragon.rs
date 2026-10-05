use super::*;

#[test]
fn dissolve_depth_artwork_retains_the_authored_fractional_alpha_mask() {
    let pack = pack(128, "entity_dissolve_layer0.skinning", false);
    let entities = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        entities.render.layers[0].material,
        assets::EntityRenderMaterial::DissolveDepth
    );
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.report.bindings, 1,
        "the dissolve mask must have an artwork route: {:?}",
        compiled.report.fallbacks
    );
    let catalog =
        RuntimeActorCatalog::decode(&compiled.bytes, &encode_entity_blob(&entities).unwrap())
            .unwrap();
    let raster = image::open(pack.path().join("textures/entity/example.png"))
        .unwrap()
        .into_rgba8();
    assert_eq!(catalog.textures()[0].rgba8.as_ref(), raster.as_raw());
    assert!(!catalog.texture_uses_color_mask(0));
    assert!(!catalog.texture_uses_multitexture(0));
}

#[test]
fn flat_wing_box_uvs_admit_the_visible_faces_without_unused_sides() {
    let pack = pack(0, "ender_dragon", false);
    write(pack.path(), "models/entity/example.geo.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.example","texture_width":256,"texture_height":256},"bones":[{"name":"wing","cubes":[{"origin":[0,0,0],"size":[56,0,56],"uv":[-56,88]}]}]}]}"#);
    RgbaImage::from_pixel(256, 256, Rgba([17, 31, 47, 255]))
        .save(pack.path().join("textures/entity/example.png"))
        .unwrap();
    let entities = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.report.bindings, 1,
        "{:?}",
        compiled.report.fallbacks
    );
    let catalog =
        RuntimeActorCatalog::decode(&compiled.bytes, &encode_entity_blob(&entities).unwrap())
            .unwrap();
    let wing = &entities.geometries[catalog.bindings()[0].geometry as usize].bones[0].cubes[0];
    assert_eq!(wing.size.map(|value| value.get()), [56.0, 0.0, 56.0]);
    assert_eq!(
        (catalog.textures()[0].width, catalog.textures()[0].height),
        (256, 256)
    );
}

#[test]
fn installed_dragon_art_admits_the_body_and_flat_wings() {
    let manifest: serde_json::Value = serde_json::from_slice(MANIFEST).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(manifest["cache_dir"].as_str().unwrap())
        .join("resource_pack");
    if !root.is_dir() {
        eprintln!(
            "skipping installed dragon fixture: {} is absent",
            root.display()
        );
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    for path in [
        "entity/ender_dragon.entity.json",
        "models/entity/ender_dragon.geo.json",
        "animations/ender_dragon.animation.json",
        "render_controllers/ender_dragon.render_controllers.json",
        "textures/entity/dragon/dragon.tga",
        "textures/entity/dragon/dragon_exploding.png",
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
    let catalog =
        RuntimeActorCatalog::decode(&compiled.bytes, &encode_entity_blob(&entities).unwrap())
            .unwrap();
    let binding = &catalog.bindings()[0];
    assert_eq!(
        entities.symbols[binding.entity_symbol as usize]
            .identifier
            .as_ref(),
        "minecraft:ender_dragon"
    );
    for path in [
        "textures/entity/dragon/dragon.tga",
        "textures/entity/dragon/dragon_exploding.png",
    ] {
        let source = entities
            .sources
            .iter()
            .position(|source| source.path.as_ref() == path)
            .unwrap() as u32;
        let texture_index = catalog
            .texture_of_source(source)
            .unwrap_or_else(|| panic!("selected dragon pass has no artwork: {path}"));
        let texture = &catalog.textures()[texture_index as usize];
        let authored = image::open(root.join(path)).unwrap().into_rgba8();
        assert_eq!(texture.rgba8.as_ref(), authored.as_raw());
        assert!(!catalog.texture_uses_color_mask(texture_index as usize));
        assert!(!catalog.texture_uses_multitexture(texture_index as usize));
    }
    let geometry = &entities.geometries[binding.geometry as usize];
    assert_eq!(geometry.bones.len(), 37);
    assert!(
        geometry
            .bones
            .iter()
            .all(|bone| bone.name.as_ref() != "neck")
    );
    for (prefix, count) in [("neck", 5), ("tail", 12)] {
        for index in 1..=count {
            let name = format!("{prefix}{index}");
            let bone_index = geometry
                .bones
                .iter()
                .position(|bone| bone.name.as_ref() == name)
                .unwrap();
            assert_eq!(geometry.bones[bone_index].parent.as_deref(), Some("root"));
            assert_eq!(geometry.bones[bone_index].cubes.len(), 2);
            assert!(entities.animation_clips.iter().any(|clip| {
                clip.geometry == Some(binding.geometry)
                    && entities.animation_channels[clip.first_channel as usize
                        ..(clip.first_channel + clip.channel_count) as usize]
                        .iter()
                        .any(|channel| channel.bone as usize == bone_index)
            }));
        }
    }
    assert!(geometry.bones.iter().any(|bone| {
        bone.name.contains("wing") && bone.cubes.iter().any(|cube| cube.size[1].get() == 0.0)
    }));
    assert!(!entities.render.layers.is_empty());
    assert!(!entities.animation_channels.is_empty());
    assert!(
        entities.rig_bindings[binding.rig as usize]
            .pre_animation
            .is_some()
    );
}

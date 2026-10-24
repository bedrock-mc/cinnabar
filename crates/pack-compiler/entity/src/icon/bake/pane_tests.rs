use super::*;

#[test]
fn glass_panes_keep_their_carried_model_in_hand() {
    for (side, name) in [16, 32].into_iter().flat_map(|side| {
        ["minecraft:glass_pane", "minecraft:red_stained_glass_pane"].map(|name| (side, name))
    }) {
        let root = tempfile::tempdir().unwrap();
        write_fixture(root.path());
        let artwork = image::RgbaImage::from_fn(side, side, |x, y| {
            image::Rgba([x as u8, y as u8, 180, if x == y { 0 } else { 200 }])
        });
        artwork
            .save(root.path().join("textures/blocks/glass.png"))
            .unwrap();
        let protocol = assets::active_content_registry_protocol();
        let records =
            assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
                .unwrap()
                .into_vec()
                .into_iter()
                .filter(|record| {
                    record.name.as_ref() == "minecraft:air" || record.name.as_ref() == name
                })
                .collect::<Vec<_>>();
        let visual = records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id;
        let span = records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap();
        let compiled_world = pack_compiler::compile_pack_with_biomes(
            root.path(),
            &root.path().join("behavior"),
            &records,
            &[],
            &vec![assets::LightProperties::default(); span],
            protocol,
        )
        .unwrap();
        let world = RuntimeAssets::decode(&assets::encode_blob(&compiled_world).unwrap()).unwrap();
        let compiled = crate::compile_entity_assets(
            root.path(),
            include_bytes!("../../../../../../assets/vanilla-source.json"),
        )
        .unwrap();
        let blocks = IconBlocks::read(root.path()).unwrap();
        let mut sprites = Vec::new();
        let baked = run(
            root.path(),
            Some(&world),
            Some(&blocks),
            &BTreeMap::new(),
            &BTreeMap::from([(visual, Err(cube::Reject::Geometry))]),
            &compiled,
            &mut sprites,
        )
        .unwrap();
        let gui = baked.model_sprites[&(visual, 0)];
        let model = baked
            .block_models
            .iter()
            .find(|model| model.sprite == gui)
            .unwrap();
        assert_eq!(model.visual.0, visual);
        assert_eq!(
            model.held_sprite, None,
            "glass panes use their carried plate in both hands"
        );
    }
}

/// Writes original item, biome and texture metadata for an isolated pane compilation.
fn write_fixture(root: &Path) {
    for (path, text) in [
        (
            "entity/item.entity.json",
            r#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#,
        ),
        (
            "models/entity/item.geo.json",
            r#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#,
        ),
        (
            "animations/empty.json",
            r#"{"format_version":"1.8.0","animations":{}}"#,
        ),
        (
            "animation_controllers/empty.json",
            r#"{"format_version":"1.10.0","animation_controllers":{}}"#,
        ),
        (
            "render_controllers/item.json",
            r#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"}}}"#,
        ),
        ("textures/item_texture.json", r#"{"texture_data":{}}"#),
        ("textures/flipbook_textures.json", "[]"),
        (
            "blocks.json",
            r#"{"glass_pane":{"textures":"glass","sound":"glass"},"red_stained_glass_pane":{"textures":"glass","sound":"glass"}}"#,
        ),
        (
            "textures/terrain_texture.json",
            r#"{"texture_data":{"glass":{"textures":"textures/blocks/glass"},"glass_pane_top":{"textures":"textures/blocks/glass"}}}"#,
        ),
        (
            "biomes/plain.client_biome.json",
            r#"{"minecraft:client_biome":{"description":{"identifier":"plain"},"components":{}}}"#,
        ),
        (
            "behavior/biomes/plain.biome.json",
            r#"{"minecraft:biome":{"description":{"identifier":"plain"},"components":{"minecraft:climate":{"temperature":0.8,"downfall":0.4}}}}"#,
        ),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::create_dir_all(root.join("textures/blocks")).unwrap();
    std::fs::create_dir_all(root.join("textures/entity")).unwrap();
    std::fs::create_dir_all(root.join("textures/colormap")).unwrap();
    for map in assets::TintMapId::ALL {
        image::RgbaImage::from_pixel(
            assets::TINT_MAP_SIZE,
            assets::TINT_MAP_SIZE,
            image::Rgba([255; 4]),
        )
        .save(root.join(format!("textures/colormap/{}.png", map.source_name())))
        .unwrap();
    }
}

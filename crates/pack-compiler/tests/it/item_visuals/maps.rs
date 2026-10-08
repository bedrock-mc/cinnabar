use super::*;

/// Build original map rasters with distinguishable metadata-selected pixels.
fn map_pack() -> TempDir {
    let pack = item_pack(false);
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{"texture_data":{"map_filled":{"textures":["textures/items/ordinary","textures/items/ordinary","textures/items/ordinary","textures/items/explorer"]}}}"#,
    );
    for (name, color) in [
        ("ordinary", [21, 53, 89, 255]),
        ("explorer", [233, 119, 41, 255]),
    ] {
        image::save_buffer(
            pack.path().join(format!("textures/items/{name}.png")),
            &color,
            1,
            1,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    pack
}

#[test]
fn filled_map_inventory_lookup_uses_the_pack_atlas_and_stack_metadata() {
    let pack = map_pack();
    let compiled = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let icons = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    for metadata in 0..3 {
        assert_eq!(
            icons
                .lookup("minecraft:filled_map", metadata)
                .expect("ordinary map icon")
                .rgba8
                .as_ref(),
            &[21, 53, 89, 255]
        );
    }
    assert_eq!(
        icons
            .lookup("minecraft:filled_map", 3)
            .expect("explorer map icon")
            .rgba8
            .as_ref(),
        &[233, 119, 41, 255]
    );
}

#[test]
fn absent_map_atlas_or_raster_does_not_invent_map_pixels() {
    let pack = map_pack();
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{"texture_data":{"ordinary":{"textures":"textures/items/ordinary"}}}"#,
    );
    let compiled = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let icons = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    assert!(icons.lookup("minecraft:filled_map", 0).is_none());

    let pack = map_pack();
    fs::remove_file(pack.path().join("textures/items/explorer.png")).unwrap();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(matches!(
        visual(&compiled.item_visuals, "minecraft:filled_map", 3).route,
        ItemVisualDefinitionRoute::Missing
    ));
}

use super::*;

#[test]
fn redstone_dust_keeps_its_item_sprite_instead_of_the_placed_wire() {
    let pack = item_pack(false);
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{"texture_data":{"redstone_dust":{"textures":"textures/items/powder_fixture"}}}"#,
    );
    let pixels = [
        211, 38, 57, 255, 103, 17, 26, 255, 176, 24, 49, 255, 0, 0, 0, 0,
    ];
    image::save_buffer(
        pack.path().join("textures/items/powder_fixture.png"),
        &pixels,
        2,
        2,
        image::ColorType::Rgba8,
    )
    .unwrap();
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let definition = visual(&entity.item_visuals, "minecraft:redstone", 0);
    let ItemVisualDefinitionRoute::Sprite { texture } = definition.route else {
        panic!(
            "redstone dust must use its item sprite, got {:?}",
            definition.route
        );
    };
    assert_eq!(
        entity.sources[texture.source as usize].path.as_ref(),
        "textures/items/powder_fixture.png"
    );
    assert_eq!(texture.variant, 0);
    assert!(matches!(
        visual(&entity.item_visuals, "minecraft:redstone_block", 0).route,
        ItemVisualDefinitionRoute::BlockItem { .. }
    ));

    let decoded =
        assets::RuntimeEntityAssets::decode(&encode_entity_blob(&entity).unwrap()).unwrap();
    assert_eq!(
        visual(decoded.item_visuals(), "minecraft:redstone", 0).route,
        definition.route
    );
    let icons = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let catalog = RuntimeIconCatalog::decode(&icons.bytes).unwrap();
    let sprite = catalog
        .lookup("minecraft:redstone", 0)
        .expect("redstone icon");
    assert_eq!([sprite.width, sprite.height], [2, 2]);
    assert_eq!(sprite.rgba8.as_ref(), pixels);
}

#[test]
fn absent_redstone_atlas_binding_keeps_the_unresolved_block_fallback() {
    let compiled = compile_entity_assets(item_pack(false).path(), MANIFEST).unwrap();
    assert!(matches!(
        visual(&compiled.item_visuals, "minecraft:redstone", 0).route,
        ItemVisualDefinitionRoute::BlockItem { .. }
    ));
}

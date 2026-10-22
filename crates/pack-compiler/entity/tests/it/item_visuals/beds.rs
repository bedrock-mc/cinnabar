use super::*;

fn bed_pack() -> TempDir {
    let pack = item_pack(false);
    let textures: Vec<_> = (0..=15)
        .map(|dye| format!("textures/items/bed_{dye}"))
        .collect();
    write(
        pack.path(),
        "textures/item_texture.json",
        &serde_json::to_vec(&serde_json::json!({
            "texture_data": {"bed": {"textures": textures}}
        }))
        .unwrap(),
    );
    for dye in 0..=15 {
        image::save_buffer(
            pack.path().join(format!("textures/items/bed_{dye}.png")),
            &[dye, 80, 160, 255],
            1,
            1,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    pack
}

#[test]
fn every_bed_dye_uses_its_item_atlas_sprite_instead_of_the_shared_block() {
    let pack = bed_pack();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    for dye in 0..=15 {
        let definition = visual(&compiled.item_visuals, "minecraft:bed", dye);
        let ItemVisualDefinitionRoute::Sprite { texture } = definition.route else {
            panic!(
                "bed dye {dye} must keep its atlas sprite: {:?}",
                definition.route
            )
        };
        assert_eq!(texture.variant, dye);
        assert_eq!(
            compiled.sources[texture.source as usize].path.as_ref(),
            format!("textures/items/bed_{dye}.png")
        );
    }
    encode_entity_blob(&compiled).unwrap();
}

#[test]
fn bed_icons_keep_the_stack_dye_in_the_runtime_catalog() {
    let pack = bed_pack();
    let icons = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let catalog = RuntimeIconCatalog::decode(&icons.bytes).unwrap();
    let indices: std::collections::BTreeSet<_> = (0..=15)
        .map(|dye| {
            let sprite = catalog.lookup("minecraft:bed", dye).unwrap();
            assert_eq!(sprite.rgba8.as_ref(), &[dye as u8, 80, 160, 255]);
            catalog.lookup_index("minecraft:bed", dye).unwrap()
        })
        .collect();
    assert_eq!(indices.len(), 16);
}

#[test]
fn straw_bed_keeps_its_atlas_sprite_in_entity_and_icon_carriers() {
    let pack = item_pack(false);
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([190, 140, 40, 255]));
    image
        .save(pack.path().join("textures/items/straw_bed.png"))
        .unwrap();
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{"texture_data":{"straw_bed":{"textures":"textures/items/straw_bed"}}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let definition = visual(&compiled.item_visuals, "minecraft:straw_bed", 0);
    let ItemVisualDefinitionRoute::Sprite { texture } = definition.route else {
        panic!(
            "straw bed must keep its atlas sprite: {:?}",
            definition.route
        )
    };
    assert_eq!(
        compiled.sources[texture.source as usize].path.as_ref(),
        "textures/items/straw_bed.png"
    );
    let decoded =
        assets::RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap();
    assert!(matches!(
        visual(decoded.item_visuals(), "minecraft:straw_bed", 0).route,
        ItemVisualDefinitionRoute::Sprite { .. }
    ));
    let icons = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let catalog = RuntimeIconCatalog::decode(&icons.bytes).unwrap();
    let sprite = catalog.lookup("minecraft:straw_bed", 0).unwrap();
    assert_eq!(
        [u32::from(sprite.width), u32::from(sprite.height)],
        [image.width(), image.height()]
    );
    assert_eq!(sprite.rgba8.as_ref(), image.as_raw());
}

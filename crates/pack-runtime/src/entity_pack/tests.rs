use super::entity_identifier;

#[test]
fn identifier_is_read_from_the_client_entity_description() {
    let json = br#"{"minecraft:client_entity":{"description":{"identifier":"a:b"}}}"#;
    assert_eq!(entity_identifier(json).as_deref(), Some("a:b"));
    assert_eq!(entity_identifier(b"{}"), None);
}

#[test]
fn texture_only_glint_pack_publishes_its_equipment_raster() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 1, image::Rgba([30, 90, 150, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let path = format!("{}.png", assets::entity::ITEM_GLINT_TEXTURE_IDENTIFIER);
    let view = resource_pack::LayeredPackView::new(crate::test_support::stack(&[(
        path.as_str(),
        png.get_ref(),
    )]));
    let pack = super::compile_encoded(&view, None, None, None, false, &|| false)
        .unwrap()
        .0
        .unwrap();
    let catalog = pack.equipment.as_ref().unwrap();
    assert!(catalog.bindings().is_empty());
    let texture = catalog
        .textures()
        .iter()
        .find(|texture| {
            texture.identifier.as_ref() == assets::entity::ITEM_GLINT_TEXTURE_IDENTIFIER
        })
        .unwrap();
    assert_eq!([texture.width, texture.height], [2, 1]);
    assert_eq!(
        texture.rgba8.as_ref(),
        &[30, 90, 150, 255, 30, 90, 150, 255]
    );
}

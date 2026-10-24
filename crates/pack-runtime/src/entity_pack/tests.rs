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

#[test]
fn texture_only_painting_pack_reaches_runtime_bindings_and_reload_tracking() {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(256, 256, image::Rgba([30, 90, 150, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let path = format!("{}.png", assets::painting::PAINTING_ATLAS);
    let view = resource_pack::LayeredPackView::tracked(crate::test_support::stack(&[(
        path.as_str(),
        png.get_ref(),
    )]));
    let pack = super::compile_encoded(&view, None, None, None, false, &|| false)
        .unwrap()
        .0
        .expect("texture-only painting pack must compile through runtime collection");
    let motive = assets::painting::PAINTING_MOTIVES[0];
    for border in [false, true] {
        let identifier = motive.part_identifier(border);
        let symbol = pack
            .assets
            .symbols()
            .iter()
            .position(|symbol| symbol.identifier.as_ref() == identifier)
            .unwrap();
        let binding = pack
            .assets
            .rig_bindings()
            .iter()
            .find(|binding| binding.entity_symbol as usize == symbol)
            .unwrap();
        let raster = pack
            .bindings
            .iter()
            .find(|raster| raster.geometry_candidate == binding.first_geometry)
            .unwrap();
        assert_eq!(
            pack.textures[raster.texture as usize].rgba8[0..4],
            [30, 90, 150, 255]
        );
    }
    let dependencies = view.dependencies().unwrap().snapshot();
    for expected in [
        path,
        "textures/painting/meditative.png".into(),
        "textures/painting/meditative.tga".into(),
    ] {
        assert!(dependencies.iter().any(|dependency| matches!(dependency, resource_pack::PackDependency::File { path, .. } if path == &expected)), "changes to present and missing painting rasters invalidate runtime compilation");
    }
}

#[test]
fn chalkboard_geometry_only_pack_publishes_and_tracks_its_current_runtime_catalog() {
    use assets::block_entity_geometry::chalkboard::ChalkboardSize;
    let geometry = serde_json::json!({"format_version":"1.8.0", ChalkboardSize::Large.geometry(): {"texturewidth":32,"textureheight":32,"bones":[{"name":"body","cubes":[{"origin":[2,7,1],"size":[3,5,2],"uv":[0,0]}]}]}});
    let bytes = serde_json::to_vec(&geometry).unwrap();
    let view = resource_pack::LayeredPackView::tracked(crate::test_support::stack(&[(
        "models/mobs.json",
        &bytes,
    )]));
    let pack = super::compile_encoded(&view, None, None, None, false, &|| false)
        .unwrap()
        .0
        .expect("a supplied chalkboard does not need a client-entity definition");
    assert!(
        pack.assets
            .geometries()
            .iter()
            .any(|geometry| geometry.identifier.as_ref() == ChalkboardSize::Large.geometry())
    );
    let dependencies = view.dependencies().unwrap().snapshot();
    assert!(dependencies.iter().any(|input|matches!(input,resource_pack::PackDependency::File{path,..} if path=="models/mobs.json")));
    let empty = resource_pack::LayeredPackView::new(crate::test_support::stack(&[]));
    assert!(
        super::compile_encoded(&empty, None, None, None, false, &|| false)
            .unwrap()
            .0
            .is_none()
    );
}

use assets::RuntimeIconCatalog;
use pack_entity_compiler::compile_icon_assets;
use std::{fs, path::Path};

const MANIFEST: &[u8] = include_bytes!("../../../../../assets/vanilla-source.json");

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn pack() -> tempfile::TempDir {
    let pack = tempfile::tempdir().unwrap();
    for (path,bytes) in [
        ("entity/item.json",&br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#[..]),
        ("models/entity/item.json",&br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#[..]),
        ("models/entity/fixture.json",&br#"{"format_version":"1.16.0","minecraft:geometry":[{"description":{"identifier":"geometry.fixture","texture_width":32,"texture_height":32},"bones":[{"name":"shield","pivot":[2,14,1],"cubes":[{"origin":[-4,16,-2],"size":[10,18,2],"uv":[0,0]}]}]}]}"#[..]),
        ("attachables/fixture.json",&br#"{"format_version":"1.10.0","minecraft:attachable":{"description":{"identifier":"minecraft:shield","materials":{"default":"entity_alphatest"},"textures":{"default":"textures/entity/fixture"},"geometry":{"default":"geometry.fixture"},"render_controllers":["controller.render.fixture"]}}}"#[..]),
        ("animations/empty.json",&br#"{"format_version":"1.8.0","animations":{}}"#[..]),
        ("animation_controllers/empty.json",&br#"{"format_version":"1.10.0","animation_controllers":{}}"#[..]),
        ("render_controllers/item.json",&br#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"},"controller.render.fixture":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#[..]),
        ("textures/entity/item.png",&b"unused-entity-raster"[..]),
        // A different legacy atlas image must never override the vanilla shield model.
        ("textures/item_texture.json",&br#"{"resource_pack_name":"synthetic","texture_name":"atlas.items","texture_data":{"shield":{"textures":"textures/items/flat"},"bundle_blue":{"textures":"textures/items/flat"}}}"#[..]),
    ] { write(pack.path(),path,bytes); }
    fs::create_dir_all(pack.path().join("textures/items")).unwrap();
    image::save_buffer(
        pack.path().join("textures/entity/fixture.png"),
        &[190, 70, 30, 255].repeat(32 * 32),
        32,
        32,
        image::ColorType::Rgba8,
    )
    .unwrap();
    image::save_buffer(
        pack.path().join("textures/items/flat.png"),
        &[0, 200, 80, 255].repeat(16 * 16),
        16,
        16,
        image::ColorType::Rgba8,
    )
    .unwrap();
    pack
}

#[test]
fn shield_gui_route_uses_bound_geometry_and_texture_without_changing_ordinary_sprites() {
    let pack = pack();
    let compiled = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.model_item_visuals, 1);
    let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    let shield = catalog.lookup("minecraft:shield", 0).unwrap();
    assert!(shield.rgba8.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
    assert!(
        shield
            .rgba8
            .as_chunks::<4>()
            .0
            .contains(&[190, 70, 30, 255])
    );
    assert!(!shield.rgba8.as_chunks::<4>().0.contains(&[0, 200, 80, 255]));
    assert_eq!(
        &*catalog.lookup("minecraft:bundle_blue", 0).unwrap().rgba8,
        &[0, 200, 80, 255].repeat(16 * 16)
    );
    assert_eq!(
        compile_icon_assets(pack.path(), MANIFEST).unwrap().bytes,
        compiled.bytes
    );
}

#[test]
fn absent_shield_model_does_not_fall_back_to_a_flat_model_texture() {
    let pack = pack();
    fs::remove_file(pack.path().join("attachables/fixture.json")).unwrap();
    let compiled = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.model_item_visuals, 0);
    assert!(
        RuntimeIconCatalog::decode(&compiled.bytes)
            .unwrap()
            .lookup("minecraft:shield", 0)
            .is_none()
    );
}

#[test]
fn dial_strips_bake_every_frame_in_order_under_their_frames_identifier() {
    let pack = pack();
    // Three 16-pixel frames, each one flat colour.
    let strip: Vec<u8> = (0..3u8)
        .flat_map(|frame| [frame * 60, 10, 10, 255].repeat(16 * 16))
        .collect();
    image::save_buffer(
        pack.path().join("textures/items/compass_atlas.png"),
        &strip,
        16,
        48,
        image::ColorType::Rgba8,
    )
    .unwrap();
    let catalog =
        RuntimeIconCatalog::decode(&compile_icon_assets(pack.path(), MANIFEST).unwrap().bytes)
            .unwrap();
    let strip = assets::icon::dial::dial_strip("minecraft:compass").unwrap();
    assert_eq!(catalog.frame_count(strip.frames_identifier), 3);
    for frame in 0..3u8 {
        assert_eq!(
            &*catalog
                .lookup(strip.frames_identifier, u32::from(frame))
                .unwrap()
                .rgba8,
            &[frame * 60, 10, 10, 255].repeat(16 * 16)
        );
    }
    assert_eq!(
        catalog.frame_count("minecraft:watch_atlas"),
        0,
        "absent strips bake nothing"
    );
}

#[test]
fn modeled_items_retain_their_separate_raw_atlas_source() {
    let pack = pack();
    let path = pack.path().join("textures/item_texture.json");
    let mut atlas: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    atlas["texture_data"]
        .as_object_mut()
        .unwrap()
        .remove("bundle_blue");
    fs::write(path, serde_json::to_vec(&atlas).unwrap()).unwrap();
    let catalog =
        RuntimeIconCatalog::decode(&compile_icon_assets(pack.path(), MANIFEST).unwrap().bytes)
            .unwrap();
    let raw = catalog
        .sources()
        .unwrap()
        .iter()
        .find(|source| source.path.as_ref() == "textures/items/flat.png")
        .map(|source| &catalog.sprites()[source.sprite as usize])
        .expect("modeled item's raw source");
    assert_eq!(raw.rgba8.as_ref(), &[0, 200, 80, 255].repeat(16 * 16));
    assert_ne!(
        raw.rgba8,
        catalog.lookup("minecraft:shield", 0).unwrap().rgba8
    );
}

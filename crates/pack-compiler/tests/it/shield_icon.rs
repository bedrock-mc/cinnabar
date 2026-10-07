use assets::RuntimeIconCatalog;
use pack_compiler::compile_icon_assets;
use std::{fs, path::Path};

const MANIFEST: &[u8] = include_bytes!("../../../../assets/vanilla-source.json");

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
    assert!(shield.rgba8.chunks_exact(4).any(|p| p[3] == 0));
    assert!(
        shield
            .rgba8
            .chunks_exact(4)
            .any(|p| p == [190, 70, 30, 255])
    );
    assert!(!shield.rgba8.chunks_exact(4).any(|p| p == [0, 200, 80, 255]));
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

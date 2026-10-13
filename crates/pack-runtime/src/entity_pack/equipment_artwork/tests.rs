use super::*;
use std::sync::Arc;

const MAIN: &str = "textures/items/fixture_armor";
const EXTRA: &str = "textures/items/fixture_artwork";

/// Encodes a tiny original raster in the requested server-pack format.
fn raster(color: [u8; 4], format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(4, 2, image::Rgba(color))
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

/// Combines an authored armour binding with a startup-path raster no binding references.
fn fixture() -> (LayeredPackView, RuntimeEquipmentCatalog) {
    let attachable = serde_json::json!({
        "format_version": "1.10.0", "minecraft:attachable": {"description": {
            "identifier": "fixture:chestplate", "materials": {"default": "armor"},
            "geometry": {"default": "geometry.fixture.armor"},
            "textures": {"default": MAIN}, "render_controllers": ["controller.render.fixture"]
        }}
    })
    .to_string();
    let geometry = serde_json::json!({
        "format_version": "1.12.0", "minecraft:geometry": [{
            "description": {"identifier": "geometry.fixture.armor", "texture_width": 4, "texture_height": 2},
            "bones": [{"name": "body", "cubes": [{"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}]}]
        }]
    }).to_string();
    let controller = serde_json::json!({
        "format_version": "1.8.0", "render_controllers": {"controller.render.fixture": {
            "geometry": "Geometry.default", "materials": [{"*": "Material.default"}], "textures": ["Texture.default"]
        }}
    }).to_string();
    let main = raster([40, 50, 60, 255], image::ImageFormat::Png);
    let extra = raster([70, 80, 90, 255], image::ImageFormat::Tga);
    let view = LayeredPackView::tracked(crate::test_support::stack(&[
        ("attachables/fixture.json", attachable.as_bytes()),
        ("models/entity/fixture.json", geometry.as_bytes()),
        ("render_controllers/fixture.json", controller.as_bytes()),
        (&format!("{MAIN}.png"), &main),
        (&format!("{EXTRA}.tga"), &extra),
    ]));
    let base = RuntimeEquipmentCatalog::from_parts(
        [1; 32],
        Vec::new(),
        [MAIN, EXTRA]
            .into_iter()
            .map(|identifier| EquipmentTexture {
                identifier: identifier.into(),
                width: 4,
                height: 2,
                rgba8: Arc::from([10, 20, 30, 255].repeat(8)),
            })
            .collect(),
    )
    .unwrap();
    (view, base)
}

#[test]
fn mixed_equipment_artwork_reuses_compiler_pixels_and_decodes_only_missing_paths() {
    let (view, base) = fixture();
    let mut files = super::super::collect_files(&view, None, None);
    let sources = collect(&view, Some(&base), &mut files);
    let mut compiled = pack_compiler::compile_actor_pack(files).unwrap().unwrap();
    let compiler_pixels = Arc::clone(
        &compiled
            .equipment_textures
            .iter()
            .find(|texture| texture.identifier.as_ref() == MAIN)
            .unwrap()
            .rgba8,
    );
    assert!(
        compiled
            .equipment_textures
            .iter()
            .all(|texture| texture.identifier.as_ref() != EXTRA)
    );
    merge(&view, &mut compiled.equipment_textures, sources);
    let retained = compiled
        .equipment_textures
        .iter()
        .find(|texture| texture.identifier.as_ref() == MAIN)
        .unwrap();
    assert!(
        Arc::ptr_eq(&retained.rgba8, &compiler_pixels),
        "the compiler's overlapping raster stays allocated once"
    );
    assert_eq!(&retained.rgba8[..4], &[40, 50, 60, 255]);
    let extra = compiled
        .equipment_textures
        .iter()
        .find(|texture| texture.identifier.as_ref() == EXTRA)
        .unwrap();
    assert_eq!((extra.width, extra.height), (4, 2));
    assert_eq!(&extra.rgba8[..4], &[70, 80, 90, 255]);
    assert_eq!(compiled.equipment_textures.len(), 2);
}

#[test]
fn invalid_artwork_only_input_does_not_publish_an_empty_layer() {
    let (_, base) = fixture();
    let view = LayeredPackView::new(crate::test_support::stack(&[(
        &format!("{MAIN}.png"),
        b"invalid raster",
    )]));
    let compiled =
        super::super::compile_encoded(&view, None, None, Some(&base), false, &|| false).unwrap();
    assert!(compiled.0.is_none());
}

//! A secondary-only pack retains the startup equipment draw and its default texture.

use super::*;

const ITEM: &str = "minecraft:iron_helmet";
const DEFAULT_PATH: &str = "textures/fixture_armor.png";
const BASE_PIXEL: [u8; 4] = [22, 33, 44, 255];

/// Encodes a small original raster used by the base and secondary equipment sources.
fn png(pixel: [u8; 4]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba(pixel))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// Resolves geometry and equipment independently of installed carriers or raw base packs.
fn compile(
    files: Vec<(Box<str>, Vec<u8>)>,
) -> (Arc<RuntimeEntityAssets>, Arc<RuntimeEquipmentCatalog>) {
    let compiled = pack_entity_compiler::compile_actor_pack(files)
        .unwrap()
        .unwrap();
    (
        Arc::new(RuntimeEntityAssets::from_compiled(compiled.entities).unwrap()),
        Arc::new(
            RuntimeEquipmentCatalog::from_parts(
                compiled.identity,
                compiled.equipment_bindings,
                compiled.equipment_textures,
            )
            .unwrap(),
        ),
    )
}

/// Samples the selected equipment texel without assuming atlas placement.
fn pixel(pages: &ActorArtworkPages, layer: &EquipmentPresentation) -> [u8; 4] {
    let page = &pages.pages()[usize::from(layer.location.page() - 1)];
    let (width, height) = page.dimensions();
    let offset = layer.location.layer() as usize * usize::from(width) * usize::from(height) * 4;
    page.pixels()[offset..offset + 4].try_into().unwrap()
}

#[test]
fn secondary_only_pack_keeps_startup_armour_visible() {
    let glint_path = format!("{}.png", assets::ACTOR_GLINT_TEXTURE_IDENTIFIER);
    let files: Vec<(Box<str>, Vec<u8>)> = vec![
        (
            "attachables/fixture.json".into(),
            serde_json::json!({
                "format_version": "1.10.0", "minecraft:attachable": {"description": {
                    "identifier": ITEM, "materials": {"default": "armor"},
                    "textures": {"default": "textures/fixture_armor",
                        "enchanted": assets::ACTOR_GLINT_TEXTURE_IDENTIFIER},
                    "geometry": {"default": "geometry.fixture.helmet"},
                    "render_controllers": ["controller.render.fixture"]
                }}
            })
            .to_string()
            .into_bytes(),
        ),
        (
            "models/entity/fixture.json".into(),
            serde_json::json!({
                "format_version": "1.12.0", "minecraft:geometry": [{
                    "description": {"identifier": "geometry.fixture.helmet",
                        "texture_width": 16, "texture_height": 16},
                    "bones": [{"name": "head", "pivot": [0, 0, 0],
                        "cubes": [{"origin": [0, 0, 0], "size": [1, 1, 1], "uv": [0, 0]}]}]
                }]
            })
            .to_string()
            .into_bytes(),
        ),
        (DEFAULT_PATH.into(), png(BASE_PIXEL)),
        (glint_path.into_boxed_str(), png([77, 88, 99, 255])),
    ];
    let (assets, catalog) = compile(files.clone());
    let (fixture, body, item) = block_fixture();
    let (mut runtime, base_pages, _) = EquipmentRuntime::build(
        assets,
        Some(catalog),
        fixture.icons,
        None,
        None,
        ActorArtworkPages::default(),
    );
    runtime.register_skin_rig(body.input.rig, vec!["head".into()]);
    crate::presentation::equipment::tests::declare_wearable(&mut runtime, ITEM, "slot.armor.head");
    let input = ActorEquipmentInput {
        armor: [
            Some(WornItem {
                identifier: ITEM.into(),
                kind: HeldKind::Other,
                ..item
            }),
            None,
            None,
            None,
        ],
        ..Default::default()
    };
    let before = runtime.layers_for(&body, &input, None);
    assert_eq!(before.len(), 1);
    assert_eq!(pixel(&base_pages, &before[0]), BASE_PIXEL);

    let (assets, catalog) = compile(
        files
            .into_iter()
            .filter(|(path, _)| path.as_ref() != DEFAULT_PATH)
            .collect(),
    );
    let binding = catalog.binding(ITEM).unwrap();
    assert!(catalog.texture(&binding.texture.identifier).is_none());
    assert!(EquipmentRuntime::actor_glint(&catalog).is_some());
    let (pages, locations) =
        base_pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(&catalog));
    runtime.set_pack_layer(Some((assets, Some(catalog), locations)));
    let after = runtime.layers_for(&body, &input, None);
    assert_eq!(
        after.len(),
        1,
        "a missing pack default must not hide startup armour"
    );
    assert_eq!(pixel(&pages, &after[0]), BASE_PIXEL);
}

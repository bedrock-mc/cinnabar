use std::io::Cursor;

use super::*;

/// One server entity: client definition, geometry, render controller and a solid texture.
fn entity(name: &str, side: u32) -> Vec<(Box<str>, Vec<u8>)> {
    let mut png = Cursor::new(Vec::new());
    RgbaImage::from_pixel(side, side, Rgba([90, 140, 200, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let definition = serde_json::json!({
        "format_version": "1.10.0",
        "minecraft:client_entity": {"description": {
            "identifier": format!("test:{name}"),
            "materials": {"default": "entity_alphatest"},
            "textures": {"default": format!("textures/entity/{name}")},
            "geometry": {"default": format!("geometry.{name}")},
            "render_controllers": [format!("controller.render.{name}")]
        }}
    });
    let geometry = serde_json::json!({
        "format_version": "1.12.0",
        "minecraft:geometry": [{
            "description": {
                "identifier": format!("geometry.{name}"),
                "texture_width": 16,
                "texture_height": 16
            },
            "bones": [{"name": "root", "cubes": [{"origin": [0, 0, 0], "size": [4, 4, 4], "uv": [0, 0]}]}]
        }]
    });
    let controller = serde_json::json!({
        "format_version": "1.8.0",
        "render_controllers": {format!("controller.render.{name}"): {
            "geometry": "Geometry.default",
            "materials": [{"*": "Material.default"}],
            "textures": ["Texture.default"]
        }}
    });
    vec![
        (
            format!("entity/{name}.entity.json").into(),
            serde_json::to_vec(&definition).unwrap(),
        ),
        (
            format!("models/entity/{name}.geo.json").into(),
            serde_json::to_vec(&geometry).unwrap(),
        ),
        (
            format!("render_controllers/{name}.json").into(),
            serde_json::to_vec(&controller).unwrap(),
        ),
        (
            format!("textures/entity/{name}.png").into(),
            png.into_inner(),
        ),
    ]
}

#[test]
fn hundreds_of_megabytes_of_billboard_art_keep_full_resolution() {
    // Five 4096 x 4096 billboards decode to 320 MiB, as large server hub art does.
    let names = ["a", "b", "c", "d", "e"].map(|name| format!("billboard_{name}"));
    let files = names
        .iter()
        .flat_map(|name| entity(name, 4096))
        .collect::<Vec<_>>();
    let compiled = pack_compiler::compile_actor_pack(files)
        .unwrap()
        .expect("every billboard compiles");

    assert!(compiled.fallbacks.is_empty(), "{:?}", compiled.fallbacks);
    assert_eq!(compiled.textures.len(), names.len());
    for texture in &compiled.textures {
        assert_eq!((texture.width, texture.height), (4096, 4096));
    }
}

#[test]
fn a_pack_with_thousands_of_textures_keeps_every_entity_texture() {
    // Sources are taken in path order, so this filler art sorts before the entity's texture.
    let mut filler = Cursor::new(Vec::new());
    RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]))
        .write_to(&mut filler, image::ImageFormat::Png)
        .unwrap();
    let filler = filler.into_inner();
    let mut files: Vec<(Box<str>, Vec<u8>)> = (0..4_700)
        .map(|index| {
            (
                format!("textures/aaa_filler/{index:05}.png").into(),
                filler.clone(),
            )
        })
        .collect();
    files.extend(entity("npc", 16));
    let compiled = pack_compiler::compile_actor_pack(files)
        .unwrap()
        .expect("the entity compiles");

    assert_eq!(compiled.skipped.over_budget, 0);
    assert!(
        compiled.bindings.iter().any(|binding| {
            &*compiled.entities.symbols[binding.entity_symbol as usize].identifier == "test:npc"
        }),
        "the entity lost its texture; fallbacks: {:?}",
        compiled.fallbacks
    );
}

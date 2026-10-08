use std::io::Cursor;

#[test]
fn equipment_texture_compiler_retains_high_resolution_server_attachable() {
    let mut png = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(260, 301, image::Rgba([37, 59, 83, 255]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let attachable = br#"{
        "format_version":"1.10.0",
        "minecraft:attachable":{"description":{
            "identifier":"test:custom_sword",
            "materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/items/custom_sword"},
            "geometry":{"default":"geometry.custom_sword"},
            "render_controllers":["controller.render.custom_sword"]
        }}
    }"#;
    let geometry = br#"{
        "format_version":"1.12.0",
        "minecraft:geometry":[{
            "description":{"identifier":"geometry.custom_sword","texture_width":260,"texture_height":301},
            "bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[8,8,1],"uv":[0,0]}]}]
        }]
    }"#;
    let controller = br#"{
        "format_version":"1.8.0",
        "render_controllers":{"controller.render.custom_sword":{
            "geometry":"Geometry.default",
            "materials":[{"*":"Material.default"}],
            "textures":["Texture.default"]
        }}
    }"#;
    let compiled = pack_compiler::compile_actor_pack(vec![
        ("attachables/custom_sword.json".into(), attachable.to_vec()),
        (
            "models/entity/custom_sword.geo.json".into(),
            geometry.to_vec(),
        ),
        (
            "render_controllers/custom_sword.json".into(),
            controller.to_vec(),
        ),
        ("textures/items/custom_sword.png".into(), png.into_inner()),
    ])
    .unwrap()
    .expect("attachable sources compile");
    let texture = compiled
        .equipment_textures
        .iter()
        .find(|texture| &*texture.identifier == "textures/items/custom_sword")
        .expect("the selected high resolution texture remains available to the held model");
    assert_eq!((texture.width, texture.height), (260, 301));
    assert_eq!(&texture.rgba8[..4], &[37, 59, 83, 255]);
    assets::RuntimeEquipmentCatalog::from_parts(
        compiled.identity,
        compiled.equipment_bindings,
        compiled.equipment_textures,
    )
    .expect("the renderer can receive the full raster in its equipment catalog");
}

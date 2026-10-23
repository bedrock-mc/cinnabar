use super::*;

const ADULT: &str = "textures/entity/example/adult";
const BABY: &str = "textures/entity/example/baby";

/// A piglin-shaped rig: `query.is_baby` picks both the geometry and the texture, and the baby
/// branch comes first.
fn branch_pack(adult: Option<Rgba<u8>>) -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    write(root, "entity/example.entity.json", format!(r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"minecraft:example","materials":{{"default":"entity_alphatest"}},"textures":{{"default":"{ADULT}","baby":"{BABY}"}},"geometry":{{"default":"geometry.example","baby":"geometry.example.baby"}},"render_controllers":["controller.render.example"]}}}}}}"#).as_bytes());
    write(root, "models/entity/example.geo.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.example","texture_width":16,"texture_height":16},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[4,4,4],"uv":[0,0]}]}]},{"description":{"identifier":"geometry.example.baby","texture_width":16,"texture_height":16},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[2,2,2],"uv":[0,0]}]}]}]}"#);
    write(
        root,
        "animations/empty.json",
        br#"{"format_version":"1.8.0","animations":{}}"#,
    );
    write(
        root,
        "animation_controllers/empty.json",
        br#"{"format_version":"1.10.0","animation_controllers":{}}"#,
    );
    write(root, "render_controllers/example.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.example":{"geometry":"query.is_baby ? Geometry.baby : Geometry.default","materials":[{"*":"Material.default"}],"textures":["query.is_baby ? Texture.baby : Texture.default"]}}}"#);
    fs::create_dir_all(root.join("textures/entity/example")).unwrap();
    RgbaImage::from_pixel(16, 16, Rgba([200, 90, 90, 255]))
        .save(root.join(format!("{BABY}.png")))
        .unwrap();
    match adult {
        Some(color) => RgbaImage::from_pixel(16, 16, color)
            .save(root.join(format!("{ADULT}.png")))
            .unwrap(),
        None => write(root, &format!("{ADULT}.png"), b"undecodable"),
    }
    temporary
}

/// Each binding's geometry identifier and route texture path.
fn routes(pack: &TempDir) -> Vec<(String, String)> {
    let entities = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&encode_entity_blob(&entities).unwrap()).unwrap(),
    )
    .unwrap();
    catalog
        .bindings()
        .iter()
        .map(|binding| {
            let texture = &catalog.textures()[binding.texture as usize];
            (
                entities.geometries[binding.geometry as usize]
                    .identifier
                    .to_string(),
                entities.sources[texture.source as usize].path.to_string(),
            )
        })
        .collect()
}

#[test]
fn each_geometry_routes_to_the_texture_its_own_branch_selects() {
    let routes = routes(&branch_pack(Some(Rgba([90, 200, 90, 255]))));
    assert!(!routes.is_empty());
    for (geometry, texture) in &routes {
        let expected = if geometry.ends_with(".baby") {
            BABY
        } else {
            ADULT
        };
        assert_eq!(texture, &format!("{expected}.png"), "{geometry}");
    }
}

#[test]
fn a_missing_adult_texture_never_falls_back_to_the_baby_branch() {
    let routes = routes(&branch_pack(None));
    assert!(!routes.is_empty());
    assert!(
        routes
            .iter()
            .all(|(geometry, texture)| geometry.ends_with(".baby")
                && texture == &format!("{BABY}.png")),
        "{routes:?}"
    );
}

use super::*;

const BASE: &str = "textures/entity/creeper/creeper";
const ARMOR: &str = "textures/entity/creeper/creeper_armor";

/// The vanilla creeper's shape: a base controller plus the charged armor overlay that
/// `query.is_powered` gates, with its own inflated geometry, material and scrolling UVs.
/// `stray` is a texel of the base art given a fractional alpha.
fn creeper_pack(stray: (u32, u32)) -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    write(root, "entity/creeper.entity.json", format!(r#"{{"format_version":"1.10.0","minecraft:client_entity":{{"description":{{"identifier":"minecraft:creeper","materials":{{"default":"creeper","charged":"charged_creeper"}},"textures":{{"default":"{BASE}","charged":"{ARMOR}"}},"geometry":{{"default":"geometry.creeper","charged":"geometry.creeper.charged"}},"render_controllers":["controller.render.creeper",{{"controller.render.creeper_armor":"query.is_powered"}}]}}}}}}"#).as_bytes());
    write(root, "models/entity/creeper.geo.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.creeper","texture_width":64,"texture_height":32},"bones":[{"name":"head","pivot":[0,18,0],"cubes":[{"origin":[-4,18,-4],"size":[8,8,8],"uv":[0,0]}]}]},{"description":{"identifier":"geometry.creeper.charged","texture_width":64,"texture_height":32},"bones":[{"name":"head","pivot":[0,18,0],"cubes":[{"origin":[-4,18,-4],"size":[8,8,8],"uv":[0,0],"inflate":2.0}]}]}]}"#);
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
    write(root, "render_controllers/creeper.render_controllers.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.creeper":{"geometry":"Geometry.default","materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#);
    write(root, "render_controllers/creeper_armor.render_controllers.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.creeper_armor":{"geometry":"Geometry.charged","materials":[{"*":"Material.charged"}],"textures":["Texture.charged"],"uv_anim":{"offset":["query.is_powered ? query.life_time * 0.2 : 0.0","query.is_powered ? query.life_time * 0.2 : 0.0"],"scale":[1.0,1.0]},"ignore_lighting":true}}}"#);
    fs::create_dir_all(root.join("textures/entity/creeper")).unwrap();
    // Opaque green where the head unfolds (u 0..32, v 0..16); clear elsewhere.
    let mut base = RgbaImage::from_fn(64, 32, |x, y| {
        if x < 32 && y < 16 {
            Rgba([60, 200, 60, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    base.put_pixel(stray.0, stray.1, Rgba([2, 155, 0, 8]));
    base.save(root.join(format!("{BASE}.png"))).unwrap();
    RgbaImage::from_fn(64, 32, |x, y| {
        Rgba(if (x + y) % 3 == 0 {
            [40, 90, 230, 255]
        } else {
            [0, 0, 0, 0]
        })
    })
    .save(root.join(format!("{ARMOR}.png")))
    .unwrap();
    temporary
}

fn source_path(entities: &assets::CompiledEntityAssets, source: u32) -> &str {
    entities.sources[source as usize].path.as_ref()
}

#[test]
fn unpowered_creeper_body_draws_the_base_art_and_only_powered_adds_the_armor_layer() {
    // An alpha-8 texel beside the head's unfolded box, which no face samples.
    let pack = creeper_pack((32, 11));
    let entities = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let render = &entities.render;
    assert_eq!(render.layers.len(), 2);
    let layer_art = |layer: &assets::EntityRenderLayer| {
        let slot = &render.slots[layer.first_slot as usize];
        source_path(
            &entities,
            render.candidates[slot.first_candidate as usize].source,
        )
    };
    assert!(render.layers[0].condition.is_none());
    assert_eq!(layer_art(&render.layers[0]), format!("{BASE}.png"));
    assert!(
        render.layers[1].condition.is_some(),
        "the armor overlay draws only while query.is_powered"
    );
    assert_eq!(layer_art(&render.layers[1]), format!("{ARMOR}.png"));
    assert!(render.layers[1].uv_anim.is_some() && render.layers[1].ignore_lighting);

    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        compiled.report.bindings, 1,
        "{:?}",
        compiled.report.fallbacks
    );
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&encode_entity_blob(&entities).unwrap()).unwrap(),
    )
    .unwrap();
    let body = &catalog.textures()[catalog.bindings()[0].texture as usize];
    assert_eq!(
        source_path(&entities, body.source),
        format!("{BASE}.png"),
        "an unpowered creeper's body must not draw the charged armor"
    );
    let texel = |x: usize, y: usize| &body.rgba8[(y * 64 + x) * 4..][..4];
    assert_eq!(texel(0, 8), [60, 200, 60, 255]);
    assert_eq!(texel(32, 11)[3], 0, "the unsampled texel is clear");
    assert!(
        catalog
            .textures()
            .iter()
            .any(|texture| source_path(&entities, texture.source) == format!("{ARMOR}.png")),
        "the powered overlay keeps its art"
    );
}

#[test]
fn rejected_base_art_falls_back_instead_of_borrowing_the_overlay() {
    // A fractional texel the head's front face samples still fails the binary alpha contract.
    let pack = creeper_pack((10, 10));
    let compiled = compile_actor_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.report.bindings, 0);
    assert!(
        compiled
            .report
            .fallbacks
            .iter()
            .any(|entry| entry.reason.as_ref() == "missing_or_ambiguous_texture")
    );
}

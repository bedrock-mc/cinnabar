use std::{fs, path::Path};

use assets::{
    ItemTextureReference, ItemVisualDefinition, ItemVisualDefinitionRoute, RuntimeIconCatalog,
    encode_entity_blob,
};
use pack_compiler::{compile_entity_assets, compile_icon_assets};
use tempfile::TempDir;

const MANIFEST: &[u8] = include_bytes!("../../../../assets/vanilla-source.json");

#[path = "item_visuals/spawn_eggs.rs"]
mod spawn_eggs;

#[path = "item_visuals/beds.rs"]
mod beds;

#[test]
fn leather_tga_atlas_sources_keep_their_opaque_untinted_trim() {
    let pack = item_pack(false);
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{
        "texture_data": {"helmet": {"textures": "textures/items/leather_helmet"}}
    }"#,
    );
    let image = image::RgbaImage::from_fn(2, 1, |x, _| {
        image::Rgba(if x == 0 {
            [200, 100, 50, 3]
        } else {
            [200, 100, 50, 255]
        })
    });
    image
        .save(pack.path().join("textures/items/leather_helmet.tga"))
        .unwrap();
    let compiled = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let icons = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    let sprite = icons
        .lookup("minecraft:leather_helmet", 0)
        .expect("TGA leather icon");
    assert_eq!(sprite.rgba8[3], 255);
    assert_eq!(sprite.rgba8[7], 255);
    assert!(
        sprite.rgba8[0] > sprite.rgba8[4],
        "trim keeps its source color"
    );
}

fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn item_pack(reverse: bool) -> TempDir {
    let temporary = tempfile::tempdir().unwrap();
    let files: [(&str, &[u8]); 9] = [
        ("entity/item.entity.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#),
        ("models/entity/item.geo.json", br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#),
        ("animations/empty.json", br#"{"format_version":"1.8.0","animations":{}}"#),
        ("animation_controllers/empty.json", br#"{"format_version":"1.10.0","animation_controllers":{}}"#),
        ("render_controllers/item.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"}}}"#),
        ("textures/entity/item.png", b"entity-raster"),
        ("textures/item_texture.json", br#"{"resource_pack_name":"synthetic","texture_name":"atlas.items","texture_data":{"apple":{"textures":["textures/items/apple","textures/items/apple_alt"]},"missing":{"textures":"textures/items/missing"},"stone":{"textures":"textures/blocks/stone"}}}"#),
        ("textures/items/apple.png", b"apple-raster"),
        ("textures/items/apple_alt.png", b"apple-alt-raster"),
    ];
    let iterator: Box<dyn Iterator<Item = &(&str, &[u8])>> = if reverse {
        Box::new(files.iter().rev())
    } else {
        Box::new(files.iter())
    };
    for (path, bytes) in iterator {
        write(temporary.path(), path, bytes);
    }
    temporary
}

fn visual<'a>(
    visuals: &'a [ItemVisualDefinition],
    identifier: &str,
    metadata: u32,
) -> &'a ItemVisualDefinition {
    visuals
        .iter()
        .find(|visual| {
            visual.key.identifier.as_ref() == identifier && visual.key.metadata == metadata
        })
        .unwrap()
}

#[test]
fn compiles_exact_metadata_variants_reviewed_block_routes_and_missing_items() {
    let first = compile_entity_assets(item_pack(false).path(), MANIFEST).unwrap();
    let second = compile_entity_assets(item_pack(true).path(), MANIFEST).unwrap();
    assert_eq!(
        encode_entity_blob(&first).unwrap(),
        encode_entity_blob(&second).unwrap()
    );
    assert_eq!(first.block_visual_count, 22_091);
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:air", 0).route,
        ItemVisualDefinitionRoute::EmptyHand
    ));
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:apple", 0).route,
        ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference { variant: 0, .. }
        }
    ));
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:apple", 1).route,
        ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference { variant: 1, .. }
        }
    ));
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:stone", 0).route,
        ItemVisualDefinitionRoute::BlockItem { .. }
    ));
    assert!(matches!(
        visual(&first.item_visuals, "minecraft:missing", 0).route,
        ItemVisualDefinitionRoute::Missing
    ));
    assert!(first.item_visual_aliases.is_empty());
}

#[test]
fn item_texture_variants_must_be_nonempty_strings() {
    for textures in [
        serde_json::json!([]),
        serde_json::json!(["textures/items/apple", 3]),
    ] {
        let pack = item_pack(false);
        let atlas = serde_json::json!({
            "resource_pack_name": "synthetic",
            "texture_name": "atlas.items",
            "texture_data": {"apple": {"textures": textures}}
        });
        write(
            pack.path(),
            "textures/item_texture.json",
            &serde_json::to_vec(&atlas).unwrap(),
        );
        assert!(compile_entity_assets(pack.path(), MANIFEST).is_err());
    }
}

#[test]
fn no_pack_sidecar_is_required_or_consumed() {
    let pack = item_pack(false);
    write(
        pack.path(),
        "textures/item_visuals.json",
        br#"{"invented":"policy"}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(matches!(
        visual(&compiled.item_visuals, "minecraft:stone", 0).route,
        ItemVisualDefinitionRoute::BlockItem { .. }
    ));
    assert!(
        !compiled
            .sources
            .iter()
            .any(|source| source.path.as_ref() == "textures/item_visuals.json")
    );
}

#[test]
fn reviewed_block_routes_and_empty_hand_do_not_depend_on_an_item_atlas() {
    let pack = item_pack(false);
    fs::remove_file(pack.path().join("textures/item_texture.json")).unwrap();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let stone = visual(&compiled.item_visuals, "minecraft:stone", 0);
    assert!(matches!(
        stone.route,
        ItemVisualDefinitionRoute::BlockItem { .. }
    ));
    assert!(matches!(
        visual(&compiled.item_visuals, "minecraft:air", 0).route,
        ItemVisualDefinitionRoute::EmptyHand
    ));
    assert_eq!(
        compiled.sources[stone.source as usize].path.as_ref(),
        "registry/block-item-routes-v2193.json"
    );
}

#[test]
fn canonical_default_bindings_restore_bundle_and_spear_inventory_keys() {
    let pack = item_pack(false);
    write(pack.path(), "textures/item_texture.json", br#"{"texture_data":{"bundle_blue":{"textures":"textures/items/blue"},"gold_spear":{"textures":"textures/items/gold"},"wood_spear":{"textures":"textures/items/wood"}}}"#);
    for name in ["blue", "gold", "wood"] {
        write(
            pack.path(),
            &format!("textures/items/{name}.png"),
            b"sprite",
        );
    }
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    for (identifier, source) in [
        ("minecraft:blue_bundle", "textures/items/blue.png"),
        ("minecraft:golden_spear", "textures/items/gold.png"),
        ("minecraft:wooden_spear", "textures/items/wood.png"),
    ] {
        let definition = visual(&compiled.item_visuals, identifier, 0);
        let ItemVisualDefinitionRoute::Sprite { texture } = definition.route else {
            panic!("canonical default did not resolve a sprite");
        };
        assert_eq!(
            compiled.sources[texture.source as usize].path.as_ref(),
            source
        );
        assert_eq!(texture.variant, 0);
    }
}

#[test]
fn canonical_default_conflicts_are_rejected_without_replacing_atlas_routes() {
    let pack = item_pack(false);
    write(pack.path(), "textures/item_texture.json", br#"{"texture_data":{"bundle_blue":{"textures":"textures/items/blue"},"blue_bundle":{"textures":"textures/items/other"}}}"#);
    for name in ["blue", "other"] {
        write(
            pack.path(),
            &format!("textures/items/{name}.png"),
            b"sprite",
        );
    }
    assert!(compile_entity_assets(pack.path(), MANIFEST).is_err());
    // Unavailable pixels cannot conceal two conflicting logical sources.
    fs::remove_file(pack.path().join("textures/items/blue.png")).unwrap();
    fs::remove_file(pack.path().join("textures/items/other.png")).unwrap();
    assert!(compile_entity_assets(pack.path(), MANIFEST).is_err());
}

#[test]
fn food_default_icons_reach_lookup_when_the_atlas_keys_differ_from_item_names() {
    let pack = item_pack(false);
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{"texture_data":{"potato_baked":{"textures":"textures/items/cooked"},"beef_raw":{"textures":"textures/items/raw"}}}"#,
    );
    for (name, rgba) in [("cooked", [31, 127, 73, 255]), ("raw", [173, 43, 29, 255])] {
        image::save_buffer(
            pack.path().join(format!("textures/items/{name}.png")),
            &rgba,
            1,
            1,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    let compiled = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    for (identifier, expected) in [
        ("minecraft:baked_potato", [31, 127, 73, 255]),
        ("minecraft:beef", [173, 43, 29, 255]),
    ] {
        let sprite = catalog.lookup(identifier, 0).expect("food default icon");
        assert_eq!(sprite.rgba8.as_ref(), &expected);
    }
}

#[test]
fn canonical_default_icons_reach_consumer_lookup_without_duplicate_pixels() {
    let pack = item_pack(false);
    write(pack.path(), "textures/item_texture.json", br#"{"texture_data":{"bundle_blue":{"textures":"textures/items/shared"},"bundle_light_blue":{"textures":"textures/items/shared"}}}"#);
    image::save_buffer(
        pack.path().join("textures/items/shared.png"),
        &[20, 80, 160, 255],
        1,
        1,
        image::ColorType::Rgba8,
    )
    .unwrap();
    let first = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let second = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(
        first.report.block_visuals, 0,
        "the legacy sprite-only entry point must not silently invent block artwork"
    );
    assert_eq!(first.report.sprites, 1);
    let catalog = RuntimeIconCatalog::decode(&first.bytes).unwrap();
    let blue = catalog.lookup_index("minecraft:blue_bundle", 0).unwrap();
    assert_eq!(
        catalog.lookup_index("minecraft:light_blue_bundle", 0),
        Some(blue)
    );
    // Preserve the consumer's existing metadata-zero fallback policy.
    assert_eq!(catalog.lookup_index("minecraft:blue_bundle", 7), Some(blue));
    assert!(
        catalog
            .lookup_index("minecraft:unknown_bundle", 0)
            .is_none()
    );
}

#[test]
fn absent_binding_aliases_do_not_invent_routes_and_missing_pixels_stay_missing() {
    let pack = item_pack(false);
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{"texture_data":{"bundle_blue":{"textures":"textures/items/not_present"}}}"#,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert!(matches!(
        visual(&compiled.item_visuals, "minecraft:blue_bundle", 0).route,
        ItemVisualDefinitionRoute::Missing
    ));
    assert!(
        !compiled
            .item_visuals
            .iter()
            .any(|item| item.key.identifier.as_ref() == "minecraft:golden_spear")
    );
    assert!(matches!(
        visual(&compiled.item_visuals, "minecraft:stone", 0).route,
        ItemVisualDefinitionRoute::BlockItem { .. }
    ));
}

#[test]
fn all_reviewed_defaults_emit_canonical_keys_without_auxiliary_metadata_routes() {
    let pack = item_pack(false);
    let facts: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../assets/data/default-sprite-bindings-1.26.50.json"
    ))
    .unwrap();
    let rows = facts["routes"].as_array().unwrap();
    assert!(!rows.is_empty());
    let mut atlas = serde_json::Map::new();
    for (index, row) in rows.iter().enumerate() {
        // Authored fixture paths intentionally do not resemble item identifiers.
        let path = format!("textures/items/fixture_{index}");
        atlas.insert(
            row["default_alias"].as_str().unwrap().to_owned(),
            serde_json::json!({"textures": path}),
        );
        write(pack.path(), &format!("{path}.png"), b"sprite");
    }
    write(
        pack.path(),
        "textures/item_texture.json",
        &serde_json::to_vec(&serde_json::json!({"texture_data": atlas})).unwrap(),
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    for (index, row) in rows.iter().enumerate() {
        let identifier = row["identifier"].as_str().unwrap();
        let item = visual(&compiled.item_visuals, identifier, 0);
        let ItemVisualDefinitionRoute::Sprite { texture } = item.route else {
            panic!("missing reviewed default");
        };
        assert_eq!(
            compiled.sources[texture.source as usize].path.as_ref(),
            format!("textures/items/fixture_{index}.png")
        );
        assert_eq!(
            compiled.sources[item.source as usize].path.as_ref(),
            "registry/default-sprite-bindings-1.26.50.json"
        );
        assert_eq!(texture.variant, 0);
    }
    assert!(
        !compiled
            .item_visuals
            .iter()
            .any(
                |item| item.key.identifier.as_ref() == "minecraft:blue_bundle"
                    && item.key.metadata != 0
            )
    );
}

#[test]
fn native_seed_components_use_item_sprites_instead_of_the_crop_they_place() {
    let pack = item_pack(false);
    let bindings: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../assets/data/default-sprite-bindings-1.26.50.json"
    ))
    .unwrap();
    let seeds = bindings["routes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["evidence_file"]
                .as_str()
                .unwrap()
                .starts_with("native/")
        })
        .collect::<Vec<_>>();
    assert!(
        seeds
            .iter()
            .any(|row| row["identifier"] == "minecraft:wheat_seeds")
    );
    let mut atlas = serde_json::Map::new();
    for row in &seeds {
        let alias = row["default_alias"].as_str().unwrap();
        let path = format!("textures/items/{alias}");
        atlas.insert(alias.to_owned(), serde_json::json!({"textures": path}));
        image::save_buffer(
            pack.path().join(format!("{path}.png")),
            &[17, 93, 41, 255],
            1,
            1,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    write(
        pack.path(),
        "textures/item_texture.json",
        &serde_json::to_vec(&serde_json::json!({"texture_data": atlas})).unwrap(),
    );
    let entity = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let decoded =
        assets::RuntimeEntityAssets::decode(&encode_entity_blob(&entity).unwrap()).unwrap();
    let icons = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let catalog = RuntimeIconCatalog::decode(&icons.bytes).unwrap();
    for row in seeds {
        let identifier = row["identifier"].as_str().unwrap();
        let alias = row["default_alias"].as_str().unwrap();
        assert!(matches!(
            visual(decoded.item_visuals(), identifier, 0).route,
            ItemVisualDefinitionRoute::Sprite { .. }
        ));
        assert_eq!(
            catalog.lookup_index(identifier, 0),
            catalog.lookup_index(&format!("minecraft:{alias}"), 0)
        );
        let sprite = &catalog.sprites()[catalog.lookup_index(identifier, 0).unwrap()];
        assert_eq!(sprite.rgba8.as_ref(), &[17, 93, 41, 255]);
    }
}

// Legacy retail icon routes key identifiers onto atlas variants; absent atlas keys add nothing.
#[test]
fn legacy_icon_routes_bind_identifiers_to_their_atlas_variants() {
    let pack = tempfile::tempdir().unwrap();
    let swords = (0..7)
        .map(|index| format!("\"textures/items/sword_{index}\""))
        .collect::<Vec<_>>()
        .join(",");
    let atlas = format!(
        r#"{{"texture_data":{{"sword":{{"textures":[{swords}]}},"compass_item":{{"textures":"textures/items/compass_item"}}}}}}"#
    );
    for (path, bytes) in [
        ("entity/item.entity.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#.to_vec()),
        ("models/entity/item.geo.json", br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#.to_vec()),
        ("animations/empty.json", br#"{"format_version":"1.8.0","animations":{}}"#.to_vec()),
        ("animation_controllers/empty.json", br#"{"format_version":"1.10.0","animation_controllers":{}}"#.to_vec()),
        ("render_controllers/item.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"}}}"#.to_vec()),
        ("textures/entity/item.png", b"entity-raster".to_vec()),
        ("textures/item_texture.json", atlas.into_bytes()),
        ("textures/items/compass_item.png", b"compass-raster".to_vec()),
    ] {
        write(pack.path(), path, &bytes);
    }
    for index in 0..7 {
        write(
            pack.path(),
            &format!("textures/items/sword_{index}.png"),
            format!("sword-{index}").as_bytes(),
        );
    }
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let sprite = |identifier: &str| match visual(&compiled.item_visuals, identifier, 0).route {
        ItemVisualDefinitionRoute::Sprite { texture } => {
            compiled.sources[texture.source as usize].path.to_string()
        }
        route => panic!("{identifier} routed to {route:?}"),
    };
    assert_eq!(
        sprite("minecraft:diamond_sword"),
        "textures/items/sword_4.png"
    );
    assert_eq!(
        sprite("minecraft:compass"),
        "textures/items/compass_item.png"
    );
    assert!(
        !compiled
            .item_visuals
            .iter()
            .any(|visual| visual.key.identifier.as_ref() == "minecraft:bow")
    );
    assets::RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap();
}

use super::*;

fn actor(root: &Path, file: &str, identifier: &str, version: &str, egg: serde_json::Value) {
    write(
        root,
        &format!("entity/{file}.entity.json"),
        serde_json::to_string(&serde_json::json!({
            "format_version":"1.10.0",
            "minecraft:client_entity":{"description":{
                "identifier":identifier,"min_engine_version":version,"spawn_egg":egg
            }}
        }))
        .unwrap()
        .as_bytes(),
    );
}

fn egg_pack() -> TempDir {
    let pack = item_pack(false);
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{"texture_data":{"unrelated_alias":{"textures":["textures/items/base","textures/items/selected"]},"modern_alias":{"textures":"textures/items/modern"}}}"#,
    );
    for (name, color) in [
        ("base", [1, 2, 3, 255]),
        ("selected", [17, 31, 63, 255]),
        ("modern", [90, 80, 70, 255]),
    ] {
        image::save_buffer(
            pack.path().join(format!("textures/items/{name}.png")),
            &color,
            1,
            1,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
    pack
}

fn texture_path<'a>(compiled: &'a assets::CompiledEntityAssets, identifier: &str) -> &'a str {
    let ItemVisualDefinitionRoute::Sprite { texture } =
        visual(&compiled.item_visuals, identifier, 0).route
    else {
        panic!("spawn egg did not resolve to its declared sprite")
    };
    &compiled.sources[texture.source as usize].path
}

#[test]
fn resolves_actor_spawn_egg_alias_and_index_at_stack_metadata_zero() {
    let pack = egg_pack();
    actor(
        pack.path(),
        "sheep",
        "minecraft:sheep",
        "1.0.0",
        serde_json::json!({"texture":"unrelated_alias","texture_index":1}),
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let egg = visual(&compiled.item_visuals, "minecraft:sheep_spawn_egg", 0);
    assert_eq!(
        compiled.sources[egg.source as usize].path.as_ref(),
        "entity/sheep.entity.json"
    );
    assert!(matches!(
        egg.route,
        ItemVisualDefinitionRoute::Sprite {
            texture: ItemTextureReference { variant: 1, .. }
        }
    ));
    assert_eq!(
        texture_path(&compiled, "minecraft:sheep_spawn_egg"),
        "textures/items/selected.png"
    );
    assert!(!compiled.item_visuals.iter().any(|visual| {
        visual.key.identifier.as_ref() == "minecraft:sheep_spawn_egg" && visual.key.metadata != 0
    }));
    let icons = compile_icon_assets(pack.path(), MANIFEST).unwrap();
    let catalog = RuntimeIconCatalog::decode(&icons.bytes).unwrap();
    assert_eq!(
        catalog
            .lookup("minecraft:sheep_spawn_egg", 0)
            .unwrap()
            .rgba8
            .as_ref(),
        &[17, 31, 63, 255]
    );
}

#[test]
fn selects_newest_compatible_actor_definition_not_lexical_filename() {
    let pack = egg_pack();
    for (file, version, texture) in [
        ("z_old", "1.0.0", "unrelated_alias"),
        ("a_modern", "1.17.10", "modern_alias"),
        ("b_future", "999.0.0", "unrelated_alias"),
    ] {
        actor(
            pack.path(),
            file,
            "minecraft:sheep",
            version,
            serde_json::json!({"texture":texture}),
        );
    }
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        texture_path(&compiled, "minecraft:sheep_spawn_egg"),
        "textures/items/modern.png"
    );
}

#[test]
fn routes_native_actor_renames_and_prefers_current_villagers() {
    let pack = egg_pack();
    for (actor_name, item_name) in [
        ("evocation_illager", "evoker"),
        ("tropicalfish", "tropical_fish"),
        ("villager_v2", "villager"),
        ("zombie_villager_v2", "zombie_villager"),
    ] {
        actor(
            pack.path(),
            actor_name,
            &format!("minecraft:{actor_name}"),
            "0.0.0",
            serde_json::json!({"texture":"modern_alias"}),
        );
        if actor_name.ends_with("_v2") {
            actor(
                pack.path(),
                item_name,
                &format!("minecraft:{item_name}"),
                "1.8.0",
                serde_json::json!({"texture":"unrelated_alias"}),
            );
        }
    }
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    for name in ["evoker", "tropical_fish", "villager", "zombie_villager"] {
        assert_eq!(
            texture_path(&compiled, &format!("minecraft:{name}_spawn_egg")),
            "textures/items/modern.png"
        );
    }
}

#[test]
fn absent_alias_or_texture_is_missing_not_a_guessed_egg() {
    let pack = egg_pack();
    actor(
        pack.path(),
        "sheep",
        "minecraft:sheep",
        "1.0.0",
        serde_json::json!({"texture":"absent"}),
    );
    actor(
        pack.path(),
        "cow",
        "minecraft:cow",
        "1.0.0",
        serde_json::json!({"texture":"unrelated_alias","texture_index":99}),
    );
    actor(
        pack.path(),
        "pig",
        "minecraft:pig",
        "1.0.0",
        serde_json::json!({"texture":"absent_raster"}),
    );
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{"texture_data":{"unrelated_alias":{"textures":["textures/items/base","textures/items/selected"]},"modern_alias":{"textures":"textures/items/modern"},"absent_raster":{"textures":"textures/items/absent"}}}"#,
    );
    actor(
        pack.path(),
        "custom",
        "custom:unknown",
        "1.0.0",
        serde_json::json!({"texture":"modern_alias"}),
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    for identifier in [
        "minecraft:sheep_spawn_egg",
        "minecraft:cow_spawn_egg",
        "minecraft:pig_spawn_egg",
    ] {
        assert_eq!(
            visual(&compiled.item_visuals, identifier, 0).route,
            ItemVisualDefinitionRoute::Missing
        );
    }
    assert!(
        !compiled
            .item_visuals
            .iter()
            .any(|visual| { visual.key.identifier.as_ref() == "custom:unknown_spawn_egg" })
    );
}

#[test]
fn exact_canonical_atlas_conflicts_are_rejected_not_silently_overwritten() {
    let pack = egg_pack();
    actor(
        pack.path(),
        "sheep",
        "minecraft:sheep",
        "1.0.0",
        serde_json::json!({"texture":"unrelated_alias"}),
    );
    write(
        pack.path(),
        "textures/item_texture.json",
        br#"{"texture_data":{"unrelated_alias":{"textures":"textures/items/base"},"sheep_spawn_egg":{"textures":"textures/items/modern"}}}"#,
    );
    let error = compile_entity_assets(pack.path(), MANIFEST).unwrap_err();
    assert!(error.to_string().contains("actor spawn egg conflicts"));
}

#[test]
fn selected_definition_without_egg_does_not_reuse_an_older_icon() {
    let pack = egg_pack();
    actor(
        pack.path(),
        "sheep_old",
        "minecraft:sheep",
        "1.0.0",
        serde_json::json!({"texture":"modern_alias"}),
    );
    actor(
        pack.path(),
        "sheep_new",
        "minecraft:sheep",
        "1.17.10",
        serde_json::Value::Null,
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        visual(&compiled.item_visuals, "minecraft:sheep_spawn_egg", 0).route,
        ItemVisualDefinitionRoute::Missing
    );
}

#[test]
fn custom_color_composition_is_not_claimed_as_an_untinted_sprite() {
    let pack = egg_pack();
    actor(
        pack.path(),
        "sheep",
        "minecraft:sheep",
        "1.0.0",
        serde_json::json!({"texture":"modern_alias","base_color":"#102030"}),
    );
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(
        visual(&compiled.item_visuals, "minecraft:sheep_spawn_egg", 0).route,
        ItemVisualDefinitionRoute::Missing
    );
}

#[test]
fn exact_pinned_pack_renders_every_retail_spawn_egg() {
    let Some(pack) = std::env::var_os("PINNED_BEDROCK_SAMPLES_PACK").map(std::path::PathBuf::from)
    else {
        eprintln!("skipping missing fixture: PINNED_BEDROCK_SAMPLES_PACK is not set");
        return;
    };
    if !pack.exists() {
        eprintln!(
            "skipping missing fixture: PINNED_BEDROCK_SAMPLES_PACK at {}",
            pack.display()
        );
        return;
    }
    let compiled = compile_entity_assets(&pack, MANIFEST).unwrap();
    let icons = compile_icon_assets(&pack, MANIFEST).unwrap();
    let catalog = RuntimeIconCatalog::decode(&icons.bytes).unwrap();
    let retail = include_str!("../../../../protocol/data/retail_items_1_26_50.tsv");
    let mut egg_count = 0;
    for identifier in retail
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(_, identifier)| identifier))
        .filter(|identifier| identifier.ends_with("_spawn_egg"))
    {
        let route = visual(&compiled.item_visuals, identifier, 0);
        assert!(
            matches!(route.route, ItemVisualDefinitionRoute::Sprite { .. }),
            "{identifier}: {:?}",
            route.route
        );
        assert!(catalog.lookup(identifier, 0).is_some(), "{identifier}");
        assert!(
            compiled.sources[route.source as usize]
                .path
                .starts_with("entity/")
        );
        egg_count += 1;
    }
    assert!(egg_count > 0);
    println!("Verified {egg_count} canonical retail spawn eggs through compiled icon lookup");
}

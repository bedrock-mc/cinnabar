//! Loading block/item/entity id maps from a pack, with option-gated entries.

use shaderpack::{Environment, OptionValues, PackSource, ShaderPack};

/// A pack whose `SWAYING_FOLIAGE` option switches block ids, plus item and entity maps.
fn pack_with_maps() -> ShaderPack {
    ShaderPack::new(PackSource::from_files([
        (
            "lib/settings.glsl",
            "//#define SWAYING_FOLIAGE\n#ifdef SWAYING_FOLIAGE\n#endif\n",
        ),
        (
            "block.properties",
            "#ifdef SWAYING_FOLIAGE\n\
             block.200=meadow_tuft cobalt:reed_bunch:half=upper\n\
             #endif\n\
             block.201=meadow_tuft cobalt:reed_bunch\n",
        ),
        ("item.properties", "item.40=cobalt:ember_rod\n"),
        ("entity.properties", "entity.7=drifter\n"),
    ]))
}

#[test]
fn option_gated_block_entries_follow_the_chosen_options() {
    let pack = pack_with_maps();
    let environment = Environment::default();
    let off = pack.id_maps(&environment, &OptionValues::new());
    let on = pack.id_maps(
        &environment,
        &OptionValues::from([("SWAYING_FOLIAGE".to_owned(), "true".to_owned())]),
    );
    let no_tags = |_: &str| false;
    let upper = [("half", "upper")];
    let lower = [("half", "lower")];
    assert_eq!(
        off.blocks.block_id("minecraft:meadow_tuft", &[], &no_tags),
        Some(201)
    );
    assert_eq!(
        off.blocks.block_id("cobalt:reed_bunch", &upper, &no_tags),
        Some(201)
    );
    assert_eq!(
        on.blocks.block_id("minecraft:meadow_tuft", &[], &no_tags),
        Some(200)
    );
    assert_eq!(
        on.blocks.block_id("cobalt:reed_bunch", &upper, &no_tags),
        Some(200)
    );
    assert_eq!(
        on.blocks.block_id("cobalt:reed_bunch", &lower, &no_tags),
        Some(201)
    );
    assert_eq!(on.items.id("cobalt:ember_rod"), Some(40));
    assert_eq!(on.entities.id("minecraft:drifter"), Some(7));
    assert!(on.diagnostics.is_empty());
}

#[test]
fn a_pack_without_block_properties_gets_iris_legacy_block_ids() {
    let pack = ShaderPack::new(PackSource::from_files([(
        "item.properties",
        "item.3=cobalt:ember_rod\n",
    )]));
    let maps = pack.id_maps(&Environment::default(), &OptionValues::new());
    let no_tags = |_: &str| false;
    assert_eq!(
        maps.blocks.block_id("minecraft:glowstone", &[], &no_tags),
        Some(89)
    );
    assert!(maps.entities.is_empty());
    assert_eq!(maps.items.id("cobalt:ember_rod"), Some(3));
}

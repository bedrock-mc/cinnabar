use super::*;

#[test]
fn renamed_item_descriptions_resolve_through_serialization_identities() {
    let cases = [
        ("minecraft:cod", "item.fish.name"),
        ("minecraft:filled_map", "item.map.name"),
        ("minecraft:firework_rocket", "item.fireworks.name"),
        ("minecraft:empty_map", "item.emptyMap.name"),
        ("minecraft:music_disc_13", "item.record.name"),
        ("minecraft:apple", "item.apple.name"),
        ("minecraft:grass_block", "tile.grass.name"),
    ];
    let mut entries = Vec::new();
    for (identifier, key) in cases {
        // The full pinned item table; the creative-only registry omits filled maps.
        assert!(assets::item::names::serialization_ids(identifier).is_some());
        entries.push(assets::LangEntry {
            key: key.into(),
            value: format!("Localized {key}").into(),
        });
    }
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    let bytes = assets::encode_lang_catalog([9; 32], [10; 32], &entries).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(std::sync::Arc::new(
        assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
    ));
    for (identifier, key) in cases {
        assert_eq!(
            runtime.localized_item_name(identifier),
            format!("Localized {key}"),
            "{identifier}"
        );
    }
}

#[test]
fn potions_and_tipped_arrows_are_named_by_their_aux() {
    let keys = [
        ("potion.moveSpeed.name", "Potion of Swiftness"),
        ("potion.moveSpeed.splash.name", "Splash Potion of Swiftness"),
        ("potion.oozing.linger.name", "Lingering Potion of Oozing"),
        ("tipped_arrow.effect.moveSpeed", "Arrow of Swiftness"),
        ("item.arrow.name", "Arrow"),
    ];
    let mut entries: Vec<_> = keys
        .iter()
        .map(|(key, value)| assets::LangEntry {
            key: (*key).into(),
            value: (*value).into(),
        })
        .collect();
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    let bytes = assets::encode_lang_catalog([9; 32], [10; 32], &entries).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(std::sync::Arc::new(
        assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
    ));
    let name = |identifier, aux| {
        crate::ui_runtime::presentation::inventory_tooltip::default_name(&runtime, identifier, aux)
    };
    assert_eq!(name("minecraft:potion", 14), "Potion of Swiftness");
    assert_eq!(
        name("minecraft:splash_potion", 14),
        "Splash Potion of Swiftness"
    );
    assert_eq!(
        name("minecraft:lingering_potion", 45),
        "Lingering Potion of Oozing"
    );
    // A tipped arrow's aux is one more than its potion's.
    assert_eq!(name("minecraft:arrow", 15), "Arrow of Swiftness");
    assert_eq!(name("minecraft:arrow", 0), "Arrow");
}

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
    let items = protocol::vanilla_item_registry();
    let mut entries = Vec::new();
    for (identifier, key) in cases {
        assert!(
            items
                .iter()
                .any(|item| item.identifier.as_ref() == identifier)
        );
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

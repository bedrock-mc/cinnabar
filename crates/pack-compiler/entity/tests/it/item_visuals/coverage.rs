use super::*;
use std::collections::BTreeMap;

#[test]
fn every_registry_item_resolves_a_presentation_route() {
    let pack = presentation_pack();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let missing = assets::item::names::identifiers()
        .filter(|identifier| {
            matches!(
                visual(&compiled.item_visuals, identifier, 0).route,
                ItemVisualDefinitionRoute::Missing
            )
        })
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "items without presentation rules: {missing:?}"
    );
}

/// Supplies original placeholder pixels for the pinned atlas vocabulary, retaining route identities.
fn presentation_pack() -> TempDir {
    let pack = item_pack(false);
    let mut variants = BTreeMap::<&str, usize>::new();
    for row in
        include_str!("../../../../../assets/data/vanilla-pack-vocabulary-1.26.50.tsv").lines()
    {
        let Some(("item_texture", key)) = row.split_once('\t') else {
            continue;
        };
        variants.insert(key, 1);
    }
    for row in include_str!("../../../../../assets/data/legacy-icon-routes-26.30.tsv").lines() {
        let fields = row.split('\t').collect::<Vec<_>>();
        if let Some(count) = variants.get_mut(fields[2]) {
            *count = (*count).max(fields[3].parse::<usize>().unwrap() + 1);
        }
    }
    let texture_data = variants
        .into_iter()
        .map(|(key, count)| {
            (
                key,
                serde_json::json!({"textures": vec!["textures/items/coverage"; count]}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    write(
        pack.path(),
        "textures/item_texture.json",
        &serde_json::to_vec(&serde_json::json!({"texture_data": texture_data})).unwrap(),
    );
    write(
        pack.path(),
        "textures/items/coverage.png",
        b"original fixture",
    );
    // Egg routes belong to client entities. Their actor identities come from the pinned vocabulary.
    for (index, row) in
        include_str!("../../../../../assets/data/vanilla-pack-vocabulary-1.26.50.tsv")
            .lines()
            .enumerate()
    {
        let Some(("actor", identifier)) = row.split_once('\t') else {
            continue;
        };
        let entity = serde_json::json!({
            "format_version": "1.10.0",
            "minecraft:client_entity": {"description": {
                "identifier": identifier,
                "spawn_egg": {"texture": "spawn_egg", "texture_index": 0}
            }}
        });
        write(
            pack.path(),
            &format!("entity/coverage_{index}.entity.json"),
            &serde_json::to_vec(&entity).unwrap(),
        );
    }
    pack
}

/// Vanilla gives every registry block that no named item claims a plain block item under the
/// block's name and draws it as that block, so server-given items such as the nether reactor
/// core show their block. An atlas sprite of the same name keeps the sprite.
#[test]
fn blocks_without_a_named_item_draw_as_their_block() {
    let pack = presentation_pack();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    let registry = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let mut first_states = BTreeMap::new();
    for record in registry.iter() {
        first_states
            .entry(record.name.as_ref())
            .or_insert(record.sequential_id);
    }
    let unclaimed = first_states
        .iter()
        .filter(|(name, _)| {
            **name != assets::facts::block::RESERVED_BLOCK_NAME
                && assets::item::names::serialization_ids(name).is_none()
        })
        .collect::<Vec<_>>();
    let unresolved = unclaimed
        .iter()
        .filter(|(name, _)| {
            compiled
                .item_visuals
                .iter()
                .find(|visual| visual.key.identifier.as_ref() == **name && visual.key.metadata == 0)
                .is_none_or(|visual| matches!(visual.route, ItemVisualDefinitionRoute::Missing))
        })
        .map(|(name, _)| **name)
        .collect::<Vec<_>>();
    assert!(
        unresolved.is_empty(),
        "unclaimed blocks without an item: {unresolved:?}"
    );
    for name in [
        "minecraft:netherreactor",
        "minecraft:glowingobsidian",
        "minecraft:info_update",
    ] {
        assert_eq!(
            visual(&compiled.item_visuals, name, 0).route,
            ItemVisualDefinitionRoute::BlockItem {
                block_visual: assets::BlockVisualId(first_states[name]),
            },
            "{name}"
        );
    }
    assert!(matches!(
        visual(&compiled.item_visuals, "minecraft:camera", 0).route,
        ItemVisualDefinitionRoute::Sprite { .. }
    ));
}

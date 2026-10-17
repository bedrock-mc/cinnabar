use super::*;

#[test]
fn pinned_fish_models_keep_artwork_for_every_geometry_and_controller() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ACTOR_ROOT").map(std::path::PathBuf::from)
    else {
        eprintln!("skipping missing fixture: CINNABAR_VANILLA_ACTOR_ROOT is not set");
        return;
    };
    if !root.exists() {
        eprintln!(
            "skipping missing fixture: CINNABAR_VANILLA_ACTOR_ROOT at {}",
            root.display()
        );
        return;
    }
    let root = Path::new(&root);
    let entities = compile_entity_assets(root, MANIFEST).unwrap();
    let entity_bytes = encode_entity_blob(&entities).unwrap();
    let compiled = compile_actor_assets(root, MANIFEST).unwrap();
    let catalog = RuntimeActorCatalog::decode(
        &compiled.bytes,
        &assets::RuntimeEntityAssets::decode(&entity_bytes).unwrap(),
    )
    .unwrap();
    for identifier in [
        "minecraft:cod",
        "minecraft:salmon",
        "minecraft:pufferfish",
        "minecraft:tropicalfish",
    ] {
        let rigs: Vec<_> = entities
            .rig_bindings
            .iter()
            .enumerate()
            .filter(|(_, rig)| {
                entities.symbols[rig.entity_symbol as usize]
                    .identifier
                    .as_ref()
                    == identifier
            })
            .collect();
        assert!(!rigs.is_empty(), "{identifier} has no compiled rig");
        for (index, rig) in rigs {
            assert_eq!(
                catalog
                    .bindings()
                    .iter()
                    .filter(|binding| binding.rig as usize == index)
                    .count(),
                usize::from(rig.geometry_count),
                "{identifier}: {:?}",
                compiled
                    .report
                    .fallbacks
                    .iter()
                    .filter(|fallback| fallback.rig as usize == index)
                    .collect::<Vec<_>>()
            );
            let layers: Vec<_> = entities
                .render
                .layers
                .iter()
                .filter(|layer| layer.rig as usize == index)
                .collect();
            assert!(!layers.is_empty(), "{identifier} has no render controller");
            for layer in layers {
                for choice in &entities.render.geometries[layer.first_geometry as usize..]
                    [..usize::from(layer.geometry_count)]
                {
                    assert!(
                        assets::neutral_actor_geometry_uvs_are_supported(
                            &entities.geometries,
                            choice.geometry as usize
                        ),
                        "{identifier} selected geometry {} loses artwork",
                        entities.geometries[choice.geometry as usize].identifier
                    );
                }
            }
        }
    }
}

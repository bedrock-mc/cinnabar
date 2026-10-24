//! Controller annotation coverage comes from the pinned pack, independently of runtime routing.
use super::*;

#[test]
fn controller_locator_and_initialization_annotations_are_admitted() {
    let mut bindings = ActorEffectBindings::default();
    bindings.insert_entity(&serde_json::json!({"minecraft:client_entity":{"description":{"identifier":"test:actor","particle_effects":{"wing":"test:effect"}}}}));
    bindings.insert_controllers(&serde_json::json!({"animation_controllers":{"controller.test":{"states":{"default":{"particle_effects":[{"effect":"wing","locator":"left_wing","pre_effect_script":"v.size = query.variant + v.emitter_lifetime;","bind_to_actor":false}]}}}}}));
    let effects = bindings
        .effects_for("test:actor", "controller.test", "default")
        .collect::<Vec<_>>();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].locator, Some("left_wing"));
    assert!(!effects[0].bound);
    assert_eq!(
        effects[0]
            .initialization
            .unwrap()
            .query_names()
            .collect::<Vec<_>>(),
        ["variant"]
    );
    assert_eq!(bindings.unsupported, 0);
}

/// Reads JSON annotations recursively without depending on a handwritten controller inventory.
fn collect_files(directory: &std::path::Path, output: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(&path, output);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            output.push(path);
        }
    }
}

#[test]
fn pinned_controller_annotation_inventory_has_no_dropped_references() {
    let pack = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(assets::vanilla_source().cache_dir.as_ref())
        .join("resource_pack");
    let directory = pack.join("animation_controllers");
    if !directory.is_dir() {
        eprintln!(
            "missing fixture: pinned controller annotations {}",
            directory.display()
        );
        return;
    }
    let mut files = Vec::new();
    collect_files(&directory, &mut files);
    let mut bindings = ActorEffectBindings::default();
    let mut annotations = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(path).unwrap();
        let root: Value = bedrock_packs::jsonc::parse_jsonc(source.as_bytes()).unwrap();
        bindings.insert_controllers(&root);
        for (controller, definition) in root["animation_controllers"]
            .as_object()
            .into_iter()
            .flatten()
        {
            for (state, definition) in definition["states"].as_object().into_iter().flatten() {
                for effect in definition["particle_effects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if effect.get("locator").is_some() || effect.get("pre_effect_script").is_some()
                    {
                        annotations.push((controller.clone(), state.clone(), effect.clone()));
                    }
                }
            }
        }
    }
    assert!(
        !annotations.is_empty(),
        "pinned inventory must contain annotated controller effects"
    );
    for (index, (controller, state, effect)) in annotations.iter().enumerate() {
        let entity = format!("test:annotation_{index}");
        let aliases = BTreeMap::from([(effect["effect"].as_str().unwrap(), "test:effect")]);
        bindings.insert_entity(&serde_json::json!({"minecraft:client_entity":{"description":{"identifier":entity,"particle_effects":aliases}}}));
        let actual = bindings
            .effects_for(&entity, controller, state)
            .find(|binding| {
                binding.locator == effect["locator"].as_str().filter(|name| !name.is_empty())
            });
        assert!(actual.is_some(), "dropped {controller}/{state}: {effect}");
        assert_eq!(
            actual.unwrap().initialization.is_some(),
            effect["pre_effect_script"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
    }
    assert_eq!(bindings.unsupported, 0);
}

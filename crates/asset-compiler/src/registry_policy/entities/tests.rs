use super::*;

/// Uses committed inventories without an installed resource pack.
fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn entity_regeneration_preserves_every_byte() {
    let scratch = tempfile::tempdir().unwrap();
    compile(&root(), scratch.path(), super::super::POLICY).unwrap();
    for path in [
        assets::facts::entity::SOURCE_PATH,
        assets::facts::entity::EXPANSION_PATH,
    ] {
        assert_eq!(
            std::fs::read(scratch.path().join(path)).unwrap(),
            std::fs::read(root().join(path)).unwrap()
        );
    }
}

#[test]
fn native_rules_reject_unknown_empty_and_overlapping_members() {
    let original: serde_json::Value = serde_json::from_slice(super::super::POLICY).unwrap();
    for mutation in 0..6 {
        let mut policy = original.clone();
        let rule = &mut policy["entity_rules"][0];
        match mutation {
            0 => rule["members"][0] = serde_json::json!("minecraft:misspelled_actor"),
            1 => rule["members"] = serde_json::json!([]),
            3 => {
                rule["facts"]["misspelled_field"] = serde_json::json!(true);
            }
            5 => {
                rule["members"].as_array_mut().unwrap().remove(0);
            }
            4 => {
                rule["facts"]["collision"] = serde_json::json!([1.0, 2.0]);
            }
            _ => {
                let mut duplicate = rule.clone();
                duplicate["id"] = serde_json::json!("duplicate");
                policy["entity_rules"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
        }
        let scratch = tempfile::tempdir().unwrap();
        assert!(
            compile(
                &root(),
                scratch.path(),
                &serde_json::to_vec(&policy).unwrap()
            )
            .is_err()
        );
    }
}

#[test]
fn entity_components_regenerate_from_the_pinned_archive() {
    let source = assets::vanilla_source();
    let paths = source.local_paths(&root()).unwrap();
    if !paths.archive.is_file() {
        eprintln!(
            "missing fixture: {}; skipping entity component regeneration",
            paths.archive.display()
        );
        return;
    }
    assert_eq!(
        components_from_archive(&paths.archive).unwrap(),
        std::fs::read(root().join(COMPONENTS_PATH)).unwrap()
    );
}

#[test]
fn an_unpinned_archive_cannot_supply_entity_components() {
    let scratch = tempfile::tempdir().unwrap();
    let path = scratch.path().join("invalid.zip");
    std::fs::write(&path, b"invalid archive").unwrap();
    assert!(
        components_from_archive(&path)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
}

#[test]
fn collidable_defaults_come_from_components_and_keep_conditional_groups_separate() {
    let scratch = tempfile::tempdir().unwrap();
    let entities = scratch.path().join("behavior_pack/entities");
    std::fs::create_dir_all(&entities).unwrap();
    for (name, components, groups) in [
        (
            "solid",
            serde_json::json!({"minecraft:is_collidable": {}}),
            serde_json::json!({}),
        ),
        (
            "conditional",
            serde_json::json!({}),
            serde_json::json!({"active": {"minecraft:is_collidable": {}}}),
        ),
        ("ordinary", serde_json::json!({}), serde_json::json!({})),
    ] {
        let document = serde_json::json!({"minecraft:entity": {
            "description": {"identifier": format!("fixture:{name}")},
            "components": components, "component_groups": groups,
        }});
        std::fs::write(
            entities.join(format!("{name}.json")),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
    }
    let source: ComponentSource =
        serde_json::from_slice(&components(scratch.path()).unwrap()).unwrap();
    assert!(source.entries["fixture:solid"].collidable);
    assert!(!source.entries["fixture:conditional"].collidable);
    assert!(!source.entries["fixture:ordinary"].collidable);
}

#[test]
fn every_admitted_actor_requires_an_explicit_use_resolver_disposition() {
    let mut policy: serde_json::Value = serde_json::from_slice(super::super::POLICY).unwrap();
    let rule = policy["entity_rules"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|rule| rule["facts"].get("actor_use_resolver").is_some())
        .unwrap();
    rule["members"].as_array_mut().unwrap().remove(0);
    let scratch = tempfile::tempdir().unwrap();
    let error = compile(
        &root(),
        scratch.path(),
        &serde_json::to_vec(&policy).unwrap(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("use resolver dispositions"));
}

#[test]
fn a_new_inventory_actor_cannot_silently_inherit_a_use_resolver() {
    let scratch = tempfile::tempdir().unwrap();
    let actors_path = scratch.path().join(ACTORS_PATH);
    std::fs::create_dir_all(actors_path.parent().unwrap()).unwrap();
    let mut actors = std::fs::read_to_string(root().join(ACTORS_PATH)).unwrap();
    actors.push_str("minecraft:fixture_new_actor\n");
    std::fs::write(&actors_path, actors).unwrap();
    let components_path = scratch.path().join(COMPONENTS_PATH);
    std::fs::create_dir_all(components_path.parent().unwrap()).unwrap();
    std::fs::copy(root().join(COMPONENTS_PATH), components_path).unwrap();
    let output = tempfile::tempdir().unwrap();
    let error = compile(scratch.path(), output.path(), super::super::POLICY).unwrap_err();
    assert!(error.to_string().contains("use resolver dispositions"));
}

use super::*;

#[test]
fn legacy_controller_activation_does_not_select_the_same_named_clip_alias() {
    let source = serde_json::json!({
        "format_version":"1.8.0",
        "minecraft:client_entity":{"description":{
            "animations":{"move":"animation.fixture.move"},
            "animation_controllers":[{"move":"controller.animation.fixture.move"}]
        }}
    });
    let roots = activation_roots(&source).unwrap();
    let controllers =
        legacy_controller_aliases(description(&source).unwrap().get("animation_controllers"))
            .unwrap();
    assert_eq!(roots.len(), 1);
    assert_ne!(roots[0].alias, "move");
    assert_eq!(
        controllers.get(roots[0].alias.as_str()).map(AsRef::as_ref),
        Some("controller.animation.fixture.move")
    );
    assert!(roots[0].condition.is_none());
}

#[test]
fn explicit_script_activation_remains_distinct_from_a_legacy_controller_root() {
    let source = serde_json::json!({
        "format_version":"1.10.0",
        "minecraft:client_entity":{"description":{
            "animations":{"move":"animation.fixture.move"},
            "animation_controllers":[{"move":"controller.animation.fixture.move"}],
            "scripts":{"animate":[{"move":"query.is_baby"}]}
        }}
    });
    let roots = activation_roots(&source).unwrap();
    assert_eq!(roots.len(), 2);
    assert_ne!(roots[0].alias, roots[1].alias);
    assert_eq!(roots[1].alias, "move");
    assert_eq!(roots[1].condition.as_deref(), Some("query.is_baby"));
}

#[test]
fn native_legacy_controller_conversion_overwrites_only_the_generated_alias() {
    let generated = legacy_controller_alias("move");
    let mut clips = serde_json::Map::new();
    clips.insert(
        "move".into(),
        Value::String("animation.fixture.move".into()),
    );
    clips.insert(
        generated.to_string(),
        Value::String("animation.fixture.old".into()),
    );
    let source = serde_json::json!({
        "format_version":"1.8.0",
        "minecraft:client_entity":{"description":{
            "animations":clips,
            "animation_controllers":[{"move":"controller.animation.fixture.move"}]
        }}
    });
    let aliases = animation_aliases(description(&source).unwrap()).unwrap();
    assert_eq!(aliases["move"].as_ref(), "animation.fixture.move");
    assert_eq!(
        aliases[generated.as_ref()].as_ref(),
        "controller.animation.fixture.move"
    );
}

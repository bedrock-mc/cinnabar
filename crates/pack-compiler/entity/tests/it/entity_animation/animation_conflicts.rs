//! Conflicting optional animation definitions retain a drawable rest pose.
use super::*;

/// Builds a textured cube whose optional animation resources may conflict.
fn conflicting_body_pack() -> TempDir {
    let pack = animation_pack(false);
    write(pack.path(), "models/entity/test.geo.json", br#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.test","texture_width":16,"texture_height":16},"bones":[{"name":"root","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]},{"name":"arm","parent":"root"}]}]}"#);
    pack
}

/// Checks that conflicts preserve geometry and rendering without choosing an animation winner.
fn assert_static_body(compiled: &pack_entity_compiler::EntityAssetCompilation) {
    assert_eq!(compiled.assets.rig_bindings.len(), 1);
    let rig = compiled.assets.rig_bindings[0];
    assert_eq!(rig.fallback, EntityRigFallback::Skip);
    assert!(rig.render_controller.is_some());
    assert!(rig.geometry_count > 0);
    for geometry in &compiled.assets.rig_geometries {
        assert_eq!(geometry.animation_count, 0);
        assert_eq!(geometry.controller_count, 0);
    }
    assert!(!compiled.assets.render.layers.is_empty());
    assert!(
        compiled
            .assets
            .geometries
            .iter()
            .any(|geometry| { geometry.bones.iter().any(|bone| !bone.cubes.is_empty()) })
    );
    let runtime =
        assets::RuntimeEntityAssets::decode(&encode_entity_blob(&compiled.assets).unwrap())
            .unwrap();
    assert_eq!(runtime.rig_bindings()[0].fallback, EntityRigFallback::Skip);
    assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_entity_compiler::CompileReferenceOutcome::OptionalStaticFallback {
            reason: pack_entity_compiler::FallbackReason::AmbiguousAnimationReference,
            ..
        }
    )));
    assert!(!compiled.reference_outcomes.iter().any(|outcome| matches!(
        outcome,
        pack_entity_compiler::CompileReferenceOutcome::RequiredRigRejected { .. }
    )));
}

#[test]
fn conflicting_clips_keep_the_body_without_selecting_a_duplicate() {
    let pack = conflicting_body_pack();
    write(
        pack.path(),
        "animations/duplicate.animation.json",
        br#"{"format_version":"1.8.0","animations":{"animation.test.walk":{"bones":{"root":{"position":[9,0,0]}}}}}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert_static_body(&compiled);
    let runtime =
        assets::RuntimeEntityAssets::decode(&encode_entity_blob(&compiled.assets).unwrap())
            .unwrap();
    assert_eq!(
        runtime.server_animation_clip(0, "animation.test.walk"),
        None
    );
    assert_eq!(runtime.server_animation_clip(0, "walk"), None);
    assert!(
        runtime
            .server_animation_clip(0, "animation.test.attack")
            .is_some()
    );
}

#[test]
fn conflicting_legacy_controllers_keep_the_body_without_selecting_a_duplicate() {
    let pack = conflicting_body_pack();
    // The controller is reachable only through the legacy controller list.
    write(
        pack.path(),
        "entity/test.entity.json",
        br#"{"format_version":"1.8.0","minecraft:client_entity":{"description":{"identifier":"minecraft:test","textures":{"default":"textures/entity/test"},"geometry":{"default":"geometry.test"},"animations":{"walk":"animation.test.walk","attack":"animation.test.attack"},"animation_controllers":[{"main":"controller.animation.test"}],"render_controllers":["controller.render.test"]}}}"#,
    );
    write(
        pack.path(),
        "animation_controllers/other.animation_controllers.json",
        br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.test":{"initial_state":"default","states":{"default":{"animations":["animation.test.attack"]}}}}}"#,
    );
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert_static_body(&compiled);
}

/// A modern root does not escape the same conflict handling used by legacy controllers.
#[test]
fn conflicting_modern_controllers_retain_scripts_and_all_geometry_candidates() {
    let pack =
        selectable_geometry_pack("query.is_baby", &["Geometry.default", "Geometry.alternate"]);
    let path = pack.path().join("entity/test.entity.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["minecraft:client_entity"]["description"]["animations"]["main"] =
        serde_json::json!("controller.animation.test");
    value["minecraft:client_entity"]["description"]["scripts"] = serde_json::json!({
        "animate": ["main"], "scale": "query.is_baby ? 6.25 : 1.0", "pre_animation": ["variable.body_visible = 1;"]
    });
    write(
        pack.path(),
        "entity/test.entity.json",
        &serde_json::to_vec(&value).unwrap(),
    );
    write(pack.path(), "animation_controllers/duplicate.json", br#"{"format_version":"1.10.0","animation_controllers":{"controller.animation.test":{"states":{"different":{"animations":["walk"]}}}}}"#);
    let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.assets.rig_bindings.len(), 1);
    let rig = compiled.assets.rig_bindings[0];
    assert_eq!(rig.fallback, EntityRigFallback::Skip);
    assert!(rig.scale_expressions.is_some());
    assert!(rig.pre_animation.is_some());
    assert!(rig.geometry_count > 1);
    for geometry in &compiled.assets.rig_geometries {
        assert_eq!(geometry.animation_count, 0);
        assert_eq!(geometry.controller_count, 0);
    }
}

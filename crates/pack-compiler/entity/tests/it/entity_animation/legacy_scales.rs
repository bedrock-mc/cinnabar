use super::*;

/// Compiles the same original rig using modern or legacy axis spellings.
fn scaled(legacy: bool) -> assets::CompiledEntityAssets {
    let pack = animation_pack(false);
    let path = pack.path().join("entity/test.entity.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut scripts = serde_json::json!({"pre_animation": "variable.size = 2;"});
    for (name, expression) in [
        ("scaleX", "variable.size"),
        ("scaleY", "3"),
        ("scaleZ", "4"),
    ] {
        let key = if legacy {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        };
        scripts[key] = expression.into();
    }
    value["minecraft:client_entity"]["description"]["scripts"] = scripts;
    compile_entity_assets_after_write(pack.path(), &path, &value)
}

#[test]
fn legacy_axis_scales_keep_the_same_programs_as_modern_spellings() {
    let modern = scaled(false);
    let legacy = scaled(true);
    let modern_scale = modern.rig_bindings[0]
        .scale_expressions
        .expect("axis scale programs");
    assert_eq!(legacy.rig_bindings[0].scale_expressions, Some(modern_scale));
    assert_eq!(legacy.rig_bindings[0].scale, modern.rig_bindings[0].scale);
    assert_eq!(legacy.molang_expressions, modern.molang_expressions);
    assert_eq!(legacy.molang_ops, modern.molang_ops);
    let runtime =
        assets::RuntimeEntityAssets::decode(&encode_entity_blob(&legacy).unwrap()).unwrap();
    assert_eq!(
        runtime.rig_bindings()[0].scale_expressions,
        Some(modern_scale)
    );
}

#[test]
fn malformed_legacy_axis_scale_keeps_the_rig_and_reports_the_fallback() {
    for scale in [serde_json::json!({}), serde_json::json!("@invalid")] {
        let pack = animation_pack(false);
        let path = pack.path().join("entity/test.entity.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["minecraft:client_entity"]["description"]["scripts"] =
            serde_json::json!({"scalex": scale, "scaley": 3, "scalez": 4});
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
        assert_eq!(compiled.assets.rig_bindings.len(), 1);
        assert_eq!(compiled.assets.rig_bindings[0].scale_expressions, None);
        assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
            outcome,
            pack_entity_compiler::CompileReferenceOutcome::OptionalStaticFallback { .. }
        )));
    }
}

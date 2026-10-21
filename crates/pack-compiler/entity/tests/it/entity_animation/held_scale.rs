use super::*;

#[test]
fn held_item_scale_numbers_and_expressions_survive_carrier_roundtrip() {
    for scale in [
        serde_json::json!(2.678),
        serde_json::json!("2.678"),
        serde_json::json!("1 + query.frame_alpha"),
    ] {
        let pack = animation_pack(false);
        let path = pack.path().join("entity/test.entity.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["minecraft:client_entity"]["description"]["held_item_scale"] = scale;
        let compiled = compile_entity_assets_after_write(pack.path(), &path, &value);
        let expression = compiled.rig_bindings[0]
            .held_item_scale
            .expect("authored scale retained");
        let runtime =
            assets::RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap();
        assert_eq!(runtime.rig_bindings()[0].held_item_scale, Some(expression));
        assert_eq!(
            runtime.molang_expressions(),
            compiled.molang_expressions.as_ref()
        );
        let mut invalid = compiled;
        invalid.rig_bindings[0].held_item_scale = Some(invalid.molang_expressions.len() as u32);
        assert!(
            invalid.validate().is_err(),
            "out-of-range programs must be rejected"
        );
    }
}

#[test]
fn omitted_and_malformed_held_item_scales_preserve_owner_rigs() {
    let pack = animation_pack(false);
    let path = pack.path().join("entity/test.entity.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
    assert_eq!(compiled.rig_bindings[0].held_item_scale, None);
    let encoded = serde_json::to_value(compiled.rig_bindings[0]).unwrap();
    assert!(encoded.get("held_item_scale").is_none());
    let old: assets::EntityRigBinding = serde_json::from_value(encoded).unwrap();
    assert_eq!(old.held_item_scale, None);
    for malformed in [
        serde_json::json!(true),
        serde_json::json!({}),
        serde_json::json!("@invalid"),
    ] {
        value["minecraft:client_entity"]["description"]["held_item_scale"] = malformed;
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let compiled = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
        assert_eq!(compiled.assets.rig_bindings.len(), 1);
        assert_eq!(compiled.assets.rig_bindings[0].held_item_scale, None);
        assert!(compiled.reference_outcomes.iter().any(|outcome| matches!(
            outcome,
            pack_entity_compiler::CompileReferenceOutcome::OptionalStaticFallback { .. }
        )));
    }
}

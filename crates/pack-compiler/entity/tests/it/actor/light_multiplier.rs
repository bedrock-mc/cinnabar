use super::*;

#[test]
fn controller_light_multiplier_retains_authored_constant_and_query_expressions() {
    let pack = pack(0, "entity_alphatest", false);
    let path = pack.path().join("render_controllers/example.json");
    let mut source: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for value in [
        serde_json::json!(0.5),
        serde_json::json!("query.is_powered ? 0.5 : 1.8"),
    ] {
        source["render_controllers"]["controller.render.example"]["light_color_multiplier"] = value;
        fs::write(&path, serde_json::to_vec(&source).unwrap()).unwrap();
        let compiled = compile_entity_assets(pack.path(), MANIFEST).unwrap();
        let expression = compiled.render.layers[0].light_color_multiplier.unwrap();
        let expression = &compiled.molang_expressions[expression as usize];
        let first = expression.first_op as usize;
        let ops = &compiled.molang_ops[first..first + usize::from(expression.op_count)];
        assert!(
            ops.iter()
                .any(|op| matches!(op, assets::MolangOp::Push(value) if value.get() == 0.5))
        );
        let encoded = encode_entity_blob(&compiled).unwrap();
        let restored = assets::RuntimeEntityAssets::decode(&encoded).unwrap();
        assert_eq!(restored.render_data().layers[0], compiled.render.layers[0]);
    }
}

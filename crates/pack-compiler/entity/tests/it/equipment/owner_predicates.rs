//! Bad owner restrictions reject only their own definitions.
use super::*;
use pack_entity_compiler::{CompileReferenceOutcome, FallbackReason};

#[test]
fn non_string_owner_predicates_never_become_unconditional() {
    for predicate in [
        serde_json::json!(false),
        serde_json::json!(0),
        serde_json::json!(true),
        serde_json::json!(1.5),
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!({}),
    ] {
        for render_controller in ["controller.render.item", "controller.render.external"] {
            let pack = equipment_pack();
            let definition = serde_json::json!({"format_version":"1.10.0","minecraft:attachable":{"description":{
                "identifier":"fixture:restricted", "item":{"minecraft:diamond_helmet":predicate},
                "materials":{"default":"armor"}, "textures":{"default":"textures/models/armor/diamond_1"},
                "geometry":{"default":"geometry.item"}, "render_controllers":[render_controller]
            }}});
            write(
                pack.path(),
                "attachables/restricted.json",
                &serde_json::to_vec(&definition).unwrap(),
            );
            let compilation = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
            let compiled = &compilation.assets;
            let rig = compiled
                .rig_bindings
                .iter()
                .find(|rig| {
                    compiled.symbols[rig.entity_symbol as usize]
                        .identifier
                        .as_ref()
                        == "fixture:restricted"
                })
                .unwrap();
            let condition = rig.item_condition.unwrap_or_else(|| {
                panic!("explicit predicate {predicate} must not become unconditional")
            });
            assert_eq!(
                crate::entity_animation::evaluate_selection_expression(compiled, condition, 0.0),
                0.0,
                "unsupported predicate {predicate} must reject the owner"
            );
            assert!(
                compilation
                    .reference_outcomes
                    .iter()
                    .any(|outcome| matches!(
                        outcome,
                        CompileReferenceOutcome::OptionalStaticFallback {
                            symbol,
                            reason: FallbackReason::UnsupportedOptionalExpression,
                            ..
                        } if *symbol == rig.entity_symbol
                    ))
            );
            assert!(compiled.rig_bindings.iter().any(|rig| {
                compiled.symbols[rig.entity_symbol as usize]
                    .identifier
                    .as_ref()
                    == "minecraft:diamond_helmet"
                    && rig.item_condition.is_none()
            }));
            let encoded = encode_entity_blob(compiled).unwrap();
            assets::RuntimeEntityAssets::decode(&encoded).unwrap();
        }
    }
}

#[test]
fn two_unreadable_owner_predicates_keep_unrelated_entities_and_equipment() {
    let pack = equipment_pack();
    for (name, predicate) in [
        ("first", "query.owner_identifier =="),
        ("second", "query.is_using_item +"),
    ] {
        let definition = serde_json::json!({"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":format!("fixture:{name}"), "item":{"minecraft:diamond_helmet":predicate},
            "materials":{"default":"armor"}, "textures":{"default":"textures/models/armor/diamond_1"},
            "geometry":{"default":"geometry.item"}, "render_controllers":["controller.render.item"]
        }}});
        write(
            pack.path(),
            &format!("attachables/{name}.json"),
            &serde_json::to_vec(&definition).unwrap(),
        );
    }
    let compilation = compile_entity_assets_with_report(pack.path(), MANIFEST).unwrap();
    assert_eq!(compilation.equipment_bindings.len(), 5);
    assert!(compilation.assets.rig_bindings.iter().any(|rig| {
        compilation.assets.symbols[rig.entity_symbol as usize]
            .identifier
            .as_ref()
            == "minecraft:item"
    }));
    assert_eq!(
        compilation
            .reference_outcomes
            .iter()
            .filter(|outcome| matches!(
                outcome,
                CompileReferenceOutcome::OptionalStaticFallback {
                    reason: FallbackReason::UnsupportedOptionalExpression,
                    ..
                }
            ))
            .count(),
        2
    );
    let encoded = encode_entity_blob(&compilation.assets).unwrap();
    assets::RuntimeEntityAssets::decode(&encoded).unwrap();
}

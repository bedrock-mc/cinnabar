//! Composition keeps the equipment decision made before predicate caches become warm.
use super::*;

/// Adds distinct rejected mappings that consume the complete cold selection budget together.
fn budget_files() -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = variant_files(true, Some("1"));
    let rejected: serde_json::Value = serde_json::from_slice(
        &files
            .iter()
            .find(|(path, _)| path.as_ref() == "attachables/wings.json")
            .unwrap()
            .1,
    )
    .unwrap();
    let mut description = rejected["minecraft:attachable"]["description"].clone();
    let cost = assets::MAX_MOLANG_OPS_PER_EXPRESSION;
    let count = actor_animation::MAX_MOLANG_OPS_PER_ACTOR_TICK / cost;
    for index in 0..count {
        description["identifier"] = serde_json::json!(format!("zfixture:rejected_{index}"));
        description["item"] =
            serde_json::json!({(CHEST): crate::selection_test_support::rejected_predicate(index)});
        files.push((
            format!("attachables/rejected_{index}.json").into(),
            serde_json::to_vec(&serde_json::json!({"format_version":"1.10.0","minecraft:attachable":{"description":description}})).unwrap(),
        ));
    }
    files
}

#[test]
fn cape_composition_reuses_the_drawn_chest_selection_with_cold_and_warm_predicates() {
    for from_pack in [false, true] {
        for local in [false, true] {
            let (entities, catalog) = compile_fixture(budget_files());
            let (mut runtime, mut pages) =
                build_runtime(&entities, (!from_pack).then(|| Arc::clone(&catalog)));
            if from_pack {
                install(&mut runtime, &mut pages, Arc::clone(&entities), catalog);
            }
            let mut state = ActorFrameState::default();
            let mut stream = equipped_stream(Arc::clone(&entities), &mut state);
            stream.advance_actor_interpolation_ticks(1);
            let input = equipment::remote_input(&stream, 2);
            for wings in [false, true, true] {
                let batch = publish(
                    &stream,
                    &entities,
                    &mut runtime,
                    &pages,
                    &mut state,
                    &input,
                    local,
                );
                let geometry = if wings {
                    assets::ELYTRA_GEOMETRY_IDENTIFIER
                } else {
                    ARMOR_GEOMETRY
                };
                let index = render_model::find_geometry_index(&entities, geometry).unwrap();
                let expected = if from_pack {
                    render_model::pack_equipment_rig_id(index)
                } else {
                    render_model::equipment_rig_id(index)
                };
                assert_category(&batch, Some(expected), wings);
            }
        }
    }
}

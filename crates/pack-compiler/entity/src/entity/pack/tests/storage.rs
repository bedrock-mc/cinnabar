use super::*;

/// Includes sparse defaults, tagged UVs and optional fields that the temporary format must retain.
fn stored_pack() -> Vec<(Box<str>, Vec<u8>)> {
    vec![
        file(
            "entity/fixture.json",
            &json!({
                "format_version":"1.10.0", "minecraft:client_entity":{"description":{
                    "identifier":"fixture:stored", "geometry":{"default":"geometry.stored"}
                }}
            })
            .to_string(),
        ),
        file(
            "models/entity/fixture.json",
            &json!({
                "format_version":"1.21.0", "minecraft:geometry":[{
                    "description":{"identifier":"geometry.stored", "texture_width":32,
                        "texture_height":16, "visible_bounds_width":3, "visible_bounds_height":4,
                        "visible_bounds_offset":[0,1,0]},
                    "bones":[{"name":"empty"}, {
                        "name":"root", "pivot":[1,2,3], "rotation":[0,45,0],
                        "binding":"query.item_slot_to_bone_name(context.item_slot)",
                        "locators":{"held":[1,2,3]},
                        "texture_meshes":[{"texture":"default", "local_pivot":[0,1,0],
                            "position":[1,0,0], "rotation":[0,0,0]}],
                        "cubes":[{"origin":[0,0,0], "size":[1,2,3], "uv":[0,0]},
                            {"origin":[1,1,1], "size":[2,3,4],
                                "uv":{"north":{"uv":[2,3], "uv_size":[4,5]}}}]
                    }]
                }]
            })
            .to_string(),
        ),
    ]
}

/// Runs an attempt through the same admission and assembly path as isolation.
fn attempt(selected: &mut [PreparedFile], store: &mut SourceStore) -> EntityPackCompilation {
    compile_selected(
        selected,
        None,
        &mut true,
        SourceBudget::default(),
        &|| false,
        store,
    )
    .unwrap()
    .unwrap()
    .unwrap()
}

#[test]
fn stored_products_preserve_compiled_results_without_reparsing() {
    let mut selected = select_sources(stored_pack())
        .into_iter()
        .map(PreparedFile::new)
        .collect::<Vec<_>>();
    let mut store = SourceStore::default();
    SOURCE_PARSES.set(0);
    let first = attempt(&mut selected, &mut store);
    let second = attempt(&mut selected, &mut store);
    assert_eq!(SOURCE_PARSES.get(), selected.len());
    assert_eq!(first.assets, second.assets);
    assert_eq!(first.payloads, second.payloads);
    assert_eq!(first.reference_outcomes, second.reference_outcomes);
    assert_eq!(first.equipment_bindings, second.equipment_bindings);
    assert_eq!(first.skipped, second.skipped);
    assert_eq!(first.assets.geometries[0].bones[1].cubes.len(), 2);
}

#[test]
fn unavailable_temporary_storage_preserves_results() {
    let expected = compile_entity_pack(stored_pack()).unwrap().unwrap();
    let mut selected = select_sources(stored_pack())
        .into_iter()
        .map(PreparedFile::new)
        .collect::<Vec<_>>();
    let mut store = SourceStore::unavailable();
    for _ in 0..2 {
        let compiled = attempt(&mut selected, &mut store);
        assert_eq!(expected.assets, compiled.assets);
        assert_eq!(expected.payloads, compiled.payloads);
        assert_eq!(expected.reference_outcomes, compiled.reference_outcomes);
        assert_eq!(expected.equipment_bindings, compiled.equipment_bindings);
        assert_eq!(expected.skipped, compiled.skipped);
    }
}

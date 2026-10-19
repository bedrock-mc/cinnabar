use super::*;

/// Returns two valid definitions around a malformed source that must stay unparsed when full.
fn budgeted_definitions() -> Vec<(Box<str>, Vec<u8>)> {
    let entity = |identifier: &str| {
        json!({
            "format_version":"1.10.0", "minecraft:client_entity":{"description":{
                "identifier":identifier
            }}
        })
        .to_string()
    };
    vec![
        file("entity/a.json", &entity("fixture:retained")),
        file("entity/b.json", "{invalid"),
        file("entity/c.json", &entity("fixture:unadmitted")),
    ]
}

#[test]
fn count_budget_rejects_sources_before_preparation() {
    SOURCE_PARSES.set(0);
    let compiled = compile_with_budget(
        budgeted_definitions(),
        &|| false,
        SourceBudget {
            count: 1,
            ..SourceBudget::default()
        },
    )
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(compiled.skipped.over_budget, 2);
    assert_eq!(compiled.skipped.unparsable, 0);
    assert_eq!(SOURCE_PARSES.get(), 1);
}

#[test]
fn byte_budget_rejects_sources_before_preparation() {
    let files = budgeted_definitions();
    let bytes = files[0].1.len();
    SOURCE_PARSES.set(0);
    let compiled = compile_with_budget(
        files,
        &|| false,
        SourceBudget {
            bytes,
            ..SourceBudget::default()
        },
    )
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(compiled.skipped.over_budget, 2);
    assert_eq!(compiled.skipped.unparsable, 0);
    assert_eq!(SOURCE_PARSES.get(), 1);
}

#[test]
fn isolation_preserves_recovery_through_byte_budget_displacement() {
    let mut files = budgeted_definitions();
    files.remove(1);
    let per_source = assets::MAX_ENTITY_GEOMETRIES / 2 + 1;
    let geometry_files = (0..3)
        .map(|source| {
            let entries = (0..per_source)
                .map(|index| {
                    json!({
                        "description":{"identifier":format!("geometry.fixture_{source}_{index}")},
                        "bones":[]
                    })
                })
                .collect::<Vec<_>>();
            file(
                &format!("models/entity/{source}.json"),
                &json!({
                    "format_version":"1.12.0", "minecraft:geometry":entries
                })
                .to_string(),
            )
        })
        .collect::<Vec<_>>();
    let geometry_bytes: usize = geometry_files.iter().map(|(_, bytes)| bytes.len()).sum();
    let small_file_bytes = geometry_files
        .iter()
        .map(|(_, bytes)| bytes.len())
        .min()
        .unwrap();
    files[0].1.resize(small_file_bytes, b' ');
    let budget = SourceBudget {
        bytes: files.iter().map(|(_, bytes)| bytes.len()).sum::<usize>() + geometry_bytes,
        ..SourceBudget::default()
    };
    let mut material = file("materials/displacing.material", r#"{"materials":{}}"#);
    material
        .1
        .resize(geometry_bytes + small_file_bytes / 2, b' ');
    files.push(material);
    files.extend(geometry_files);
    ASSEMBLY_ATTEMPTS.set(0);
    SOURCE_PARSES.set(0);
    let selected_count = files.len();
    let compiled = compile_with_budget(files, &|| false, budget)
        .unwrap()
        .expect("omitting the first entity must admit the material and displace the geometries")
        .unwrap();
    assert_eq!(compiled.skipped.isolated, 1);
    assert_eq!(compiled.skipped.over_budget, 3);
    assert!(compiled.assets.geometries.is_empty());
    assert!(
        compiled
            .payloads
            .contains_key("materials/displacing.material")
    );
    assert!(!compiled.payloads.contains_key("entity/a.json"));
    assert_eq!(ASSEMBLY_ATTEMPTS.get(), 2);
    assert_eq!(SOURCE_PARSES.get(), selected_count);
}

#[test]
fn geometry_uses_normalized_bytes_for_admission() {
    let mut files = budgeted_definitions();
    files.truncate(1);
    let geometry = json!({"format_version":"1.12.0", "minecraft:geometry":[{
        "description":{"identifier":"geometry.fixture"}, "bones":[]
    }]});
    let normalized = serde_json::to_vec(&geometry).unwrap();
    let budget = SourceBudget {
        bytes: files[0].1.len() + normalized.len(),
        ..SourceBudget::default()
    };
    let mut authored = geometry;
    authored["discarded"] = json!("padding".repeat(256));
    files.push(file("models/entity/fixture.json", &authored.to_string()));
    let compiled = compile_with_budget(files, &|| false, budget)
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(compiled.skipped, EntityPackSkips::default());
    assert_eq!(compiled.assets.geometries.len(), 1);
    assert_eq!(
        compiled.payloads["models/entity/fixture.json"].as_ref(),
        normalized
    );
}

#[test]
fn malformed_geometry_is_rejected_before_admission_budgets() {
    let mut files = budgeted_definitions();
    files.truncate(1);
    files.push(file("models/entity/bad.json", "{invalid"));
    SOURCE_PARSES.set(0);
    let compiled = compile_with_budget(
        files,
        &|| false,
        SourceBudget {
            count: 1,
            ..SourceBudget::default()
        },
    )
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(compiled.skipped.unparsable, 1);
    assert_eq!(compiled.skipped.over_budget, 0);
    assert_eq!(SOURCE_PARSES.get(), 1);
}

#[test]
fn cancellation_stops_between_source_batches() {
    SOURCE_PARSES.set(0);
    let result = compile_entity_pack_unless(budgeted_definitions(), &|| SOURCE_PARSES.get() >= 1);
    assert!(result.is_none());
    assert!((1..=rayon::current_num_threads()).contains(&SOURCE_PARSES.get()));
}

#[test]
fn oversized_sources_are_rejected_before_admission_budgets() {
    let mut files = budgeted_definitions();
    files[1].1.resize(assets::MAX_ENTITY_SOURCE_BYTES + 1, b' ');
    SOURCE_PARSES.set(0);
    let compiled = compile_with_budget(
        files,
        &|| false,
        SourceBudget {
            count: 1,
            ..SourceBudget::default()
        },
    )
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(compiled.skipped.oversized, 1);
    assert_eq!(compiled.skipped.over_budget, 1);
    assert_eq!(compiled.skipped.unparsable, 0);
    assert_eq!(SOURCE_PARSES.get(), 1);
}

#[test]
fn byte_budget_rejected_geometry_keeps_only_lengths() {
    let mut files = budgeted_definitions();
    files.truncate(1);
    let budget = SourceBudget {
        bytes: files[0].1.len(),
        ..SourceBudget::default()
    };
    for index in 0..32 {
        files.push(file(&format!("models/entity/{index}.json"), &json!({
            "format_version":"1.12.0", "minecraft:geometry":[{
                "description":{"identifier":format!("geometry.fixture_{index}")},
                "bones":(0..128).map(|bone| json!({"name":format!("bone_{bone}")})).collect::<Vec<_>>()
            }]
        }).to_string()));
    }
    let mut selected = select_sources(files)
        .into_iter()
        .map(PreparedFile::new)
        .collect::<Vec<_>>();
    let mut can_isolate = true;
    SOURCE_PARSES.set(0);
    let compiled = compile_selected(&mut selected, None, &mut can_isolate, budget, &|| false)
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(compiled.skipped.over_budget, 32);
    assert_eq!(SOURCE_PARSES.get(), 1);
    assert!(selected[0].products.as_ref().unwrap().is_some());
    for source in &selected[1..] {
        assert!(source.length.flatten().is_some());
        assert!(source.products.is_none());
    }
}

#[test]
fn rejected_geometry_is_prepared_once_when_a_retry_admits_it() {
    let mut files = budgeted_definitions();
    files.truncate(1);
    let geometry = file(
        "models/entity/retry.json",
        &json!({
            "format_version":"1.12.0", "minecraft:geometry":[{
                "description":{"identifier":"geometry.retry"},
                "bones":[{"name":"root", "cubes":[{"origin":[0,0,0], "size":[1,2,3], "uv":[0,0]}]}]
            }]
        })
        .to_string(),
    );
    files[0].1.resize(geometry.1.len() * 2, b' ');
    let budget = SourceBudget {
        bytes: files[0].1.len(),
        ..SourceBudget::default()
    };
    files.push(geometry);
    let mut selected = select_sources(files)
        .into_iter()
        .map(PreparedFile::new)
        .collect::<Vec<_>>();
    SOURCE_PARSES.set(0);
    let compiled = compile_selected(&mut selected, None, &mut true, budget, &|| false)
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(compiled.skipped.over_budget, 1);
    assert!(selected[1].products.is_none());
    assert_eq!(SOURCE_PARSES.get(), 1);
    for _ in 0..2 {
        // Geometry alone is not retained as a usable pack, but it has been admitted and parsed.
        assert!(
            compile_selected(&mut selected, Some(0), &mut true, budget, &|| false)
                .unwrap()
                .unwrap()
                .is_none()
        );
        let parsed = selected[1].products.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(
            parsed.geometries.values().next().unwrap().bones[0]
                .cubes
                .len(),
            1
        );
    }
    assert_eq!(SOURCE_PARSES.get(), 2);
}

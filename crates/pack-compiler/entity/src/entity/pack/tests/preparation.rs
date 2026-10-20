use super::*;

#[test]
fn large_geometry_catalog_preserves_entities_and_every_geometry() {
    for counts in [vec![4_097], vec![2_049, 2_049]] {
        let mut files = vec![file(
            "entity/fixture.json",
            &json!({"format_version":"1.10.0", "minecraft:client_entity":{"description":{
                "identifier":"fixture:actor", "geometry":{"default":"geometry.fixture_0_0"}
            }}})
            .to_string(),
        )];
        for (source, count) in counts.iter().enumerate() {
            let geometries = (0..*count)
                .map(|index| {
                    json!({
                        "description":{"identifier":format!("geometry.fixture_{source}_{index}")},
                        "bones":[]
                    })
                })
                .collect::<Vec<_>>();
            files.push(file(
                &format!("models/entity/{source}.json"),
                &json!({"format_version":"1.12.0", "minecraft:geometry":geometries}).to_string(),
            ));
        }
        let compiled = compile_entity_pack(files).unwrap().unwrap();
        assert_eq!(compiled.skipped, EntityPackSkips::default());
        assert_eq!(
            compiled.assets.geometries.len(),
            counts.iter().sum::<usize>()
        );
        assert!(compiled.assets.symbols.iter().any(|symbol| {
            symbol.kind == EntityAssetKind::Entity && symbol.identifier.as_ref() == "fixture:actor"
        }));
        assert!(compiled.payloads.contains_key("entity/fixture.json"));
    }
}

/// Creates a cycle that cannot be isolated within the entity-first attempt budget.
fn cyclic_pack() -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = (0..65)
        .map(|index| {
            let document = json!({"format_version":"1.10.0", "minecraft:client_entity":{"description":{
                "identifier":format!("fixture:item_{index}"), "geometry":{"default":"geometry.a"}
            }}});
            file(&format!("entity/item_{index:02}.json"), &document.to_string())
        })
        .collect::<Vec<_>>();
    files.push(file(
        "models/entity/cycle.json",
        &json!({"format_version":"1.10.0",
            "geometry.a:geometry.b":{"bones":[]}, "geometry.b:geometry.a":{"bones":[]}
        })
        .to_string(),
    ));
    files
}

#[test]
fn isolation_prepares_each_source_once() {
    let files = cyclic_pack();
    let expected = files.len();
    SOURCE_PARSES.set(0);
    ASSEMBLY_ATTEMPTS.set(0);
    let error = compile_entity_pack(files).unwrap_err().to_string();
    assert!(error.contains("cycle"), "{error}");
    assert_eq!(ASSEMBLY_ATTEMPTS.get(), 65);
    assert_eq!(SOURCE_PARSES.get(), expected);
}

#[test]
fn impossible_aggregate_count_skips_isolation() {
    let per_source = assets::MAX_ENTITY_ASSET_SYMBOLS / 2 + 1;
    let mut files = cyclic_pack();
    files.pop();
    // No source alone can remove enough symbols to fit the aggregate bound.
    for source in 0..3 {
        let controllers = (0..per_source)
            .map(|index| {
                (
                    format!("controller.render.fixture_{source}_{index}"),
                    json!({}),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        files.push(file(
            &format!("render_controllers/{source}.json"),
            &json!({"format_version":"1.8.0", "render_controllers":controllers}).to_string(),
        ));
    }
    ASSEMBLY_ATTEMPTS.set(0);
    let error = compile_entity_pack(files).unwrap_err().to_string();
    assert!(error.contains("symbol count exceeds bound"), "{error}");
    assert_eq!(ASSEMBLY_ATTEMPTS.get(), 1);
}

#[test]
fn isolation_can_drop_the_single_file_that_exceeds_the_count() {
    let mut files = cyclic_pack();
    files.truncate(1);
    let controllers = (0..assets::MAX_ENTITY_ASSET_SYMBOLS)
        .map(|index| (format!("controller.render.fixture_{index}"), json!({})))
        .collect::<serde_json::Map<_, _>>();
    files.push(file(
        "render_controllers/excess.json",
        &json!({"format_version":"1.8.0",
            "render_controllers":controllers
        })
        .to_string(),
    ));
    files.push(file(
        "models/entity/retained.json",
        &json!({"format_version":"1.12.0",
            "minecraft:geometry":[{"description":{"identifier":"geometry.retained"}, "bones":[]}]
        })
        .to_string(),
    ));
    ASSEMBLY_ATTEMPTS.set(0);
    let compiled = compile_entity_pack(files).unwrap().unwrap();
    assert_eq!(compiled.skipped.isolated, 1);
    assert_eq!(compiled.assets.geometries.len(), 1);
    assert_eq!(ASSEMBLY_ATTEMPTS.get(), 4);
}

#[test]
fn cancellation_stops_before_the_next_isolation_attempt() {
    ASSEMBLY_ATTEMPTS.set(0);
    let result = crate::compile_actor_pack_unless(cyclic_pack(), &|| ASSEMBLY_ATTEMPTS.get() >= 2);
    assert!(result.is_none());
    assert_eq!(ASSEMBLY_ATTEMPTS.get(), 2);
}

#[test]
fn cancellation_stops_before_preparing_sources() {
    SOURCE_PARSES.set(0);
    let result = compile_entity_pack_unless(cyclic_pack(), &|| true);
    assert!(result.is_none());
    assert_eq!(SOURCE_PARSES.get(), 0);
}

#[test]
fn isolation_caches_invalid_admitted_sources() {
    let mut files = cyclic_pack();
    files.push(file("entity/invalid.json", "{invalid"));
    let expected = files.len();
    SOURCE_PARSES.set(0);
    let error = compile_entity_pack(files).unwrap_err().to_string();
    assert!(error.contains("cycle"), "{error}");
    assert_eq!(SOURCE_PARSES.get(), expected);
}

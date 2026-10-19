use super::*;

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
    let count = assets::MAX_ENTITY_GEOMETRIES + 2;
    let mut files = cyclic_pack();
    files.pop();
    // No geometry source alone can remove the two excess entries.
    for index in 0..count {
        files.push(file(
            &format!("models/entity/{index}.json"),
            &json!({
                "format_version":"1.12.0", "minecraft:geometry":[{
                    "description":{"identifier":format!("geometry.fixture_{index}")}, "bones":[]
                }]
            })
            .to_string(),
        ));
    }
    ASSEMBLY_ATTEMPTS.set(0);
    let error = compile_entity_pack(files).unwrap_err().to_string();
    assert!(error.contains("geometry count exceeds bound"), "{error}");
    assert_eq!(ASSEMBLY_ATTEMPTS.get(), 1);
}

#[test]
fn isolation_can_drop_the_single_file_that_exceeds_the_count() {
    let mut files = cyclic_pack();
    files.truncate(1);
    let geometries = (0..assets::MAX_ENTITY_GEOMETRIES)
        .map(|index| {
            json!({
                "description":{"identifier":format!("geometry.fixture_{index}")}, "bones":[]
            })
        })
        .collect::<Vec<_>>();
    files.push(file(
        "models/entity/excess.json",
        &json!({"format_version":"1.12.0",
            "minecraft:geometry":geometries
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
    assert_eq!(ASSEMBLY_ATTEMPTS.get(), 3);
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

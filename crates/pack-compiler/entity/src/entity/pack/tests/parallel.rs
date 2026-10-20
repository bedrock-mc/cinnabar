use super::*;

/// Builds independent definitions with shared animation and render sources.
fn many_files(count: usize) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = Vec::new();
    for index in 0..count {
        files.push(file(
            &format!("entity/model_{index:03}.json"),
            &json!({
                "format_version":"1.10.0", "minecraft:client_entity":{"description":{
                    "identifier":format!("fixture:model_{index}"),
                    "geometry":{"default":format!("geometry.fixture_{index}")},
                    "animations":{"idle":"animation.fixture.idle"},
                    "scripts":{"animate":["idle"]},
                    "render_controllers":["controller.render.fixture"]
                }}
            })
            .to_string(),
        ));
        files.push(file(
            &format!("models/entity/model_{index:03}.json"),
            &json!({
                "format_version":"1.12.0", "minecraft:geometry":[{
                    "description":{"identifier":format!("geometry.fixture_{index}")},
                    "bones":[{"name":"root"}]
                }]
            })
            .to_string(),
        ));
    }
    files.push(file(
        "animations/shared.json",
        &json!({
            "format_version":"1.8.0", "animations":{
                "animation.fixture.idle":{"loop":true,"bones":{"root":{"rotation":[0,10,0]}}}
            }
        })
        .to_string(),
    ));
    files.push(file(
        "render_controllers/shared.json",
        &json!({
            "format_version":"1.8.0", "render_controllers":{
                "controller.render.fixture":{"geometry":"Geometry.default"}
            }
        })
        .to_string(),
    ));
    files
}

#[test]
fn assembly_parses_each_document_once() {
    DOCUMENT_PARSES.with_borrow_mut(BTreeMap::clear);
    let compiled = compile_entity_pack(many_files(4)).unwrap().unwrap();
    assert_eq!(compiled.assets.geometries.len(), 4);
    assert_eq!(compiled.assets.rig_bindings.len(), 4);
    DOCUMENT_PARSES.with_borrow(|counts| {
        assert_eq!(counts.len(), 6);
        for (path, count) in counts {
            assert_eq!(*count, 1, "assembly parsed {path} repeatedly");
        }
    });
}

#[test]
fn many_file_output_is_identical_on_one_and_four_threads() {
    let compile = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let compiled = compile_entity_pack(many_files(64)).unwrap().unwrap();
                assert_eq!(compiled.assets.geometries.len(), 64);
                (
                    assets::encode_entity_blob(&compiled.assets).unwrap(),
                    compiled.skipped,
                    compiled.payloads,
                )
            })
    };
    assert_eq!(compile(1), compile(4));
}

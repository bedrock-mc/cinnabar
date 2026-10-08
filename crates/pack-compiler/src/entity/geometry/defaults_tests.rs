use super::*;

fn parse(value: &Value) -> Box<[EntityGeometryCube]> {
    parse_geometry_cubes(value, Path::new("synthetic.geo.json"), false, zero_scalar())
        .expect("bounded cube geometry")
}

#[test]
fn cube_pivot_defaults_to_uninflated_center_and_preserves_an_explicit_zero() {
    let cubes = parse(&serde_json::json!([
        {"origin":[2,3,4],"size":[6,8,10],"rotation":[17,23,31],"inflate":5},
        {"origin":[2,3,4],"size":[6,8,10],"rotation":[17,23,31],"pivot":[0,0,0]}
    ]));
    assert_eq!(cubes[0].pivot.map(|value| value.get()), [5.0, 7.0, 9.0]);
    assert_eq!(cubes[1].pivot, zero_vec3());
}

#[test]
fn computed_cube_pivot_remains_bounded() {
    let max = assets::MAX_ENTITY_GEOMETRY_SCALAR;
    let cube = serde_json::json!([{"origin":[max,0,0],"size":[max,1,1]}]);
    assert!(
        parse_geometry_cubes(&cube, Path::new("bounded.geo.json"), false, zero_scalar()).is_err()
    );
}

#[test]
#[ignore = "requires CINNABAR_VANILLA_QUADRUPED_MODELS pointing to downloaded model sources"]
fn downloaded_quadruped_models_preserve_native_rotation_pivots() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CINNABAR_VANILLA_QUADRUPED_MODELS").expect("model source directory"),
    );
    for (file, identifier, expected_body_pivot) in [
        ("pig.v3.geo.json", "geometry.pig.v3", [0.0, 10.0, -1.0]),
        ("cow.v2.geo.json", "geometry.cow.v2", [0.0, 18.0, 20.0]),
        (
            "sheep.geo.json",
            "geometry.sheep.sheared.v1.8",
            [0.0, 21.0, -2.0],
        ),
    ] {
        let path = root.join(file);
        let mut geometries = BTreeMap::new();
        parse_geometry(
            file,
            &path,
            &serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap(),
            &mut BTreeMap::new(),
            &mut geometries,
        )
        .unwrap();
        let body = geometries
            .values()
            .find(|geometry| geometry.identifier.as_ref() == identifier)
            .unwrap()
            .bones
            .iter()
            .find(|bone| bone.name.as_ref() == "body")
            .unwrap();
        assert_eq!(
            body.cubes[0].pivot.map(|value| value.get()),
            expected_body_pivot
        );
    }
}

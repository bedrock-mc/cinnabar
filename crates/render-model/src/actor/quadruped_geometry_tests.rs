use super::*;

fn bounds(vertices: &[super::super::ActorRigVertex]) -> ([f32; 3], [f32; 3]) {
    let min = std::array::from_fn(|axis| {
        vertices
            .iter()
            .map(|v| v.position[axis] * 16.0)
            .fold(f32::INFINITY, f32::min)
    });
    let max = std::array::from_fn(|axis| {
        vertices
            .iter()
            .map(|v| v.position[axis] * 16.0)
            .fold(f32::NEG_INFINITY, f32::max)
    });
    (min, max)
}

fn assert_bounds(actual: ([f32; 3], [f32; 3]), min: [f32; 3], max: [f32; 3]) {
    for (actual, expected) in actual
        .0
        .into_iter()
        .zip(min)
        .chain(actual.1.into_iter().zip(max))
    {
        assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
    }
}

#[test]
fn implicit_cube_pivot_keeps_a_rotated_body_at_its_authored_center() {
    let model = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        r#"{"format_version":"1.21.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture","texture_width":64,"texture_height":64},
            "bones":[{"name":"body","cubes":[{
                "origin":[2,3,4],"size":[6,8,10],"rotation":[90,0,0],"uv":[0,0]
            }]}]}]}"#,
    )
    .unwrap()
    .unwrap();
    let mesh = skin_geometry(&model, EntityRigId(0)).unwrap();
    // Authored center [5,7,9] is unchanged; X is mirrored in the renderer's rig frame.
    assert_bounds(bounds(&mesh.vertices), [-8.0, 2.0, 5.0], [-2.0, 12.0, 13.0]);
}

#[test]
#[ignore = "requires CINNABAR_VANILLA_QUADRUPED_MODELS pointing to downloaded model sources"]
fn downloaded_pig_and_cow_body_cubes_match_native_model_space_bounds() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CINNABAR_VANILLA_QUADRUPED_MODELS").expect("model source directory"),
    );
    for (file, identifier, first_body_min, first_body_max) in [
        (
            "pig.v3.geo.json",
            "geometry.pig.v3",
            [-5.5, 5.5, -9.5],
            [5.5, 14.5, 7.5],
        ),
        (
            "cow.v2.geo.json",
            "geometry.cow.v2",
            [-6.0, 12.0, -9.0],
            [6.0, 22.0, 9.0],
        ),
    ] {
        let source = std::fs::read_to_string(root.join(file)).unwrap();
        let patch = serde_json::json!({"geometry":{"default":identifier}}).to_string();
        let model = assets::parse_skin_geometry(&patch, &source)
            .unwrap()
            .unwrap();
        let body_index = model
            .bones
            .iter()
            .position(|bone| bone.name.as_ref() == "body")
            .unwrap();
        let mesh = skin_geometry(&model, EntityRigId(0)).unwrap();
        let body_vertices: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.bone_index as usize == body_index)
            .take(36)
            .copied()
            .collect();
        assert_eq!(body_vertices.len(), 36);
        assert_bounds(bounds(&body_vertices), first_body_min, first_body_max);
        assert!(model.bones[body_index].rotation.is_none());
    }
}

#[test]
#[ignore = "requires CINNABAR_ENTITY_CARRIER pointing to the compiled pinned entity carrier"]
fn pinned_fox_cube_binds_keep_body_horizontal_and_tail_clear_of_the_floor() {
    let bytes = std::fs::read(std::env::var_os("CINNABAR_ENTITY_CARRIER").unwrap()).unwrap();
    let assets = assets::RuntimeEntityAssets::decode(&bytes).unwrap();
    let index = find_geometry_index(&assets, "geometry.fox").unwrap() as usize;
    let bones = resolve_geometry_bones(&assets, index).unwrap();
    let mesh = entity_geometry(&assets, index, EntityRigId(0)).unwrap();
    let bone_vertices = |name: &str| {
        let index = bones
            .iter()
            .position(|bone| bone.name.as_ref() == name)
            .unwrap();
        mesh.vertices
            .iter()
            .filter(|v| v.bone_index as usize == index)
            .copied()
            .collect::<Vec<_>>()
    };
    assert_bounds(
        bounds(&bone_vertices("body")),
        [-3.0, 5.0, -3.0],
        [3.0, 11.0, 8.0],
    );
    // Independently measured native bind results in authored pixels (X is mirrored).
    assert_bounds(
        bounds(&bone_vertices("tail")),
        [-2.0, 4.047_701, 7.5940995],
        [2.0, 10.534573, 17.32561],
    );
    assert_bounds(
        bounds(&bone_vertices("head")),
        [-4.0, 4.0, -12.0],
        [4.0, 12.0, -3.0],
    );
    assert_bounds(
        bounds(&bone_vertices("leg0")),
        [1.005, 0.0, 5.0],
        [3.005, 6.0, 7.0],
    );
    for bone in &bones {
        assert!(
            bone.rotation.is_none(),
            "cube binds must not tilt {} or its children",
            bone.name
        );
    }
}

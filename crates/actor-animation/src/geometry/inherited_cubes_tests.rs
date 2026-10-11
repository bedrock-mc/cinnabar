use super::*;

#[test]
fn inherited_skeleton_metadata_preserves_defaults_without_copying_cubes() {
    let model = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        r#"{"format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture"},
            "bones":[{"name":"head","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]
        }]}"#,
    )
    .unwrap()
    .unwrap();
    let mut base = bone_metadata(&model.bones[0]);
    assert!(base.cubes.is_empty());
    assert!(base.texture_meshes.is_empty());
    let mut child = model.bones[0].clone();
    child.pivot = Some([1.0, 2.0, 3.0].map(|v| assets::EntityGeometryScalar::new(v).unwrap()));
    child.rotation = Some([4.0, 5.0, 6.0].map(|v| assets::EntityGeometryScalar::new(v).unwrap()));
    child.binding = Some("query.fixture".into());
    overlay_bone(&mut base, &child);
    assert_eq!(base.pivot, child.pivot);
    assert_eq!(base.rotation, child.rotation);
    assert_eq!(base.binding, child.binding);
    assert!(base.cubes.is_empty());
    child.reset = Some(true);
    overlay_bone(&mut base, &child);
    assert!(base.cubes.is_empty());
    child.cubes = Box::default();
    overlay_bone(&mut base, &child);
    assert!(base.cubes.is_empty());
    let (bones, names) = skeleton(&[base]).unwrap();
    assert_eq!(names[0].as_ref(), "head");
    assert_eq!(bones[0].pivot, [-1.0, 2.0, 3.0]);
    assert_eq!(bones[0].rotation, [4.0, 5.0, 6.0]);
    assert!(bones[0].has_binding_expression);
}

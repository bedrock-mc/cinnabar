use super::*;

#[test]
fn captured_persona_body_polygon_survives_geometry_building() {
    let model = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        include_str!("fixtures/skin_poly_mesh.json"),
    )
    .unwrap()
    .unwrap();
    let mesh = skin_geometry(&model, EntityRigId(0)).expect("persona body draws");
    assert_eq!(mesh.vertices.len(), 6);
    assert_eq!(mesh.vertices[0].position, [0.25, 1.5, -0.125]);
    assert_eq!(mesh.vertices[0].normal, [0.0, 1.0, 0.0]);
    assert_eq!(mesh.vertices[0].uv, [0.08203125, 0.01953125]);
    assert!(mesh.vertices.iter().all(|vertex| vertex.bone_index == 0));
}

#[test]
fn invalid_polygon_indices_do_not_discard_other_faces_or_shift_bones() {
    let mut source: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/skin_poly_mesh.json")).unwrap();
    let bones = source["minecraft:geometry"][0]["bones"]
        .as_array_mut()
        .unwrap();
    bones.insert(0, serde_json::json!({"name":""}));
    bones.insert(1, serde_json::json!({"name":"root"}));
    let polygons = bones[2]["poly_mesh"]["polys"].as_array_mut().unwrap();
    polygons.insert(0, serde_json::json!([[999, 0, 0], [1, 0, 1], [2, 0, 2]]));
    let geometry = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        &source.to_string(),
    )
    .unwrap()
    .unwrap();
    let mesh = skin_geometry(&geometry, EntityRigId(0)).unwrap();
    assert_eq!(mesh.vertices.len(), 6);
    assert!(mesh.vertices.iter().all(|vertex| vertex.bone_index == 1));
}

/// Vanilla flips polygon V while parsing, then scales pixel UVs during tessellation.
#[test]
fn polygon_pixel_uvs_are_flipped_before_texture_scaling() {
    let mut source: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/skin_poly_mesh.json")).unwrap();
    let mesh = &mut source["minecraft:geometry"][0]["bones"][0]["poly_mesh"];
    mesh["normalized_uvs"] = serde_json::json!(false);
    mesh["uvs"][0] = serde_json::json!([21.0, -4.0]);
    let model = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        &source.to_string(),
    )
    .unwrap()
    .unwrap();
    let built = skin_geometry(&model, EntityRigId(0)).unwrap();
    assert_eq!(built.vertices[0].uv, [21.0 / 256.0, 5.0 / 256.0]);
}

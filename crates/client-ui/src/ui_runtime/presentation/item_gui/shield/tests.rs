use assets::gui_item::{
    SHIELD_GUI_MODEL_SCALE as GUI_MODEL_SCALE, SHIELD_GUI_ROTATION_RADIANS as GUI_ROTATION_RADIANS,
    SHIELD_GUI_TRANSLATION as GUI_TRANSLATION, SHIELD_MODEL_PART_HEIGHT as MODEL_PART_HEIGHT,
    SHIELD_MODEL_UNIT as MODEL_UNIT,
};
use assets::{EntityGeometryScalar as Scalar, EntityGeometryUv};
use {super::*, assets::gui_item::GUI_ITEM_SIDE, ui::IconRef};

fn scalar(value: f32) -> Scalar {
    Scalar::new(value).unwrap()
}

fn geometry() -> EntityGeometry {
    EntityGeometry {
        visible_bounds: None,
        identifier: "geometry.fixture".into(),
        inherits: None,
        source_index: 0,
        texture_width: 32,
        texture_height: 32,
        bones: vec![EntityGeometryBone {
            name: ROOT_BONE.into(),
            parent: None,
            pivot: Some([2.0, 14.0, 1.0].map(scalar)),
            rotation: None,
            bind_pose_rotation: None,
            mirror: None,
            inflate: None,
            never_render: None,
            reset: None,
            binding: None,
            texture_meshes: Box::new([]),
            cubes: vec![EntityGeometryCube {
                origin: [-4.0, 16.0, -2.0].map(scalar),
                size: [10.0, 18.0, 2.0].map(scalar),
                pivot: [0.0; 3].map(scalar),
                rotation: [0.0; 3].map(scalar),
                uv: EntityGeometryUv::Box([0.0; 2].map(scalar)),
                inflate: scalar(0.0),
                mirror: false,
            }]
            .into(),
        }]
        .into(),
    }
}

fn texture() -> EquipmentTexture {
    EquipmentTexture {
        identifier: "textures/entity/fixture".into(),
        width: 32,
        height: 32,
        rgba8: [200, 60, 20, 255].repeat(32 * 32).into(),
    }
}

fn texture_ref() -> IconRef {
    IconRef {
        page: 3,
        uv: [16, 48, 48, 80],
        glint: false,
    }
}

#[test]
fn gui_projection_matches_modelpart_orientation_and_native_matrix_order() {
    assert_eq!(
        project([0.0, MODEL_PART_HEIGHT, 0.0]),
        [GUI_TRANSLATION[0], GUI_TRANSLATION[1]].map(|value| value / GUI_ITEM_SIDE)
    );
    let (sin, cos) = GUI_ROTATION_RADIANS.sin_cos();
    let projected = project([1.0 / MODEL_UNIT, MODEL_PART_HEIGHT, 0.0]);
    assert!(
        (projected[0] - (GUI_TRANSLATION[0] + GUI_MODEL_SCALE * cos) / GUI_ITEM_SIDE).abs() < 1e-6
    );
    assert!(
        (projected[1] - (GUI_TRANSLATION[1] + GUI_MODEL_SCALE * sin * sin) / GUI_ITEM_SIDE).abs()
            < 1e-6
    );
}

#[test]
fn gui_mesh_uses_bound_sheet_white_color_alpha_test_and_no_depth() {
    let mesh = mesh(&geometry(), &texture(), texture_ref()).unwrap();
    assert!(!mesh.vertices().is_empty());
    assert_eq!(mesh.batches().len(), 1);
    let batch = &mesh.batches()[0];
    assert_eq!(batch.texture_page, 3);
    assert_eq!(batch.alpha_cutoff, Some(SHIELD_ALPHA_CUTOFF));
    assert!(!batch.depth_test && !batch.depth_write);
    assert!(
        mesh.vertices()
            .iter()
            .all(|vertex| vertex.color == [255; 4] && vertex.alpha_test)
    );
    for triangle in mesh.indices().chunks_exact(3) {
        let points: [[f32; 2]; 3] =
            std::array::from_fn(|index| mesh.vertices()[triangle[index] as usize].position);
        assert!(signed_area(points[0], points[1], points[2]) > 0.0);
    }
    let mut changed_pivot = geometry();
    changed_pivot.bones[0].pivot = Some([30.0, -12.0, 2.0].map(scalar));
    assert_eq!(
        mesh.as_ref(),
        super::mesh(&changed_pivot, &texture(), texture_ref())
            .unwrap()
            .as_ref()
    );
}

#[test]
fn unsupported_modelpart_branches_are_not_silently_guessed() {
    let mut rotated = geometry();
    rotated.bones[0].rotation = Some([1.0, 0.0, 0.0].map(scalar));
    assert!(mesh(&rotated, &texture(), texture_ref()).is_none());
    let mut fractional_uv = geometry();
    fractional_uv.bones[0].cubes[0].uv = EntityGeometryUv::Box([0.5, 0.0].map(scalar));
    let fractional = mesh(&fractional_uv, &texture(), texture_ref()).unwrap();
    assert!(
        fractional
            .vertices()
            .iter()
            .any(|vertex| vertex.uv[0].fract() == 0.5)
    );
    let mut mismatched_texture_ref = texture_ref();
    mismatched_texture_ref.uv[2] += 1;
    assert!(mesh(&geometry(), &texture(), mismatched_texture_ref).is_none());
}

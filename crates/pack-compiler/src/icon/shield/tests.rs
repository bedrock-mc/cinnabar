use super::*;
use assets::gui_item::{
    SHIELD_ALPHA_CUTOFF, SHIELD_GUI_MODEL_SCALE as GUI_MODEL_SCALE,
    SHIELD_GUI_ROTATION_RADIANS as GUI_ROTATION_RADIANS, SHIELD_GUI_TRANSLATION as GUI_TRANSLATION,
    SHIELD_MODEL_PART_HEIGHT as MODEL_PART_HEIGHT, SHIELD_MODEL_UNIT as MODEL_UNIT,
};
use assets::{EntityGeometryScalar as Scalar, EntityGeometryUv};

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

fn texture(alpha: u8) -> EquipmentTexture {
    EquipmentTexture {
        identifier: "textures/entity/fixture".into(),
        width: 32,
        height: 32,
        rgba8: [200, 60, 20, alpha].repeat(32 * 32).into(),
    }
}

#[test]
fn gui_projection_uses_modelpart_y_orientation_then_y_rotation_then_x_rotation() {
    assert_eq!(
        project([0.0, MODEL_PART_HEIGHT, 0.0]),
        GUI_TRANSLATION[..2]
            .iter()
            .map(|v| v * PIXELS_PER_GUI_PIXEL)
            .collect::<Vec<_>>()
            .as_slice()
    );
    let (sin, cos) = GUI_ROTATION_RADIANS.sin_cos();
    let p = project([1.0 / MODEL_UNIT, MODEL_PART_HEIGHT, 0.0]);
    assert!(
        (p[0] - (GUI_TRANSLATION[0] + GUI_MODEL_SCALE * cos) * PIXELS_PER_GUI_PIXEL).abs() < 1e-5
    );
    assert!(
        (p[1] - (GUI_TRANSLATION[1] + GUI_MODEL_SCALE * sin * sin) * PIXELS_PER_GUI_PIXEL).abs()
            < 1e-5
    );
}

#[test]
fn model_bake_is_a_projected_cutout_not_its_raw_uv_sheet_or_cube_lighting() {
    let sprite = bake(&geometry(), &texture(255)).unwrap();
    assert_eq!(
        (usize::from(sprite.width), usize::from(sprite.height)),
        (SIDE, SIDE)
    );
    let opaque = sprite
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] != 0)
        .count();
    assert!(
        opaque > 100 && opaque < SIDE * SIDE / 2,
        "projected model silhouette, pixels={opaque}"
    );
    assert!(
        sprite
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] != 0)
            .all(|pixel| *pixel == [200, 60, 20, 255])
    );
    let mut narrower = geometry();
    narrower.bones[0].cubes[0].size[0] = scalar(4.0);
    assert_ne!(sprite.rgba8, bake(&narrower, &texture(255)).unwrap().rgba8);
    // A static part's pivot cancels its box-local offset, not an arbitrary model-centering rule.
    let mut moved_pivot = geometry();
    moved_pivot.bones[0].pivot = Some([11.0, -30.0, 5.0].map(scalar));
    assert_eq!(sprite, bake(&moved_pivot, &texture(255)).unwrap());
}

#[test]
fn gui_alpha_test_uses_the_sampled_half_alpha_threshold() {
    let accepted = (SHIELD_ALPHA_CUTOFF * f32::from(u8::MAX)).ceil() as u8;
    assert!(
        bake(&geometry(), &texture(accepted - 1))
            .unwrap()
            .rgba8
            .iter()
            .all(|byte| *byte == 0)
    );
    assert!(
        bake(&geometry(), &texture(accepted))
            .unwrap()
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] >= accepted)
    );
}

#[test]
fn unsupported_animated_modelpart_geometry_is_not_silently_approximated() {
    let mut geometry = geometry();
    geometry.bones[0].rotation = Some([1.0, 0.0, 0.0].map(scalar));
    assert!(bake(&geometry, &texture(255)).is_none());
}

#[test]
fn face_uv_defaults_use_authored_dimensions_and_explicit_negative_size_is_retained() {
    let mut cube = geometry().bones[0].cubes[0].clone();
    cube.size = [2.5, 3.75, 4.25].map(scalar);
    cube.uv = EntityGeometryUv::Faces(assets::EntityGeometryFaceUvs {
        north: Some(assets::EntityGeometryFaceUv {
            uv: [5.0, 7.0].map(scalar),
            uv_size: None,
        }),
        south: Some(assets::EntityGeometryFaceUv {
            uv: [8.0, 10.0].map(scalar),
            uv_size: Some([-2.0, 3.0].map(scalar)),
        }),
        east: None,
        west: None,
        up: None,
        down: None,
    });
    let uv = face_uvs(&cube);
    assert_eq!(
        uv[0],
        Some([[5.0, 7.0], [7.5, 7.0], [7.5, 10.75], [5.0, 10.75]])
    );
    assert_eq!(
        uv[1],
        Some([[8.0, 10.0], [6.0, 10.0], [6.0, 13.0], [8.0, 13.0]])
    );
    assert!(uv[2..].iter().all(Option::is_none));
}

#[test]
fn review_shared_triangle_edges_blend_once() {
    let mut pixels = vec![0; SIDE * SIDE * 4];
    let texture = texture(128);
    for points in [
        [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]],
        [[0.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
    ] {
        raster::triangle(&mut pixels, points, [[0.5; 2]; 3], &texture);
    }
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(pixels[(y * SIDE + x) * 4 + 3], 128, "pixel {x},{y}");
        }
    }
}

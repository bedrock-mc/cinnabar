use super::*;
use crate::dropped_item::{
    OPAQUE_WHITE, cube_mesh, dropped_item_transform, native_dropped_sprite_mesh,
};

/// Builds bounds from the same cube vertices the item renderer uploads.
fn cube() -> DroppedItemBounds {
    DroppedItemBounds::from_mesh(&cube_mesh([0; 6], [OPAQUE_WHITE; 6], 1, 1).unwrap()).unwrap()
}

/// Uses the production reverse-Z near distance with a camera facing the requested point.
fn view(eye: Vec3, target: Vec3) -> DroppedItemCullView {
    let up = if (target - eye).normalize().dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    DroppedItemCullView::new(
        glam::camera::rh::proj::directx::perspective_infinite_reverse(
            0.65,
            1.0,
            render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ) * glam::camera::rh::view::look_at_mat4(eye, target, up),
        eye,
    )
    .unwrap()
}

#[test]
fn camera_inside_or_near_plane_intersecting_any_cube_face_retains_rear_surfaces() {
    let bounds = cube()
        .transformed(dropped_item_transform([0.0; 3], 0.0, 1.0))
        .unwrap();
    let near = render_api::CAMERA_NEAR_PLANE_BLOCKS;
    for axis in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
        for eye in [
            axis * 0.25,
            axis * 0.5,
            axis * (0.5 + near * 0.8),
            axis * (0.5 + near),
        ] {
            assert!(
                !bounds.allows_back_face_culling(&view(eye, Vec3::ZERO)),
                "{eye:?}"
            );
        }
        assert!(bounds.allows_back_face_culling(&view(axis * (0.5 + near * 4.0), Vec3::ZERO)));
    }
    assert!(!bounds.allows_back_face_culling(&view(Vec3::ZERO, -Vec3::Z)));
}

#[test]
fn transformed_bounds_enclose_rotated_scaled_and_sheared_mesh_vertices() {
    let mesh = cube_mesh([0; 6], [OPAQUE_WHITE; 6], 1, 1).unwrap();
    for rows in [
        dropped_item_transform([10.0, 2.0, -3.0], 0.7, 2.0),
        [
            [1.0, 0.35, -0.2, 10.0],
            [0.2, 2.0, 0.1, -3.0],
            [-0.3, 0.4, 0.75, 5.0],
        ],
    ] {
        let bounds = cube().transformed(rows).unwrap();
        for vertex in &mesh {
            let point = Vec3::from_array(vertex.position).extend(1.0);
            let position = Vec3::from_array(rows.map(|row| Vec4::from_array(row).dot(point)));
            assert!(position.cmpge(bounds.min).all() && position.cmple(bounds.max).all());
        }
        let center = (bounds.min + bounds.max) * 0.5;
        assert!(!bounds.allows_back_face_culling(&view(center, center - Vec3::Z)));
        assert!(bounds.allows_back_face_culling(&view(center + Vec3::Z * 20.0, center)));
    }
}

#[test]
fn sprite_bounds_retain_the_native_floor_origin_and_depth() {
    let mesh = native_dropped_sprite_mesh(1, 1, &[255; 4], 32, 0).unwrap();
    let bounds = DroppedItemBounds::from_mesh(&mesh).unwrap();
    assert_eq!(bounds.min.y, 0.0);
    assert_eq!(bounds.max.y, 1.0);
    assert_eq!(bounds.min.z, -1.0 / 16.0);
    assert_eq!(bounds.max.z, 0.0);
    assert!(!bounds.allows_back_face_culling(&view(
        Vec3::new(0.0, 0.5, -1.0 / 32.0),
        Vec3::new(0.0, 0.5, -1.0)
    )));
}

#[test]
fn missing_or_nonfinite_geometry_and_views_cannot_justify_culling() {
    assert!(DroppedItemBounds::from_mesh(&[]).is_none());
    let mut mesh = cube_mesh([0; 6], [OPAQUE_WHITE; 6], 1, 1).unwrap();
    mesh[0].position[0] = f32::NAN;
    assert!(DroppedItemBounds::from_mesh(&mesh).is_none());
    assert!(cube().transformed([[f32::NAN; 4]; 3]).is_none());
    assert!(DroppedItemCullView::new(Mat4::ZERO, Vec3::ZERO).is_none());
    assert!(DroppedItemCullView::new(Mat4::from_cols_array(&[f32::NAN; 16]), Vec3::ZERO).is_none());
}

#[test]
fn camera_containment_disables_culling_even_when_its_near_plane_is_behind_the_bound() {
    let inside = DroppedItemCullView::new(Mat4::IDENTITY, Vec3::ZERO).unwrap();
    assert!(!cube().allows_back_face_culling(&inside));
    let outside = DroppedItemCullView::new(Mat4::IDENTITY, Vec3::Z * 2.0).unwrap();
    assert!(cube().allows_back_face_culling(&outside));
    assert!(DroppedItemCullView::new(Mat4::IDENTITY, Vec3::splat(f32::NAN)).is_none());
}

/// Checks every emitted position against a retained conservative model bound.
fn encloses(bounds: DroppedItemBounds, mesh: &[ItemMeshVertex]) {
    assert!(!mesh.is_empty());
    for vertex in mesh {
        let point = Vec3::from_array(vertex.position);
        assert!(
            point.cmpge(bounds.min).all() && point.cmple(bounds.max).all(),
            "{point:?}"
        );
    }
}

#[test]
fn retained_native_bounds_enclose_cube_and_sprite_mesh_contracts() {
    use crate::{DroppedItemCube, DroppedItemSprite};
    let cube_model = DroppedItemModel::Cube(DroppedItemCube {
        tile: 1,
        faces: std::array::from_fn(|_| std::sync::Arc::from([255; 4])),
        tints: [OPAQUE_WHITE; 6],
    });
    encloses(
        DroppedItemBounds::from_model(&cube_model).unwrap(),
        &cube_mesh([0; 6], [OPAQUE_WHITE; 6], 1, 1).unwrap(),
    );
    for (width, height) in [(1, 1), (4, 1), (1, 4), (16, 16), (23, 16), (32, 64)] {
        let pixels = vec![255; (width * height * 4) as usize];
        let model = DroppedItemModel::NativeSprite(DroppedItemSprite {
            width,
            height,
            rgba8: pixels.clone().into(),
        });
        encloses(
            DroppedItemBounds::from_model(&model).unwrap(),
            &native_dropped_sprite_mesh(width, height, &pixels, width.max(height), 0).unwrap(),
        );
    }
    assert!(DroppedItemBounds::from_model(&DroppedItemModel::Vacant).is_none());
}

#[test]
fn retained_bounds_follow_nonunit_block_templates_and_every_quarter_turn() {
    use crate::{DroppedItemBlock, DroppedItemSprite, dropped_item::block_mesh};
    let quad = assets::ModelQuad {
        positions: [
            [-1024, -768, 4096],
            [2048, -768, 4096],
            [2048, 1536, 4096],
            [-1024, 1536, 4096],
        ],
        uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        material: 0,
        flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
    };
    for rotation in 0..4 {
        let block = DroppedItemBlock {
            materials: std::sync::Arc::from([(
                DroppedItemSprite {
                    width: 1,
                    height: 1,
                    rgba8: std::sync::Arc::from([255; 4]),
                },
                OPAQUE_WHITE,
            )]),
            quads: std::sync::Arc::from([quad]),
            rotation,
        };
        let mesh = block_mesh(&block, 0, 1).unwrap();
        let bounds = DroppedItemBounds::from_model(&DroppedItemModel::Block(block)).unwrap();
        encloses(bounds, &mesh);
        let actual = DroppedItemBounds::from_mesh(&mesh).unwrap();
        assert_eq!(bounds.min, actual.min);
        assert_eq!(bounds.max, actual.max);
    }
}

#[test]
fn presentation_frustum_rejects_all_six_planes_but_keeps_boundaries() {
    let frustum = DroppedItemFrustum::new(Mat4::IDENTITY, Vec3::ZERO).unwrap();
    for center in [
        [-2.0, 0.0, 0.5],
        [2.0, 0.0, 0.5],
        [0.0, -2.0, 0.5],
        [0.0, 2.0, 0.5],
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 2.0],
    ] {
        assert!(
            frustum.rejects(
                cube()
                    .transformed(dropped_item_transform(center, 0.0, 0.2))
                    .unwrap()
            ),
            "{center:?}"
        );
    }
    for center in [
        [-1.1, 0.0, 0.5],
        [1.1, 0.0, 0.5],
        [0.0, -1.1, 0.5],
        [0.0, 1.1, 0.5],
        [0.0, 0.0, -0.1],
        [0.0, 0.0, 1.1],
        [0.0; 3],
    ] {
        assert!(
            !frustum.rejects(
                cube()
                    .transformed(dropped_item_transform(center, 0.0, 0.2))
                    .unwrap()
            ),
            "{center:?}"
        );
    }
}

#[test]
fn presentation_rounding_and_negative_world_coordinates_keep_touching_boxes() {
    for origin in [Vec3::ZERO, Vec3::new(-30_000.0, -64.0, -20_000.0)] {
        let frustum = DroppedItemFrustum::new(Mat4::from_translation(-origin), origin).unwrap();
        let touching = origin + Vec3::new(1.1_f32.next_up(), 0.0, 0.5);
        assert!(
            !frustum.rejects(
                cube()
                    .transformed(dropped_item_transform(touching.to_array(), 0.0, 0.2))
                    .unwrap()
            )
        );
        let outside = origin + Vec3::new(2.0, 0.0, 0.5);
        assert!(
            frustum.rejects(
                cube()
                    .transformed(dropped_item_transform(outside.to_array(), 0.0, 0.2))
                    .unwrap()
            )
        );
    }
}

#[test]
fn reverse_z_infinite_frustum_keeps_near_intersections_and_camera_inside() {
    let clip = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        0.65,
        1.0,
        render_api::CAMERA_NEAR_PLANE_BLOCKS,
    );
    let frustum = DroppedItemFrustum::new(clip, Vec3::ZERO).unwrap();
    for center in [
        [0.0; 3],
        [0.0, 0.0, -render_api::CAMERA_NEAR_PLANE_BLOCKS],
        [0.0, 0.0, -100_000.0],
    ] {
        assert!(
            !frustum.rejects(
                cube()
                    .transformed(dropped_item_transform(center, 0.0, 1.0))
                    .unwrap()
            )
        );
    }
    assert!(
        frustum.rejects(
            cube()
                .transformed(dropped_item_transform([0.0, 0.0, 2.0], 0.0, 1.0))
                .unwrap()
        )
    );
    for matrix in [
        Mat4::ZERO,
        Mat4::from_cols_array(&[f32::NAN; 16]),
        Mat4::from_scale(Vec3::new(0.0, 1.0, 1.0)),
    ] {
        assert!(DroppedItemFrustum::new(matrix, Vec3::ZERO).is_none());
    }
    assert!(DroppedItemFrustum::new(clip, Vec3::splat(f32::INFINITY)).is_none());
}

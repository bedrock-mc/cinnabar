use std::sync::Arc;
use ui::{
    SafeArea, UiBlendMode, UiMesh, UiMeshBatch, UiMeshError, UiMeshVertex, UiNode, UiNodeId,
    UiPoint, UiRect, UiScale, UiTree, UiVisual,
};

fn rect(x: f32, y: f32, width: f32, height: f32) -> UiRect {
    UiRect::new(
        UiPoint::new(x, y).unwrap(),
        UiPoint::new(x + width, y + height).unwrap(),
    )
    .unwrap()
}

fn vertex(position: [f32; 2], w: f32) -> UiMeshVertex {
    UiMeshVertex {
        position,
        clip_z: 0.25 * w,
        clip_w: w,
        uv: [8.5, 4.5],
        color: [200, 100, 50, 255],
        style_flags: 0,
        alpha_test: false,
        model_light: 0.718_629,
        overlay_color: [0.0; 4],
    }
}

fn batch(page: u16, range: std::ops::Range<u32>) -> UiMeshBatch {
    UiMeshBatch {
        texture_page: page,
        index_range: range,
        blend: UiBlendMode::Alpha,
        depth_test: true,
        depth_write: true,
        alpha_cutoff: Some(0.1),
    }
}

fn mesh() -> Arc<UiMesh> {
    Arc::new(
        UiMesh::new(
            vec![
                vertex([0.0, 0.0], 2.0),
                vertex([2.0, 0.0], 2.0),
                vertex([0.0, 2.0], 2.0),
            ]
            .into(),
            vec![0, 1, 2, 2, 1, 0].into(),
            vec![
                batch(1, 0..3),
                UiMeshBatch {
                    depth_write: false,
                    alpha_cutoff: None,
                    ..batch(2, 3..6)
                },
            ]
            .into(),
        )
        .unwrap(),
    )
}

#[test]
fn mesh_fades_share_topology_and_do_not_change_the_cached_source() {
    let source = mesh();
    let faded = (*source).clone().with_opacity(0.5);
    assert_eq!(faded.vertices()[0].color, [200, 100, 50, 128]);
    assert_eq!(source.vertices()[0].color, [200, 100, 50, 255]);
    assert!(std::ptr::eq(
        source.indices().as_ptr(),
        faded.indices().as_ptr()
    ));
    assert!(std::ptr::eq(
        source.batches().as_ptr(),
        faded.batches().as_ptr()
    ));
    assert_eq!((*source).clone().with_opacity(f32::NAN), *source);
    assert_eq!(
        (*source).clone().with_opacity(-1.0).vertices()[0].color[3],
        0
    );
    assert!(std::ptr::eq(
        source.vertices().as_ptr(),
        (*source).clone().with_opacity(1.0).vertices().as_ptr()
    ));
}

#[test]
fn ui_mesh_keeps_node_layout_clips_homogeneous_depth_and_authored_material_order() {
    let mut tree = UiTree::new(vec![
        UiNode::new(UiNodeId::new(1), None, rect(0.0, 0.0, 50.0, 50.0)).with_clip_children(true),
        UiNode::new(
            UiNodeId::new(2),
            Some(UiNodeId::new(1)),
            rect(10.0, 20.0, 20.0, 10.0),
        )
        .with_visual(UiVisual::Mesh(mesh())),
        UiNode::new(UiNodeId::new(3), None, rect(0.0, 0.0, 10.0, 10.0)).with_visual(
            UiVisual::Solid {
                texture_page: 1,
                color: [255; 4],
            },
        ),
        UiNode::new(UiNodeId::new(4), None, rect(0.0, 0.0, 10.0, 10.0))
            .with_visual(UiVisual::Mesh(mesh())),
    ])
    .unwrap();
    tree.layout(
        rect(0.0, 0.0, 200.0, 200.0),
        UiScale::new(2.0).unwrap(),
        SafeArea::new(5.0, 7.0, 0.0, 0.0).unwrap(),
    )
    .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.vertices[0].position, [50.0, 94.0]);
    assert_eq!(draw.vertices[1].position, [130.0, 94.0]);
    assert_eq!(draw.vertices[0].clip_z, 0.5);
    assert_eq!(draw.vertices[0].clip_w, 2.0);
    assert_eq!(draw.vertices[0].alpha_cutoff, 0.1);
    assert_eq!(draw.vertices[3].alpha_cutoff, -1.0);
    assert_eq!(
        draw.batches
            .iter()
            .map(|b| (b.texture_page, b.isolated_depth_scope))
            .collect::<Vec<_>>(),
        [
            (1, Some(2)),
            (2, Some(2)),
            (1, None),
            (1, Some(4)),
            (2, Some(4))
        ]
    );
    assert_eq!(draw.batches[0].clip, rect(5.0, 7.0, 100.0, 100.0));
    assert!(!draw.batches[0].world_projection);
    assert!(draw.batches[0].depth_write);
    assert!(!draw.batches[1].depth_write);
    assert_eq!(draw.vertices[0].color, [200, 100, 50, 255]);
    assert_eq!(draw.vertices[0].uv, [8.5, 4.5]);
    assert_eq!(
        draw.vertices[0].model_light.to_bits(),
        0.718_629_f32.to_bits()
    );
}

#[test]
fn ui_mesh_rejects_unchecked_geometry_and_materials_before_draw() {
    let vertices: Arc<[UiMeshVertex]> = vec![vertex([0.0; 2], 1.0); 3].into();
    assert_eq!(
        UiMesh::new(
            vertices.clone(),
            vec![0, 1, 3].into(),
            vec![batch(0, 0..3)].into()
        ),
        Err(UiMeshError::IndexOutOfBounds)
    );
    assert_eq!(
        UiMesh::new(
            vertices.clone(),
            vec![0, 1, 2].into(),
            vec![batch(0, 1..3)].into()
        ),
        Err(UiMeshError::InvalidBatchRange)
    );
    assert_eq!(
        UiMesh::new(
            vertices.clone(),
            vec![0, 1, 2].into(),
            vec![UiMeshBatch {
                alpha_cutoff: Some(f32::NAN),
                ..batch(0, 0..3)
            }]
            .into()
        ),
        Err(UiMeshError::InvalidAlphaCutoff)
    );
    assert_eq!(
        UiMesh::new(
            vec![vertex([0.0; 2], 0.0); 3].into(),
            vec![0, 1, 2].into(),
            vec![batch(0, 0..3)].into()
        ),
        Err(UiMeshError::InvalidHomogeneousW)
    );
    for model_light in [f32::NAN, f32::INFINITY, -0.1] {
        let mut invalid = vertex([0.0; 2], 1.0);
        invalid.model_light = model_light;
        assert_eq!(
            UiMesh::new(
                vec![invalid; 3].into(),
                vec![0, 1, 2].into(),
                vec![batch(0, 0..3)].into()
            ),
            Err(UiMeshError::InvalidModelLight)
        );
    }
    let mut invalid = vertex([0.0; 2], 1.0);
    invalid.uv[0] = f32::NAN;
    assert_eq!(
        UiMesh::new(
            vec![invalid; 3].into(),
            vec![0, 1, 2].into(),
            vec![batch(0, 0..3)].into()
        ),
        Err(UiMeshError::NonFiniteVertex)
    );
}

//! A prepared mask row preserves cropped sampling and the ordinary inverse sprite blend.

use ui::{SafeArea, UiBlendMode, UiNode, UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual};

/// Builds one fixed viewport without a font or external texture fixture.
fn rect() -> UiRect {
    UiRect::new(
        UiPoint::new(0., 0.).unwrap(),
        UiPoint::new(90., 9.).unwrap(),
    )
    .unwrap()
}

#[test]
fn prepared_mask_row_keeps_cropped_uvs_tint_and_inverse_blending() {
    let mut tree = UiTree::new(vec![
        UiNode::new(UiNodeId::new(0), None, rect()).with_visual(UiVisual::Mesh(
            std::sync::Arc::new(
                ui::UiMesh::sprite(
                    3,
                    [14.5, 20., 19., 29.],
                    [255, 255, 255, 110],
                    UiBlendMode::Invert,
                )
                .unwrap(),
            ),
        )),
    ])
    .unwrap();
    tree.layout(rect(), UiScale::default(), SafeArea::ZERO)
        .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.batches.len(), 1);
    assert_eq!(draw.batches[0].blend, UiBlendMode::Invert);
    assert_eq!(draw.batches[0].texture_page, 3);
    assert!(!draw.vertices.is_empty());
    for vertex in &draw.vertices {
        assert_eq!(vertex.style_flags, 0);
        assert_eq!(vertex.color, [255, 255, 255, 110]);
        assert!(vertex.uv[0] >= 14.5 && vertex.uv[0] <= 19.);
    }
}

//! Pack outline draws preserve cropped sampling, full mask bounds and shared blend batches.

use ui::{SafeArea, UiBlendMode, UiNode, UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual};

/// Builds one fixed viewport without a font or external texture fixture.
fn rect() -> UiRect {
    UiRect::new(
        UiPoint::new(0., 0.).unwrap(),
        UiPoint::new(90., 9.).unwrap(),
    )
    .unwrap()
}

/// Adjacent inverse outlines stay in one batch and retain alpha instead of using food RGB.
#[test]
fn cropped_sprite_outlines_share_one_invert_batch_and_keep_the_full_mask() {
    let mut tree = UiTree::new(
        (0..10)
            .map(|index| {
                UiNode::new(UiNodeId::new(index), None, rect()).with_visual(UiVisual::Mesh(
                    std::sync::Arc::new(
                        ui::UiMesh::sprite(
                            3,
                            [14.5, 20., 19., 29.],
                            [255, 255, 255, 110],
                            UiBlendMode::Invert,
                        )
                        .unwrap(),
                    ),
                ))
            })
            .collect(),
    )
    .unwrap();
    tree.layout(rect(), UiScale::default(), SafeArea::ZERO)
        .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert_eq!(draw.batches.len(), 1);
    assert_eq!(draw.batches[0].blend, UiBlendMode::Invert);
    assert_eq!(draw.batches[0].texture_page, 3);
    assert_eq!(draw.vertices.len(), 40);
    assert_eq!(draw.indices.len(), 60);
    for vertex in &draw.vertices {
        assert_eq!(vertex.style_flags, 0);
        assert_eq!(vertex.color, [255, 255, 255, 110]);
        assert!(vertex.uv[0] >= 14.5 && vertex.uv[0] <= 19.);
    }
}

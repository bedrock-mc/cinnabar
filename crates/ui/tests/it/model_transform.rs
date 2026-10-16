//! Node paint transforms move drawn vertices only: layout, clips and siblings stay put, and
//! retained redraws match full builds.

use ui::{
    RetainedDraw, SafeArea, TextEffects, UiDrawList, UiNode, UiNodeId, UiPoint, UiRect, UiScale,
    UiTransform, UiTree, UiVisual,
};

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> UiRect {
    UiRect::new(
        UiPoint::new(left, top).unwrap(),
        UiPoint::new(right, bottom).unwrap(),
    )
    .unwrap()
}

fn solid() -> UiVisual {
    UiVisual::Solid {
        texture_page: 0,
        color: [255; 4],
    }
}

/// Doubles a 10x10 node about its own centre, in node coordinates.
fn doubled() -> UiTransform {
    UiTransform {
        linear: [2.0, 0.0, 0.0, 2.0],
        translation: [-5.0, -5.0],
    }
}

/// A clip group at (10, 10) holding a transformed leaf and an untransformed sibling.
fn nodes(transform: Option<UiTransform>) -> Vec<UiNode> {
    let leaf = UiNode::new(
        UiNodeId::new(2),
        Some(UiNodeId::new(1)),
        rect(10.0, 10.0, 20.0, 20.0),
    )
    .with_visual(solid());
    vec![
        UiNode::new(UiNodeId::new(1), None, rect(10.0, 10.0, 110.0, 110.0))
            .with_clip_children(true),
        match transform {
            Some(transform) => leaf.with_transform(transform),
            None => leaf,
        },
        UiNode::new(
            UiNodeId::new(3),
            Some(UiNodeId::new(1)),
            rect(40.0, 0.0, 50.0, 10.0),
        )
        .with_visual(solid()),
    ]
}

fn viewport() -> UiRect {
    rect(0.0, 0.0, 400.0, 300.0)
}

/// A full build at `scale`.
fn build(nodes: Vec<UiNode>, scale: f32) -> (UiTree, UiDrawList) {
    let mut tree = UiTree::new(nodes).unwrap();
    let frame = tree
        .layout(viewport(), UiScale::new(scale).unwrap(), SafeArea::ZERO)
        .unwrap();
    assert_eq!(
        frame.bounds(UiNodeId::new(2)),
        Some(rect(20.0 * scale, 20.0 * scale, 30.0 * scale, 30.0 * scale)),
        "a transform never changes layout"
    );
    let list = tree.build_draw_list().unwrap();
    (tree, list)
}

#[test]
fn transformed_vertices_scale_about_the_node_while_siblings_and_clips_stay() {
    for scale in [1.0, 2.0] {
        let (_, plain) = build(nodes(None), scale);
        let (_, moved) = build(nodes(Some(doubled())), scale);
        // The leaf's quad grows about its centre in frame space; the sibling is unchanged.
        let centre = 25.0 * scale;
        let half = 10.0 * scale;
        let positions: Vec<[f32; 2]> = moved.vertices[..4].iter().map(|v| v.position).collect();
        assert_eq!(
            positions,
            vec![
                [centre - half, centre - half],
                [centre + half, centre - half],
                [centre + half, centre + half],
                [centre - half, centre + half],
            ]
        );
        assert_eq!(moved.vertices[4..], plain.vertices[4..]);
        assert_eq!(moved.batches, plain.batches);
    }
}

#[test]
fn retained_redraws_of_a_changing_transform_match_full_builds() {
    let scale = UiScale::new(2.0).unwrap();
    let mut last = nodes(None);
    let mut draw = RetainedDraw::build(
        &last,
        viewport(),
        scale,
        SafeArea::ZERO,
        TextEffects::default(),
    )
    .unwrap();
    for transform in [Some(doubled()), None] {
        let next = nodes(transform);
        draw.update(
            &mut last,
            &next,
            viewport(),
            scale,
            SafeArea::ZERO,
            TextEffects::default(),
        )
        .unwrap();
        let (_, full) = build(next, 2.0);
        assert_eq!(draw.draw_list().vertices, full.vertices);
        assert_eq!(draw.draw_list().batches, full.batches);
    }
}

use super::*;
use ui::{RetainedDraw, SafeArea, TextEffects, UiScale, UiTree};

/// A source visual whose parent and rectangle can exercise retained layout.
fn solid(id: u32, parent: Option<u32>, bounds: [f32; 4]) -> UiNode {
    UiNode::new(
        UiNodeId::new(id),
        parent.map(UiNodeId::new),
        rect(bounds[0], bounds[1], bounds[2], bounds[3]).unwrap(),
    )
    .with_visual(UiVisual::Solid {
        texture_page: 0,
        color: [10, 20, 30, 200],
    })
}

/// Runs replayed nodes through the actual retained geometry emitter.
fn draw(nodes: &[UiNode]) -> ui::UiDrawList {
    RetainedDraw::build(
        nodes,
        rect(0.0, 0.0, 500.0, 500.0).unwrap(),
        UiScale::default(),
        SafeArea::ZERO,
        TextEffects::default(),
    )
    .unwrap()
    .draw_list()
    .clone()
}

#[test]
fn contains_artwork_with_nested_clips_fresh_ids_and_no_focus() {
    let source = [
        UiNode::new(UiNodeId::new(9), None, rect(10.0, 5.0, 90.0, 45.0).unwrap())
            .with_clip_children(true),
        solid(10, Some(9), [-10.0, -5.0, 90.0, 45.0])
            .with_focusable(true)
            .with_navigation_order(0),
    ];
    let (nodes, next) = replay(
        &source,
        [100.0, 50.0],
        [50.0, 100.0, 250.0, 300.0],
        [60.0, 0.0, 240.0, 500.0],
        0.5,
        100,
        0,
    )
    .unwrap();
    assert_eq!(next, 102);
    assert_eq!(nodes[0].id(), UiNodeId::new(100));
    assert_eq!(nodes[1].parent(), Some(UiNodeId::new(100)));
    let mut tree = UiTree::new(nodes.clone()).unwrap();
    assert!(
        tree.layout(
            rect(0.0, 0.0, 500.0, 500.0).unwrap(),
            UiScale::default(),
            SafeArea::ZERO
        )
        .unwrap()
        .focus_order()
        .is_empty()
    );
    let drawn = draw(&nodes);
    assert_eq!(
        drawn
            .vertices
            .iter()
            .map(|vertex| vertex.position[0])
            .fold(f32::INFINITY, f32::min),
        50.0
    );
    assert_eq!(
        drawn
            .vertices
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::INFINITY, f32::min),
        150.0
    );
    assert!(drawn.vertices.iter().all(|vertex| vertex.color[3] == 100));
    assert_eq!(edges(drawn.batches[0].clip), [70.0, 160.0, 230.0, 240.0]);
}

#[test]
fn preserves_node_transform_and_absolute_viewport_clip() {
    let source = [solid(1, None, [10.0, 5.0, 30.0, 15.0])
        .with_viewport_clip(rect(12.0, 6.0, 28.0, 14.0).unwrap())
        .with_transform(UiTransform {
            linear: [2.0, 0.0, 0.0, 1.0],
            translation: [3.0, 4.0],
        })];
    let (nodes, _) = replay(
        &source,
        [40.0, 20.0],
        [20.0, 30.0, 100.0, 70.0],
        [0.0, 0.0, 500.0, 500.0],
        1.0,
        10,
        0,
    )
    .unwrap();
    let drawn = draw(&nodes);
    assert_eq!(drawn.vertices[0].position, [46.0, 48.0]);
    assert_eq!(edges(drawn.batches[0].clip), [44.0, 42.0, 76.0, 58.0]);
}

#[test]
fn preserves_retained_paint_order_when_source_subtrees_are_interleaved() {
    let colored = |id, parent, color| {
        solid(id, parent, [0.0, 0.0, 10.0, 10.0]).with_visual(UiVisual::Solid {
            texture_page: 0,
            color,
        })
    };
    let source = [
        colored(10, None, [1, 0, 0, 255]),
        colored(20, None, [2, 0, 0, 255]),
        colored(30, Some(10), [3, 0, 0, 255]),
        colored(25, Some(10), [4, 0, 0, 255]),
    ];
    let (replayed, _) = replay(
        &source,
        [10.0; 2],
        [0.0, 0.0, 10.0, 10.0],
        [0.0, 0.0, 500.0, 500.0],
        1.0,
        100,
        0,
    )
    .unwrap();
    let colors = |nodes: &[UiNode]| {
        draw(nodes)
            .vertices
            .iter()
            .map(|vertex| vertex.color)
            .collect::<Vec<_>>()
    };
    assert_eq!(colors(&replayed), colors(&source));
}

#[test]
fn normalizes_actual_clipped_artwork_with_negative_child_offsets_and_root_overflow() {
    let child = solid(2, Some(1), [-30.0, -70.0, 50.0, 20.0])
        .with_viewport_clip(rect(75.0, 35.0, 145.0, 115.0).unwrap());
    let mut source = [
        // A tiny centered reference box whose child paints beyond every layout edge.
        UiNode::new(
            UiNodeId::new(1),
            None,
            rect(100.0, 100.0, 110.0, 110.0).unwrap(),
        ),
        child.clone(),
        solid(3, None, [1000.0, 1000.0, 1010.0, 1010.0]),
    ];
    let size = normalize_fragment(&mut source, [0.0, 0.0, 500.0, 500.0])
        .unwrap()
        .unwrap();
    assert_eq!(size, [70.0, 80.0]);
    assert_eq!(edges(source[0].bounds()), [25.0, 65.0, 35.0, 75.0]);
    assert_eq!(
        source[1].bounds(),
        child.bounds(),
        "child-local layout changed"
    );
    assert_eq!(
        edges(source[1].viewport_clip().unwrap()),
        [0.0, 0.0, 70.0, 80.0]
    );
    let measured = draw(&source).painted_bounds().unwrap();
    assert_eq!(edges(measured), [0.0, 0.0, 70.0, 80.0]);
    let (painted, _) = replay(
        &source,
        size,
        [10.0, 20.0, 80.0, 100.0],
        [0.0, 0.0, 500.0, 500.0],
        1.0,
        10,
        0,
    )
    .unwrap();
    assert_eq!(
        edges(draw(&painted).painted_bounds().unwrap()),
        [10.0, 20.0, 80.0, 100.0]
    );
    let mut empty = [UiNode::new(
        UiNodeId::new(1),
        None,
        rect(1.0, 1.0, 50.0, 50.0).unwrap(),
    )];
    assert_eq!(
        normalize_fragment(&mut empty, [0.0, 0.0, 500.0, 500.0]).unwrap(),
        None
    );
}

#[test]
fn an_authored_outer_transform_moves_fragment_artwork_and_its_clip_together() {
    use crate::{
        engine::{
            FormEngine, ScreenArt,
            screen::{CustomDraw, CustomRenderer, PaintFrame, ScreenRegistry},
        },
        sources::{ImageWork, RemoteImage, RemoteSource},
        text_metrics::TextMetrics,
    };
    use json_ui::{Catalog, Context};
    struct NoImages;
    impl RemoteSource for NoImages {
        /// The synthetic fragment contains no downloaded artwork.
        fn state(&self, _: &str) -> RemoteImage {
            RemoteImage::Failed
        }
    }
    struct Fragment(Vec<UiNode>);
    impl CustomRenderer for Fragment {
        /// Replays the same fragment path used by native pack-control previews.
        fn paint(
            &mut self,
            node: &CustomDraw<'_>,
            canvas: &mut Canvas<'_>,
        ) -> Result<bool, PaintError> {
            canvas.paint_fragment(&self.0, [10.0; 2], node.dest, 1.0)?;
            Ok(true)
        }
    }
    static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
    let registry = REGISTRY.get_or_init(|| ScreenRegistry::new(Context::default()));
    let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    let assets = Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap());
    let mut catalog = Catalog::default().with_paint_transforms();
    catalog.overlay_text(
        "ui/fragment.json",
        r#"{"namespace":"fragment","root":{
        "type":"custom","renderer":"fragment_fixture","size":[50,50],
        "anchor_from":"top_left","anchor_to":"top_left","offset":[100,100],
        "cinnabar_transform":{"scale":2,"origin":[0.5,0.5]}}}"#,
    );
    let tree = json_ui::resolve(&catalog, "fragment.root", &Context::default())
        .control
        .unwrap();
    let engine = FormEngine::new(
        assets,
        catalog,
        0,
        registry,
        Arc::new(NoImages),
        ImageWork::inline(),
    );
    let font = crate::test_support::fixture_font();
    let metrics = TextMetrics::for_viewport([1000, 1000], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let px = metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
    let mut layouts = ui::TextLayoutCache::new(1, 16 * 1024);
    let mut nodes = Vec::new();
    let mut next = 1;
    let mut fragment = Fragment(vec![solid(1, None, [0.0, 0.0, 10.0, 10.0])]);
    let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut fragment];
    engine
        .draw(
            ScreenArt::default(),
            &mut PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [500.0; 2],
                translate: &|_| None,
                language: [0; 3],
                now: 0.0,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut renderers,
            },
            |env, root| {
                Some(json_ui::render_bound(
                    tree,
                    root,
                    env,
                    &json_ui::ViewState::default(),
                ))
            },
        )
        .unwrap()
        .unwrap();
    let drawn = draw(&nodes);
    let expected = [75.0, 75.0, 175.0, 175.0].map(|value| value * px);
    assert_eq!(edges(drawn.batches[0].clip), expected);
    let min = drawn
        .vertices
        .iter()
        .fold([f32::INFINITY; 2], |value, vertex| {
            [
                value[0].min(vertex.position[0]),
                value[1].min(vertex.position[1]),
            ]
        });
    let max = drawn
        .vertices
        .iter()
        .fold([f32::NEG_INFINITY; 2], |value, vertex| {
            [
                value[0].max(vertex.position[0]),
                value[1].max(vertex.position[1]),
            ]
        });
    assert_eq!([min[0], min[1], max[0], max[1]], expected);
}

#[test]
fn fades_text_and_meshes_without_changing_source_artwork() {
    let font = crate::test_support::fixture_font();
    let metrics = crate::text_metrics::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let layout = ui::TextLayoutCache::new(1, 16 * 1024)
        .layout(metrics.request("0", 200 * 64, &font))
        .unwrap();
    let visual = UiVisual::Text {
        layout: Arc::clone(&layout),
        color: [255, 200, 100, 160],
        shadow: ui::TextShadow::None,
    };
    let UiVisual::Text {
        layout: faded_layout,
        color,
        ..
    } = faded(&visual, 0.5, &mut BTreeMap::new())
    else {
        panic!("text lost");
    };
    assert!(Arc::ptr_eq(&layout, &faded_layout));
    assert_eq!(color, [255, 200, 100, 80]);
    let UiVisual::Mesh(mesh) = faded(
        &UiVisual::InvertedSprite {
            texture_page: 3,
            uv: [8, 16, 24, 32],
        },
        0.5,
        &mut BTreeMap::new(),
    ) else {
        panic!("inverse sprite lost");
    };
    assert_eq!(mesh.vertices()[0].uv, [8.0, 16.0]);
    assert_eq!(mesh.vertices()[2].uv, [24.0, 32.0]);
    assert_eq!(mesh.batches()[0].blend, UiBlendMode::Invert);
    assert_eq!(mesh.vertices()[0].color[3], 128);
    let mut shared = BTreeMap::new();
    let source = UiVisual::Mesh(Arc::clone(&mesh));
    let UiVisual::Mesh(first) = faded(&source, 0.5, &mut shared) else {
        unreachable!()
    };
    let UiVisual::Mesh(second) = faded(&source, 0.5, &mut shared) else {
        unreachable!()
    };
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.vertices()[0].color[3], 64);
    assert_eq!(mesh.vertices()[0].color[3], 128);
}

#[test]
fn fades_repeated_fragments_without_changing_source_coordinates_or_styles() {
    let source = [
        solid(1, None, [0.0, 0.0, 10.0, 10.0]).with_visual(UiVisual::RepeatedSprite {
            texture_page: 3,
            tile_uv: [0.25, 0.5, 20.25, 30.5],
            source: [2.0, 3.0, 4.0, 5.0],
            color: [80, 160, 240, 200],
            style: render_api::ui_repeat::UI_STYLE_REPEAT
                | render_api::ui_repeat::UI_STYLE_REPEAT_PAGE,
        }),
    ];
    let before = draw(&source);
    let (nodes, _) = replay(
        &source,
        [10.0; 2],
        [0.0, 0.0, 10.0, 10.0],
        [0.0, 0.0, 10.0, 10.0],
        0.5,
        10,
        0,
    )
    .unwrap();
    let after = draw(&nodes);
    assert_eq!(after.vertices.len(), before.vertices.len());
    assert_eq!(after.batches[0].texture_page, 3);
    for (before, after) in before.vertices.iter().zip(&after.vertices) {
        assert_eq!(after.uv, before.uv);
        assert_eq!(after.overlay_color, before.overlay_color);
        assert_eq!(after.style_flags, before.style_flags);
        assert_eq!(after.color, [80, 160, 240, 100]);
    }
    assert!(draw(&source).vertices.iter().all(|v| v.color[3] == 200));
}

#[test]
fn rejects_filter_fragments_as_a_whole_and_bounds_replay() {
    let ordinary = solid(1, None, [0.0, 0.0, 10.0, 10.0]);
    let filtered = solid(2, None, [0.0, 0.0, 10.0, 10.0]).with_filter_group(7);
    let (nodes, next) = replay(
        &[ordinary.clone(), filtered],
        [10.0; 2],
        [0.0, 0.0, 10.0, 10.0],
        [0.0, 0.0, 10.0, 10.0],
        1.0,
        5,
        0,
    )
    .unwrap();
    assert!(nodes.is_empty());
    assert_eq!(next, 5);
    for blend in [UiBlendMode::Filter, UiBlendMode::Backdrop] {
        let mesh =
            crate::engine::glass::cover_quad(0, render_api::UiBackdropPaint::IDENTITY, blend)
                .unwrap();
        let composite =
            solid(2, None, [0.0, 0.0, 10.0, 10.0]).with_visual(UiVisual::Mesh(Arc::new(mesh)));
        assert!(
            replay(
                &[ordinary.clone(), composite],
                [10.0; 2],
                [0.0, 0.0, 10.0, 10.0],
                [0.0, 0.0, 10.0, 10.0],
                1.0,
                5,
                0
            )
            .unwrap()
            .0
            .is_empty()
        );
    }
    for (source, dest, opacity) in [
        ([0.0, 10.0], [0.0, 0.0, 10.0, 10.0], 1.0),
        ([10.0; 2], [0.0, 0.0, f32::NAN, 10.0], 1.0),
        ([10.0; 2], [0.0, 0.0, 10.0, 10.0], f32::NAN),
    ] {
        assert!(
            replay(
                std::slice::from_ref(&ordinary),
                source,
                dest,
                [0.0, 0.0, 10.0, 10.0],
                opacity,
                5,
                0
            )
            .unwrap()
            .0
            .is_empty()
        );
    }
    assert!(matches!(
        replay(
            std::slice::from_ref(&ordinary),
            [10.0; 2],
            [0.0, 0.0, 10.0, 10.0],
            [0.0, 0.0, 10.0, 10.0],
            1.0,
            5,
            UiLimits::MAX_NODES - 1
        ),
        Err(PaintError::Tree(UiError::NodeLimitExceeded { .. }))
    ));
    let orphan = ordinary.with_identity(UiNodeId::new(1), Some(UiNodeId::new(99)));
    assert!(matches!(
        replay(
            &[orphan],
            [10.0; 2],
            [0.0, 0.0, 10.0, 10.0],
            [0.0, 0.0, 10.0, 10.0],
            1.0,
            5,
            0
        ),
        Err(PaintError::Tree(UiError::MissingParent { .. }))
    ));
}

use super::*;
use crate::{motion::Surface, transitions::Transitions, widgets};
use launcher::menu::{MenuAction, MenuView};
use ui::{UiBlendMode, UiDrawList};
use ui_engine::test_support::fixture_font;

const SIZE: [f32; 2] = [1280.0, 720.0];
const CLIP: Bounds = [100.0, 100.0, 220.0, 160.0];

#[test]
fn editable_selection_preserves_the_characters_it_selects() {
    let long = "02".repeat(40);
    for (value, selected) in [(" 0  2 ", [1, 2]), (long.as_str(), [78, 79])] {
        let (_, _, nodes) = crate::review_tests::paint(Default::default(), |canvas| {
            let mut view = MenuView::new(true, "Fixture".into());
            view.caret.byte = selected[1];
            view.caret.selection = Some(selected);
            widgets::text_field(
                canvas,
                &view,
                [90.0, 90.0, 460.0, 150.0],
                value,
                "Placeholder",
                true,
                Some(MenuAction::AddName),
            )
            .unwrap();
        });
        let text = nodes
            .iter()
            .find_map(|node| match node.visual() {
                UiVisual::Text { layout, .. } => Some(layout),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            text.glyphs()
                .iter()
                .map(|g| g.codepoint)
                .collect::<String>(),
            value
        );
        let selection = nodes
            .iter()
            .find(|node| matches!(node.visual(), UiVisual::InvertedSprite { .. }))
            .unwrap();
        let label = nodes
            .iter()
            .find(|node| matches!(node.visual(), UiVisual::Text { .. }))
            .unwrap();
        let at = world_bounds(label, &nodes);
        let bounds = world_bounds(selection, &nodes);
        let glyph = &text.glyphs()[selected[0]];
        let center = at[0] + (glyph.bounds_64[0] + glyph.bounds_64[2]) as f32 / 128.0;
        assert!(
            bounds[0] <= center && center < bounds[2],
            "the selected zero is highlighted"
        );
        assert_eq!(
            bounds[0],
            at[0] + text.first_line_caret_x_64(selected[0]) as f32 / 64.0
        );
        assert_eq!(
            bounds[2],
            at[0] + text.first_line_caret_x_64(selected[1]) as f32 / 64.0
        );
    }
}

/// Resolves local node geometry through the field's clipping ancestors.
fn world_bounds(node: &UiNode, nodes: &[UiNode]) -> Bounds {
    let bounds = node.bounds();
    let mut result = [
        bounds.min().x(),
        bounds.min().y(),
        bounds.max().x(),
        bounds.max().y(),
    ];
    let mut parent = node.parent();
    while let Some(ancestor) = parent.and_then(|id| nodes.iter().find(|node| node.id() == id)) {
        let at = ancestor.bounds().min();
        result[0] += at.x();
        result[2] += at.x();
        result[1] += at.y();
        result[3] += at.y();
        parent = ancestor.parent();
    }
    result
}

#[test]
fn editable_caret_and_pointer_share_scrolled_text_and_placeholder_geometry() {
    use crate::theme::FIELD_CARET;
    use ui_engine::caret::nearest_boundary;

    let long = "02".repeat(40);
    for (value, caret) in [
        ("", 0),
        (" 02  ", 1),
        ("   ", 2),
        (long.as_str(), 0),
        (long.as_str(), 40),
        (long.as_str(), 80),
    ] {
        let (_, _, nodes) = crate::review_tests::paint(Default::default(), |canvas| {
            let mut view = MenuView::new(true, "Fixture".into());
            view.caret.byte = caret;
            view.caret.shown = true;
            widgets::text_field(
                canvas,
                &view,
                [90.0, 90.0, 260.0, 150.0],
                value,
                "Placeholder",
                true,
                Some(MenuAction::AddName),
            )
            .unwrap();
            let spot = &canvas.spots[0];
            let target = spot.width(canvas.layouts, canvas.font, &value[..caret]);
            let point = spot.left + target;
            assert!(
                (90.0..260.0).contains(&point),
                "caret is visible after scrolling"
            );
            assert_eq!(
                nearest_boundary(value, point - spot.left, |prefix| spot.width(
                    canvas.layouts,
                    canvas.font,
                    prefix
                )),
                caret
            );
            if value.is_empty() {
                assert_eq!(target, 0.0, "placeholder does not advance the caret");
            } else {
                let layout = spot.layout.as_ref().unwrap();
                assert!(canvas.nodes.iter().any(|node| matches!(node.visual(), UiVisual::Text { layout: drawn, .. } if std::sync::Arc::ptr_eq(drawn, layout))));
            }
        });
        let caret_node = nodes.iter().find(|node| matches!(node.visual(), UiVisual::Solid { color, .. } if *color == FIELD_CARET)).unwrap();
        let caret_bounds = world_bounds(caret_node, &nodes);
        let label = nodes
            .iter()
            .find(|node| matches!(node.visual(), UiVisual::Text { .. }))
            .unwrap();
        let label_bounds = world_bounds(label, &nodes);
        assert_eq!(caret_bounds[1], label_bounds[1]);
        assert_eq!(caret_bounds[3], label_bounds[3]);
        let UiVisual::Text { layout, .. } = label.visual() else {
            unreachable!()
        };
        let x = if value.is_empty() {
            0.0
        } else {
            layout.first_line_caret_x_64(caret) as f32 / 64.0
        };
        assert_eq!(caret_bounds[0], label_bounds[0] + x);
        assert!(caret_bounds[3] - caret_bounds[1] > 0.0);
    }
}

#[test]
fn editable_scrolling_keeps_the_animated_caret_visible_during_a_long_paste() {
    let mut transitions = Transitions::default();
    transitions.configure_motion(true);
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut layouts = TextLayoutCache::new(32, 65536);
    let long = "02".repeat(40);
    for (seconds, value) in [
        (0.0, "0"),
        (1.0, long.as_str()),
        (1.025, long.as_str()),
        (2.0, long.as_str()),
    ] {
        transitions.begin_frame(None, false, seconds);
        let (mut nodes, mut next) = (Vec::new(), 1);
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        canvas.seconds = seconds;
        canvas.transitions = Some(&mut transitions);
        let mut view = MenuView::new(true, "Fixture".into());
        view.caret.byte = value.len();
        view.caret.shown = true;
        widgets::text_field(
            &mut canvas,
            &view,
            [90.0, 90.0, 260.0, 150.0],
            value,
            "",
            true,
            Some(MenuAction::AddName),
        )
        .unwrap();
        drop(canvas);
        transitions.end_frame();
        let caret = nodes.iter().find(|node| matches!(node.visual(), UiVisual::Solid { color, .. } if *color == crate::theme::FIELD_CARET)).unwrap();
        let bounds = world_bounds(caret, &nodes);
        assert!(
            bounds[0] >= 90.0 && bounds[2] <= 260.0,
            "painted caret remains visible at {seconds}s"
        );
    }
}

/// Draws a selected field through the dialog's entrance, clipping and retained exit.
fn selection_frame(
    transitions: &mut Transitions,
    seconds: f64,
    visible: bool,
    alpha: f32,
) -> UiDrawList {
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(32, 65536);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    transitions.begin_frame(None, false, seconds);
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    canvas.seconds = seconds;
    canvas.alpha = alpha;
    canvas.transitions = Some(&mut *transitions);
    if visible {
        let entrance = canvas.begin_entrance(Surface::Dialog(1));
        let clip = canvas.begin_clip(CLIP).unwrap();
        let value = "0202020202020202020202";
        let mut view = MenuView::new(true, "Fixture".into());
        view.caret.selection = Some([0, value.len()]);
        widgets::text_field(
            &mut canvas,
            &view,
            [90.0, 90.0, 260.0, 150.0],
            value,
            "",
            true,
            Some(MenuAction::AddName),
        )
        .unwrap();
        canvas.end_clip(clip);
        canvas.end_entrance(entrance, SIZE).unwrap();
    }
    drop(canvas);
    transitions
        .effects
        .finish(&mut nodes, &mut next, seconds, SIZE)
        .unwrap();
    transitions.end_frame();
    let mut tree = ui::UiTree::new(nodes).unwrap();
    tree.layout(
        rect(0.0, 0.0, SIZE[0], SIZE[1]).unwrap(),
        ui::UiScale::default(),
        ui::SafeArea::ZERO,
    )
    .unwrap();
    tree.build_draw_list().unwrap()
}

#[test]
fn selection_feedback_requires_an_opaque_surface() {
    let mut transitions = Transitions::default();
    transitions.configure_motion(false);
    for (alpha, visible) in [(0.0, false), (0.5, false), (1.0, true)] {
        let draw = selection_frame(&mut transitions, 0.0, true, alpha);
        assert_eq!(
            draw.batches
                .iter()
                .any(|batch| batch.blend == UiBlendMode::Invert),
            visible
        );
    }
}

#[test]
fn selection_feedback_waits_for_entrance_and_retires_during_exit() {
    let mut transitions = Transitions::default();
    selection_frame(&mut transitions, 0.0, false, 1.0);
    for (seconds, visible, inverted) in [
        (1.0, true, false),
        (2.0, true, true),
        (3.0, false, true),
        (3.04, false, false),
        (3.09, false, false),
    ] {
        let draw = selection_frame(&mut transitions, seconds, visible, 1.0);
        assert_eq!(
            draw.batches
                .iter()
                .any(|batch| batch.blend == UiBlendMode::Invert),
            inverted,
            "selection at {seconds}s"
        );
    }
}

#[test]
fn selection_feedback_stays_clipped_above_text_and_below_the_focus_outline() {
    let mut transitions = Transitions::default();
    transitions.configure_motion(false);
    let draw = selection_frame(&mut transitions, 0.0, true, 1.0);
    let index = draw
        .batches
        .iter()
        .position(|batch| batch.blend == UiBlendMode::Invert)
        .unwrap();
    let inversion = &draw.batches[index];
    assert!(inversion.clip.min().x() >= CLIP[0]);
    assert!(inversion.clip.min().y() >= CLIP[1]);
    assert!(inversion.clip.max().x() <= CLIP[2]);
    assert!(inversion.clip.max().y() <= CLIP[3]);
    let before = &draw.batches[index - 1];
    let after = &draw.batches[index + 1];
    assert_eq!(before.blend, UiBlendMode::Alpha);
    assert_eq!(after.blend, UiBlendMode::Alpha);
    assert!(
        draw.indices[before.index_range.start as usize..before.index_range.end as usize]
            .iter()
            .any(|index| draw.vertices[*index as usize].color == crate::theme::TEXT)
    );
}

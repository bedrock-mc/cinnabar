use super::*;
use crate::{motion::Surface, transitions::Transitions, widgets};
use launcher::menu::{MenuAction, MenuView};
use ui::{UiBlendMode, UiDrawList};
use ui_engine::test_support::fixture_font;

const SIZE: [f32; 2] = [1280.0, 720.0];
const CLIP: Bounds = [100.0, 100.0, 220.0, 160.0];

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
    assert_eq!(
        inversion.clip,
        rect(CLIP[0], CLIP[1], CLIP[2], CLIP[3]).unwrap()
    );
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

use super::super::{paint::Canvas, transitions::Transitions};
use crate::ui_runtime::presentation::{TextMetrics, tests::fixture_font};
use launcher::menu::JoinStage;
use {super::*, launcher::menu::MenuView};

fn frame(transitions: &mut Transitions, view: &MenuView, seconds: f64) -> Vec<ui::UiNode> {
    let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(128, 65536));
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    transitions.begin_frame(None, false, seconds);
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    canvas.seconds = seconds;
    canvas.transitions = Some(transitions);
    join(&mut canvas, view, [1280.0, 720.0], &|_| None).unwrap();
    drop(canvas);
    transitions
        .effects
        .finish(&mut nodes, &mut next, seconds, [1280.0, 720.0])
        .unwrap();
    transitions.end_frame();
    nodes
}

fn title_alpha(nodes: &[ui::UiNode]) -> u8 {
    nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, color, .. }
                if layout
                    .glyphs()
                    .iter()
                    .any(|glyph| glyph.codepoint.is_uppercase()) =>
            {
                Some(color[3])
            }
            _ => None,
        })
        .unwrap()
}

fn bar_width(nodes: &[ui::UiNode]) -> f32 {
    super::super::review_tests::solids(nodes)
        .into_iter()
        .find_map(|(bounds, color)| {
            (color == theme::PRIMARY_ROLE.fill).then_some(bounds[2] - bounds[0])
        })
        .expect("measured download progress paints a bar")
}

#[test]
fn joining_stages_animate_once_without_restarting_for_download_updates() {
    let mut transitions = Transitions::default();
    let mut view = MenuView::new(true, "Player".into());
    view.connecting = true;
    frame(&mut transitions, &view, 0.0);
    view.feeds.join.stage = JoinStage::Packs {
        done: 0,
        total: 1,
        received_bytes: 10,
        total_bytes: 100,
    };
    let entering = frame(&mut transitions, &view, 1.0);
    assert!(title_alpha(&entering) < 255);
    let settled = frame(&mut transitions, &view, 1.14);
    assert_eq!(title_alpha(&settled), 255);
    let before = bar_width(&settled);
    view.feeds.join.stage = JoinStage::Packs {
        done: 1,
        total: 1,
        received_bytes: 90,
        total_bytes: 100,
    };
    let updated = frame(&mut transitions, &view, 1.15);
    assert_eq!(title_alpha(&updated), 255);
    assert_eq!(bar_width(&updated), before);
    let middle = bar_width(&frame(&mut transitions, &view, 1.19));
    let finished = bar_width(&frame(&mut transitions, &view, 1.3));
    assert!(middle > before && middle < finished);
    view.feeds.join.stage = JoinStage::Generating;
    assert!(title_alpha(&frame(&mut transitions, &view, 2.0)) < 255);
    assert_eq!(title_alpha(&frame(&mut transitions, &view, 2.14)), 255);
}

#[test]
fn disabled_join_transitions_show_every_stage_immediately() {
    let mut transitions = Transitions::default();
    transitions.configure_motion(false);
    let mut view = MenuView::new(true, "Player".into());
    view.connecting = true;
    frame(&mut transitions, &view, 0.0);
    view.feeds.join.stage = JoinStage::Generating;
    assert_eq!(title_alpha(&frame(&mut transitions, &view, 1.0)), 255);
}

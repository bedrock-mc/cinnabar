use super::*;
use crate::ui_runtime::presentation::{
    TextMetrics,
    forms::oreui::{theme, transitions::Transitions, widgets},
    tests::fixture_font,
};

#[test]
fn entrances_move_clipped_content_once_and_keep_hit_geometry_and_backdrops_stable() {
    let mut transitions = Transitions::default();
    transitions.end_frame();
    let font = fixture_font();
    let mut layouts = ui::TextLayoutCache::new(16, 4096);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    canvas.seconds = 1.0;
    canvas.transitions = Some(&mut transitions);
    let scope = canvas.begin_entrance(Surface::Screen(MenuScreen::Pause));
    canvas
        .fill([0.0, 0.0, 1280.0, 720.0], [0, 0, 0, 90])
        .unwrap();
    let viewport = [100.0, 100.0, 400.0, 300.0];
    let scroll = canvas.begin_scroll("test", viewport).unwrap();
    let button = [120.0, 120.0, 300.0, 164.0];
    let view = launcher::menu::MenuView::new(true, "Player".into());
    widgets::button(
        &mut canvas,
        &view,
        button,
        widgets::Variant::Primary,
        "",
        Some(MenuAction::PauseResume),
    )
    .unwrap();
    canvas.end_scroll(scroll, 100.0).unwrap();
    let hits = canvas.hits.clone();
    let rem = canvas.rem;
    canvas.end_entrance(scope, [1280.0, 720.0]).unwrap();
    assert_eq!(canvas.hits, hits);
    assert_eq!(
        hits[0].1,
        rect(button[0], button[1], button[2], button[3]).unwrap()
    );
    drop(canvas);
    assert!(matches!(
        nodes[0].visual(),
        UiVisual::Solid {
            color: [0, 0, 0, 90],
            ..
        }
    ));
    assert_eq!(nodes[0].bounds().min().y(), 0.0);
    let mut tree = ui::UiTree::new(nodes).unwrap();
    tree.layout(
        rect(0.0, 0.0, 1280.0, 720.0).unwrap(),
        ui::UiScale::default(),
        ui::SafeArea::ZERO,
    )
    .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert!((draw.vertices[4].position[1] - (button[1] + rem * 0.6)).abs() < 0.001);
    assert!(draw.vertices[4].color[3] < 255);
    assert_eq!(theme::PRIMARY_ROLE.border[3], 255);
}

#[test]
fn disabled_motion_keeps_the_complete_face_and_hit_area_immediate() {
    let mut transitions = Transitions::default();
    transitions.motion.configure(false);
    let font = fixture_font();
    let mut layouts = ui::TextLayoutCache::new(16, 4096);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    canvas.transitions = Some(&mut transitions);
    let scope = canvas.begin_entrance(Surface::Screen(MenuScreen::Pause));
    let mut view = launcher::menu::MenuView::new(true, "Player".into());
    view.pressed = Some(MenuAction::PauseResume);
    let button = [100.0, 100.0, 400.0, 164.0];
    let expected_top = button[1] + canvas.r(0.4);
    widgets::button(
        &mut canvas,
        &view,
        button,
        widgets::Variant::Primary,
        "",
        Some(MenuAction::PauseResume),
    )
    .unwrap();
    let before = canvas.nodes.clone();
    canvas.end_entrance(scope, [1280.0, 720.0]).unwrap();
    assert_eq!(*canvas.nodes, before);
    assert_eq!(canvas.nodes[0].bounds().min().y(), expected_top);
    assert_eq!(
        canvas.hits[0].1,
        rect(button[0], button[1], button[2], button[3]).unwrap()
    );
}

use launcher::menu::{MenuAction, MenuScreen};

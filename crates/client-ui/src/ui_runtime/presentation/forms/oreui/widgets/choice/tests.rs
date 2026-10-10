use crate::ui_runtime::presentation::forms::oreui::review_tests::{paint, solids};
use {
    super::*,
    launcher::menu::{MenuAction, MenuView},
};

fn draw_choice(selected: bool) -> Vec<ui::UiNode> {
    let view = MenuView::new(true, "Player".into());
    paint(Default::default(), |canvas| {
        choice(
            canvas,
            &view,
            [100.0, 100.0, 300.0, 160.0],
            "Choice",
            selected,
            MenuAction::SettingsOption(0, i32::from(selected)),
        )
        .unwrap();
    })
    .2
}

#[test]
fn unselected_segment_uses_the_light_secondary_face_and_dark_label() {
    let nodes = draw_choice(false);
    assert!(
        solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.fill)
    );
    assert!(nodes.iter().any(|node| matches!(node.visual(),
        ui::UiVisual::Text { color, .. } if *color == theme::SECONDARY.text)));
}

#[test]
fn selected_segment_marks_the_choice_with_a_centred_bottom_underline() {
    let nodes = draw_choice(true);
    let markers: Vec<_> = solids(&nodes)
        .into_iter()
        .filter(|(bounds, color)| *color == theme::TEXT && bounds[1] >= 150.0)
        .collect();
    assert_eq!(markers.len(), 1);
    let bounds = markers[0].0;
    assert!(((bounds[0] + bounds[2]) * 0.5 - 200.0).abs() < 0.001);
    assert!(bounds[2] - bounds[0] < 100.0);
}

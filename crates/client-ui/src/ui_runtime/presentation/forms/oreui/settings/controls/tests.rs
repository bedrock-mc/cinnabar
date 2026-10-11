use crate::ui_runtime::presentation::forms::oreui::review_tests::{paint, solids};
use crate::ui_runtime::presentation::forms::oreui::transitions::Transitions;
use crate::ui_runtime::presentation::{TextMetrics, tests::fixture_font};
use {
    super::*,
    launcher::menu::{MenuAction, MenuView},
};

fn animated_switch(
    transitions: &mut Transitions,
    view: &MenuView,
    on: bool,
    seconds: f64,
) -> (f32, Vec<ui::UiNode>) {
    let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(16, 65536));
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    transitions.begin_frame(
        view.settings_control_activation,
        view.settings_control_activation_navigation,
        seconds,
    );
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    canvas.seconds = seconds;
    canvas.transitions = Some(transitions);
    let rem = canvas.rem;
    toggle_control(
        &mut canvas,
        view,
        [100.0, 100.0],
        on,
        MenuAction::SettingsOption(7, i32::from(!on)),
        true,
    )
    .unwrap();
    drop(canvas);
    transitions.end_frame();
    (rem, nodes)
}

fn switch_thumb(nodes: &[ui::UiNode], rem: f32) -> Bounds {
    solids(nodes)
        .into_iter()
        .find_map(|(bounds, color)| {
            (color == theme::SECONDARY.border
                && (bounds[2] - bounds[0] - 3.2 * rem).abs() < 0.001
                && (bounds[3] - bounds[1] - 3.2 * rem).abs() < 0.001)
                .then_some(bounds)
        })
        .expect("the switch paints its fixed-size inner thumb")
}

#[test]
fn interactive_switch_publishes_timed_geometry_without_widening_its_visible_thumb() {
    let mut transitions = Transitions::default();
    let mut view = MenuView::new(true, "Player".into());
    animated_switch(&mut transitions, &view, false, -1.0);
    view.settings_control_activation = Some((MenuAction::SettingsOption(7, 1), 1));
    let (rem, nodes) = animated_switch(&mut transitions, &view, true, 0.0);
    let start = switch_thumb(&nodes, rem);
    let (_, nodes) = animated_switch(&mut transitions, &view, true, 0.03);
    let middle = switch_thumb(&nodes, rem);
    let (_, nodes) = animated_switch(&mut transitions, &view, true, 0.249);
    let last_step = switch_thumb(&nodes, rem);
    let (_, nodes) = animated_switch(&mut transitions, &view, true, 0.250);
    let settled = switch_thumb(&nodes, rem);
    assert!((start[0] - 100.0).abs() < 0.001);
    assert!(middle[0] > start[0] && middle[0] < settled[0]);
    assert_eq!(last_step, settled);
    let (_, unchanged) = animated_switch(&mut transitions, &view, true, 500.0);
    assert_eq!(
        nodes, unchanged,
        "settled clocks preserve the retained scene fast path"
    );
}

#[test]
fn moving_switch_reveals_both_permanent_rail_glyphs() {
    let mut transitions = Transitions::default();
    let mut view = MenuView::new(true, "Player".into());
    animated_switch(&mut transitions, &view, false, -1.0);
    view.settings_control_activation = Some((MenuAction::SettingsOption(7, 1), 1));
    let (rem, nodes) = animated_switch(&mut transitions, &view, true, 0.0);
    let point = [100.0 + 4.5 * rem, 100.0 + 1.05 * rem];
    let visible = solids(&nodes)
        .into_iter()
        .rev()
        .find_map(|(bounds, color)| {
            (point[0] >= bounds[0]
                && point[0] < bounds[2]
                && point[1] >= bounds[1]
                && point[1] < bounds[3])
                .then_some(color)
        });
    assert_eq!(visible, Some([36, 36, 37, 255]));
    view.settings_control_activation = Some((MenuAction::SettingsOption(7, 0), 2));
    let (_, nodes) = animated_switch(&mut transitions, &view, false, 0.4);
    assert!(
        solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::TEXT)
    );
}

#[test]
fn switch_navigation_press_keeps_identity_after_its_next_value_action_changes() {
    let mut transitions = Transitions::default();
    let mut view = MenuView::new(true, "Player".into());
    animated_switch(&mut transitions, &view, false, -1.0);
    view.settings_control_activation = Some((MenuAction::SettingsOption(7, 1), 1));
    view.settings_control_activation_navigation = true;
    view.hovered = Some(MenuAction::SettingsOption(7, 1));
    animated_switch(&mut transitions, &view, true, 0.0);
    assert!(transitions.pressed_switch(MenuAction::SettingsOption(7, 0), 0.0));
    let (_, nodes) = animated_switch(&mut transitions, &view, true, 0.05);
    assert!(
        solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.pressed)
    );
    view.hovered = None;
    let (_, nodes) = animated_switch(&mut transitions, &view, true, 0.1);
    assert!(
        solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.pressed)
    );
    animated_switch(&mut transitions, &view, true, 0.1501);
    assert!(!transitions.pressed_switch(MenuAction::SettingsOption(7, 0), 0.1501));
    let (_, nodes) = animated_switch(&mut transitions, &view, true, 0.24);
    assert!(
        !solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.pressed)
    );
}

#[test]
fn slider_drag_geometry_retains_full_endpoints_and_clips_only_its_animated_thumb() {
    let view = MenuView::new(true, "Player".into());
    paint(Default::default(), |canvas| {
        let viewport = canvas
            .begin_scroll("clip", [120.0, 100.0, 250.0, 135.0])
            .unwrap();
        let half = canvas.r(1.6);
        let actions = [
            MenuAction::SettingsOption(7, 0),
            MenuAction::SettingsOption(7, 1),
        ];
        slider(
            canvas,
            &view,
            [100.0, 90.0, 300.0, 140.0],
            &actions,
            0,
            false,
        )
        .unwrap();
        let (index, track, thumb) = canvas.slider_tracks[0];
        assert_eq!(index, 7);
        assert!((track.min().x() - 100.0 - half).abs() < 0.001);
        assert!((track.max().x() - 300.0 + half).abs() < 0.001);
        let thumb = thumb.unwrap();
        assert_eq!(thumb.min().x(), 120.0);
        assert_eq!(thumb.min().y(), 100.0);
        assert_eq!(thumb.max().y(), 135.0);
        canvas.end_scroll(viewport, 140.0).unwrap();
    });
}

#[test]
fn selected_slider_uses_hover_face_without_a_pressed_face() {
    use super::super::super::review_tests::{paint, solids};
    let index = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "gamma")
        .unwrap() as u16;
    let actions = [0, 1, 2].map(|value| MenuAction::SettingsOption(index, value));
    let mut view = MenuView::new(true, "Player".into());
    view.settings_slider_selected = Some(index);
    view.pressed = Some(actions[1]);
    let (_, _, nodes) = paint(Default::default(), |canvas| {
        slider(
            canvas,
            &view,
            [100.0, 100.0, 500.0, 140.0],
            &actions,
            1,
            false,
        )
        .unwrap();
    });
    let colors = solids(&nodes);
    assert!(
        colors
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.hovered)
    );
    assert!(
        colors
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.specular_hovered[0]),
        "selection uses the hovered bevel even while Select is pressed"
    );
    assert!(
        !colors
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.specular[1]),
        "the equal hovered/pressed fill colours cannot distinguish their bevels"
    );
}

#[test]
fn rail_hover_does_not_highlight_the_thumb_but_thumb_hover_does() {
    let actions = [0, 1, 2].map(|value| MenuAction::SettingsOption(7, value));
    let mut view = MenuView::new(true, "Player".into());
    view.hovered = Some(actions[2]);
    let draw = |view: &MenuView| {
        paint(Default::default(), |canvas| {
            slider(
                canvas,
                view,
                [100.0, 100.0, 500.0, 140.0],
                &actions,
                1,
                false,
            )
            .unwrap();
        })
        .2
    };
    let nodes = draw(&view);
    assert!(
        !solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.hovered)
    );
    view.settings_slider_hovered = Some(7);
    let nodes = draw(&view);
    assert!(
        solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.hovered)
    );
    view.settings_slider_pointer = Some(launcher::menu::view::SettingsSliderPointer {
        option: 8,
        fraction: 0.5,
        mouse_input: true,
    });
    let nodes = draw(&view);
    assert!(
        !solids(&nodes)
            .iter()
            .any(|(_, color)| *color == theme::SECONDARY.hovered)
    );
}

#[test]
fn slider_registers_one_inset_focus_anchor_independent_of_pointer_steps() {
    let view = MenuView::new(true, "Player".into());
    let actions = [0, 1, 2].map(|value| MenuAction::SettingsOption(7, value));
    paint(Default::default(), |canvas| {
        canvas.capture_focus = true;
        slider(
            canvas,
            &view,
            [100.0, 100.0, 500.0, 140.0],
            &actions,
            1,
            false,
        )
        .unwrap();
        assert_eq!(canvas.focus_targets.len(), 1);
        assert_eq!(canvas.focus_targets[0].action, actions[1]);
        assert_eq!(canvas.focus_targets[0].bounds, canvas.slider_tracks[0].1);
        assert_eq!(canvas.focus_targets[0].bounds.height(), 40.0);
        assert!(canvas.focus_targets[0].bounds.min().x() > 100.0);
        assert!(canvas.focus_targets[0].bounds.max().x() < 500.0);
        assert_eq!(canvas.hits.len(), actions.len());
        assert!(canvas.capture_focus);
    });
}

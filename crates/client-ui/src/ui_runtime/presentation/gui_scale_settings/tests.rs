use super::*;
use crate::ui_runtime::presentation::{rect, tests::fixture_font};

fn point(x: f32, y: f32) -> ui::UiPoint {
    ui::UiPoint::new(x, y).unwrap()
}

#[test]
fn native_drag_fraction_uses_the_full_track_and_keeps_unsnapped_values() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.form_presentation.oreui_slider_tracks =
        vec![(7, rect(100.0, 200.0, 300.0, 230.0).unwrap(), None)];
    assert_eq!(
        presentation.settings_slider_drag_fraction(7, point(174.0, -100.0)),
        Some(0.37)
    );
    assert_eq!(
        presentation.settings_slider_drag_fraction(7, point(-100.0, 0.0)),
        Some(0.0)
    );
    assert_eq!(
        presentation.settings_slider_drag_fraction(7, point(400.0, 1000.0)),
        Some(1.0)
    );
    assert_eq!(
        presentation.settings_slider_drag_fraction(9, point(174.0, 210.0)),
        None
    );
}

#[test]
fn only_visible_animated_thumb_pixels_capture_including_overshoot() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.form_presentation.oreui_slider_tracks = vec![
        (
            7,
            rect(100.0, 200.0, 300.0, 230.0).unwrap(),
            Some(rect(300.0, 210.0, 330.0, 230.0).unwrap()),
        ),
        (9, rect(100.0, 300.0, 300.0, 330.0).unwrap(), None),
    ];
    let overshoot = point(320.0, 220.0);
    assert!(presentation.settings_slider_thumb_contains(7, overshoot));
    assert_eq!(
        presentation.settings_slider_thumb_hit_test(overshoot),
        Some(7)
    );
    assert!(!presentation.settings_slider_thumb_contains(7, point(200.0, 220.0)));
    assert!(!presentation.settings_slider_thumb_contains(7, point(320.0, 205.0)));
    assert_eq!(
        presentation.settings_slider_thumb_hit_test(point(200.0, 320.0)),
        None
    );
}

#[test]
fn new_form_frame_clears_native_input_ownership_and_slider_capture() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.form_presentation.oreui_settings_input = true;
    presentation.form_presentation.oreui_slider_tracks.push((
        7,
        rect(100.0, 200.0, 300.0, 230.0).unwrap(),
        Some(rect(150.0, 200.0, 180.0, 230.0).unwrap()),
    ));
    assert!(presentation.uses_oreui_settings());
    assert_eq!(
        presentation.settings_slider_thumb_hit_test(point(160.0, 210.0)),
        Some(7)
    );
    presentation.begin_form_frame();
    assert!(!presentation.uses_oreui_settings());
    assert_eq!(
        presentation.settings_slider_thumb_hit_test(point(160.0, 210.0)),
        None
    );
    assert_eq!(
        presentation.settings_slider_drag_fraction(7, point(160.0, 210.0)),
        None
    );
}

use super::tests::{area, point};
use super::*;

fn smooth() -> MenuScrolls {
    let mut scrolls = MenuScrolls::default();
    scrolls.begin_frame("oreui".into());
    scrolls.set_areas(vec![area()]);
    scrolls.configure_motion(true, 0.0);
    scrolls
}

#[test]
fn oreui_wheel_accumulates_targets_and_reverses_without_losing_input_or_jumping() {
    let mut scrolls = smooth();
    let point = point(50.0, 50.0);
    assert!(scrolls.wheel(point, -2.0, false));
    assert_eq!(scrolls.offsets()["list"], 0.0);
    scrolls.wheel(point, -2.0, false);
    scrolls.configure_motion(true, 0.03);
    let middle = scrolls.offsets()["list"];
    assert!(middle > 20.0 && middle < 40.0);
    scrolls.wheel(point, 1.0, false);
    assert_eq!(scrolls.offsets()["list"], middle);
    scrolls.configure_motion(true, 0.2);
    assert_eq!(scrolls.offsets()["list"], 30.0);
    assert!(scrolls.motion.is_empty());
}

#[test]
fn oreui_scroll_motion_survives_redraw_and_clamps_when_content_shrinks() {
    let mut scrolls = smooth();
    scrolls.wheel(point(50.0, 50.0), -20.0, false);
    scrolls.clear_areas();
    scrolls.configure_motion(true, 0.03);
    let mut next_area = area();
    next_area.offset = scrolls.offsets()["list"];
    scrolls.set_areas(vec![next_area]);
    assert!(!scrolls.motion.is_empty());
    let mut smaller = area();
    smaller.max = 10.0;
    scrolls.set_areas(vec![smaller]);
    scrolls.clear_areas();
    scrolls.configure_motion(true, 0.04);
    assert!(scrolls.offsets()["list"] <= 10.0);
    scrolls.configure_motion(true, 0.2);
    assert_eq!(scrolls.offsets()["list"], 10.0);
}

#[test]
fn oreui_pixel_wheel_drags_and_disabled_motion_stay_responsive() {
    let mut scrolls = smooth();
    scrolls.wheel(point(50.0, 50.0), -20.0, true);
    scrolls.configure_motion(true, 0.04);
    assert_eq!(scrolls.offsets()["list"], 10.0);
    scrolls.wheel(point(50.0, 50.0), -2.0, false);
    scrolls.configure_motion(false, 0.05);
    assert_eq!(scrolls.offsets()["list"], 30.0);
    scrolls.configure_motion(true, 0.1);
    scrolls.wheel(point(50.0, 50.0), -2.0, false);
    assert!(scrolls.press(point(97.0, 80.0)));
    let track = scrolls.offsets()["list"];
    assert!(scrolls.motion.is_empty());
    scrolls.configure_motion(true, 0.3);
    assert_eq!(scrolls.offsets()["list"], track);
    scrolls.set_areas(vec![area()]);
    scrolls.press(point(97.0, 10.0));
    scrolls.drag(Some(point(97.0, 85.0)), true);
    assert_eq!(scrolls.offsets()["list"], 150.0);
    assert!(scrolls.motion.is_empty());
    scrolls.begin_frame("another screen".into());
    assert!(scrolls.offsets().is_empty());
}

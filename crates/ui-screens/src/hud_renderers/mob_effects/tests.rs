use super::*;

/// The top band grows by ten GUI px per status row beyond two.
#[test]
fn extra_status_rows_lower_the_column() {
    let control = [0.0, 0.0, 1280.0, 540.0];
    let screen = [0.0, 0.0, 1280.0, 720.0];
    let first = |rows| icon_rects(1, control, screen, 2.0, rows).next().unwrap();
    assert_eq!(first(0), first(2));
    assert_eq!(first(4)[1] - first(2)[1], 40.0);
}

//! Client-owned wheel speed stays independent while vanilla keeps its first-view latch.

use json_ui::ScrollMetrics;

/// Owned views honor their own speed without changing vanilla's normalization or sensitivity.
#[test]
fn local_wheel_speed_does_not_read_or_replace_the_vanilla_latch() {
    let mut metrics = ScrollMetrics {
        content: 1000.,
        viewport: 100.,
        speed: 1.,
        horizontal: true,
        ..Default::default()
    };
    assert_eq!(metrics.offset_for_wheel(-1.), 0.);
    metrics.horizontal = false;
    metrics.speed = 2.;
    let vanilla = metrics.offset_for_wheel(-1.);
    assert_eq!(vanilla, 2. * 120. / 128.);
    metrics.speed = 24.;
    assert_eq!(metrics.offset_for_wheel(-1.), vanilla);
    assert_eq!(metrics.offset_for_wheel_local(-1.), 24. * 120. / 128.);
    assert_eq!(metrics.offset_for_wheel_local(-1000.), metrics.max_offset());
    metrics.offset = 30.;
    assert_eq!(metrics.offset_for_wheel_local(1000.), 0.);
}

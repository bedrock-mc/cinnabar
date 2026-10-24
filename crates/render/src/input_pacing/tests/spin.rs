//! The focused spin covers recent sleep overshoot without letting rare hiccups hold it.

use super::*;

fn us(micros: u64) -> Duration {
    Duration::from_micros(micros)
}

/// Records `count` copies of `overshoot`.
fn record(estimate: &mut SpinEstimate, overshoot: Duration, count: usize) {
    for _ in 0..count {
        estimate.record(overshoot);
    }
}

#[test]
fn first_wakes_cover_every_overshoot_seen() {
    let mut estimate = SpinEstimate::default();
    assert_eq!(estimate.spin, INITIAL_SPIN);
    estimate.record(us(300));
    estimate.record(us(10));
    assert_eq!(estimate.spin, us(300) + SPIN_HEADROOM);
}

#[test]
fn rare_late_wakes_do_not_hold_a_long_spin() {
    let mut estimate = SpinEstimate::default();
    record(
        &mut estimate,
        us(20),
        OVERSHOOT_WINDOW - UNCOVERED_OVERSHOOTS,
    );
    record(&mut estimate, us(450), UNCOVERED_OVERSHOOTS);
    assert_eq!(estimate.spin, MIN_SPIN);
}

#[test]
fn a_burst_of_late_wakes_raises_the_spin_until_it_leaves_the_window() {
    let burst = UNCOVERED_OVERSHOOTS + 1;
    let mut estimate = SpinEstimate::default();
    record(&mut estimate, us(20), OVERSHOOT_WINDOW);
    record(&mut estimate, us(300), burst);
    assert_eq!(estimate.spin, us(300) + SPIN_HEADROOM);
    record(&mut estimate, us(20), OVERSHOOT_WINDOW - burst);
    assert_eq!(
        estimate.spin,
        us(300) + SPIN_HEADROOM,
        "the whole burst is still in the window"
    );
    estimate.record(us(20));
    assert_eq!(estimate.spin, MIN_SPIN);
}

#[test]
fn coarse_timer_granularity_keeps_the_full_spin() {
    let mut estimate = SpinEstimate::default();
    // Wakes rounded up to a 0.5 ms timer tick overshoot evenly across the tick.
    for index in 0..OVERSHOOT_WINDOW as u64 {
        estimate.record(us(index * 500 / OVERSHOOT_WINDOW as u64));
    }
    assert_eq!(estimate.spin, MAX_SPIN);
    record(&mut estimate, Duration::from_millis(2), OVERSHOOT_WINDOW);
    assert_eq!(estimate.spin, MAX_SPIN);
}

use super::*;

const SECOND: u64 = 1_000_000_000;

fn rate(millihertz: u32) -> FrameRate {
    FrameRate::from_millihertz(millihertz).unwrap()
}

/// Display and cap rates, including NTSC's 60000/1001 Hz.
const RATES: [u32; 6] = [30_000, 60_000, 59_940, 120_000, 144_000, 240_000];

#[test]
fn slots_follow_the_exact_ratio_over_millions_of_frames_without_drift() {
    for millihertz in RATES {
        let cadence = Cadence::new(rate(millihertz), 7);
        let exact = |slot: u64| 7 + u128::from(slot) * 1_000_000_000_000 / u128::from(millihertz);
        let mut previous = cadence.slot_nanos(0);
        for slot in 1..2_000_000u64 {
            let at = cadence.slot_nanos(slot);
            let step = at - previous;
            let period = rate(millihertz).period_nanos();
            assert!(
                step == period || step == period + 1,
                "{millihertz} mHz slot {slot}"
            );
            previous = at;
        }
        let far = 10_000_000_000u64;
        assert_eq!(u128::from(cadence.slot_nanos(far)), exact(far));
    }
}

/// Simulates a frame loop that samples at each deadline plus `lateness(frame)`.
fn admitted(millihertz: u32, frames: usize, lateness: impl Fn(usize) -> u64) -> Vec<u64> {
    let mut cadence = Cadence::new(rate(millihertz), SECOND);
    let mut samples = Vec::with_capacity(frames);
    for frame in 0..frames {
        let sampled = cadence.next_admission_nanos() + lateness(frame);
        cadence.admit(sampled);
        samples.push(sampled);
    }
    samples
}

#[test]
fn on_time_frames_take_every_slot_and_keep_the_epoch_phase() {
    for millihertz in RATES {
        let samples = admitted(millihertz, 100_000, |_| 0);
        let cadence = Cadence::new(rate(millihertz), SECOND);
        for (slot, sampled) in samples.iter().enumerate() {
            assert_eq!(*sampled, cadence.slot_nanos(slot as u64));
        }
    }
}

#[test]
fn slightly_late_wakes_recover_phase_without_skipping_a_slot() {
    let period = rate(120_000).period_nanos();
    let samples = admitted(
        120_000,
        1_000,
        |frame| if frame % 7 == 3 { period / 3 } else { 0 },
    );
    let cadence = Cadence::new(rate(120_000), SECOND);
    for (slot, sampled) in samples.iter().enumerate() {
        let opened = cadence.slot_nanos(slot as u64);
        assert!(
            *sampled >= opened && *sampled - opened <= period / 3,
            "slot {slot}"
        );
    }
}

/// A hitch restarts the cadence one period after the late frame: no catch-up burst, and no
/// skipped slot that would stretch the following interval.
#[test]
fn late_frames_restart_the_cadence_instead_of_bursting() {
    let period = rate(60_000).period_nanos();
    for stall in [period * 6 / 10, period * 3, SECOND * 5, SECOND * 86_400] {
        let samples = admitted(60_000, 64, |frame| if frame == 10 { stall } else { 0 });
        for pair in samples.windows(2) {
            assert!(pair[1] - pair[0] >= period / 2, "stall {stall}: {pair:?}");
        }
        let after = samples[11] - samples[10];
        assert!(
            after == period || after == period + 1,
            "stall {stall}: {after}"
        );
    }
}

#[test]
fn each_slot_admits_at_most_one_frame_however_often_admission_is_asked() {
    let mut cadence = Cadence::new(rate(30_000), 0);
    let first = cadence.next_admission_nanos();
    cadence.admit(first);
    // Frames that sample early, as an input-woken update would, still move one slot each.
    for _ in 0..1_000 {
        let before = cadence.next_admission_nanos();
        cadence.admit(0);
        let step = cadence.next_admission_nanos() - before;
        let period = cadence.rate().period_nanos();
        assert!(step == period || step == period + 1);
    }
}

#[test]
fn background_states_cap_the_requested_rate() {
    let fast = FrameRate::from_hz(240);
    let slow = FrameRate::from_hz(10);
    assert_eq!(effective_frame_rate(fast, WindowActivity::Focused), fast);
    assert_eq!(effective_frame_rate(None, WindowActivity::Focused), None);
    assert_eq!(
        effective_frame_rate(fast, WindowActivity::Unfocused),
        Some(UNFOCUSED_FRAME_RATE)
    );
    assert_eq!(
        effective_frame_rate(None, WindowActivity::Occluded),
        Some(OCCLUDED_FRAME_RATE)
    );
    assert_eq!(effective_frame_rate(slow, WindowActivity::Unfocused), slow);
    assert!(OCCLUDED_FRAME_RATE < UNFOCUSED_FRAME_RATE);
}

#[test]
fn rates_reject_zero_and_saturate_instead_of_overflowing() {
    assert_eq!(FrameRate::from_hz(0), None);
    assert_eq!(FrameRate::from_millihertz(0), None);
    assert_eq!(FrameRate::from_hz(u32::MAX).unwrap().millihertz(), u32::MAX);
    let cadence = Cadence::new(rate(1), u64::MAX - 5);
    assert_eq!(cadence.slot_nanos(u64::MAX), u64::MAX);
}

use std::num::NonZeroU16;

use super::*;
use FrameRateLimit::{Automatic, Fixed, Unlimited};
use PresentModeKind::{Fifo, FifoRelaxed, Immediate, Mailbox};
use PresentationIntent::{AllowTearing, LowLatency, Synchronized};

const INTENTS: [PresentationIntent; 3] = [Synchronized, LowLatency, AllowTearing];

fn modes(list: &[PresentModeKind]) -> SurfacePresentModes {
    list.iter().copied().collect()
}

fn fixed(fps: u16) -> FrameRateLimit {
    Fixed(NonZeroU16::new(fps).unwrap())
}

fn hz(rate: u32) -> FrameRate {
    FrameRate::from_hz(rate).unwrap()
}

fn display(refresh: u32, vrr: VrrStatus) -> DisplayTiming {
    DisplayTiming {
        refresh: FrameRate::from_hz(refresh),
        vrr,
    }
}

/// Every advertised subset, FIFO always included.
fn all_surfaces() -> impl Iterator<Item = SurfacePresentModes> {
    (0u8..16).map(SurfacePresentModes::from_bits)
}

fn all_limits() -> [FrameRateLimit; 5] {
    [Automatic, Unlimited, fixed(30), fixed(120), fixed(240)]
}

#[test]
fn selection_table_matches_backend_capability_sets() {
    let metal = modes(&[Fifo, Immediate]);
    let dx12_tearing = modes(&[Fifo, Mailbox, Immediate]);
    let dx12 = modes(&[Fifo, Mailbox]);
    let fifo_only = SurfacePresentModes::FIFO_ONLY;
    let fixed_120 = display(120, VrrStatus::Unknown);
    for (intent, limit, surface, expected) in [
        (Synchronized, Unlimited, dx12_tearing, Fifo),
        (Synchronized, Automatic, metal, Fifo),
        (LowLatency, Automatic, dx12_tearing, Fifo),
        (LowLatency, Automatic, metal, Fifo),
        (LowLatency, fixed(120), dx12, Fifo),
        (LowLatency, fixed(240), dx12, Mailbox),
        (LowLatency, Unlimited, dx12_tearing, Mailbox),
        (LowLatency, Unlimited, metal, Fifo),
        (AllowTearing, Automatic, metal, Immediate),
        (AllowTearing, Automatic, dx12_tearing, Immediate),
        (AllowTearing, Automatic, dx12, Mailbox),
        (AllowTearing, Unlimited, modes(&[Fifo, FifoRelaxed]), Fifo),
        (AllowTearing, Automatic, fifo_only, Fifo),
    ] {
        assert_eq!(
            select_present_mode(intent, limit, fixed_120, surface),
            expected,
            "{intent:?} {limit:?} on {surface:?}"
        );
    }
}

/// Without tearing, Immediate is never chosen whatever the surface, limit or display.
#[test]
fn only_an_explicit_tearing_intent_selects_immediate() {
    for surface in all_surfaces() {
        for limit in all_limits() {
            for vrr in [VrrStatus::Active, VrrStatus::Unknown] {
                for intent in [Synchronized, LowLatency] {
                    let mode = select_present_mode(intent, limit, display(60, vrr), surface);
                    assert_ne!(mode, Immediate, "{intent:?} {limit:?} {surface:?}");
                }
            }
        }
    }
}

#[test]
fn selection_never_requests_an_unadvertised_mode_or_relies_on_fallback() {
    for surface in all_surfaces() {
        for limit in all_limits() {
            for intent in INTENTS {
                let selected =
                    select_present_mode(intent, limit, DisplayTiming::default(), surface);
                assert!(surface.contains(selected), "{intent:?} on {surface:?}");
                assert_eq!(configured_present_mode(selected, surface), selected);
            }
        }
    }
}

#[test]
fn unprobed_requests_stay_tear_free_unless_tearing_is_allowed() {
    assert_eq!(initial_present_mode(Synchronized), Fifo);
    assert_eq!(initial_present_mode(LowLatency), Fifo);
    assert_eq!(initial_present_mode(AllowTearing), Immediate);
    let fifo_only = SurfacePresentModes::FIFO_ONLY;
    assert_eq!(configured_present_mode(Immediate, fifo_only), Fifo);
    assert_eq!(
        configured_present_mode(Mailbox, modes(&[Immediate])),
        Immediate
    );
    assert_eq!(configured_present_mode(FifoRelaxed, fifo_only), Fifo);
}

#[test]
fn a_limit_outpaces_the_display_only_above_its_refresh() {
    let known = display(144, VrrStatus::Unknown);
    assert!(!outpaces_display(Automatic, known));
    assert!(outpaces_display(Unlimited, known));
    assert!(!outpaces_display(fixed(144), known));
    assert!(outpaces_display(fixed(145), known));
    assert!(outpaces_display(fixed(60), DisplayTiming::default()));
}

#[test]
fn automatic_rates_follow_the_intent_and_display() {
    let fixed_120 = display(120, VrrStatus::Unknown);
    assert_eq!(frame_rate_target(Synchronized, Automatic, fixed_120), None);
    assert_eq!(frame_rate_target(LowLatency, Automatic, fixed_120), None);
    assert_eq!(
        frame_rate_target(AllowTearing, Automatic, fixed_120),
        Some(hz(240))
    );
    assert_eq!(
        frame_rate_target(AllowTearing, Automatic, DisplayTiming::default()),
        Some(hz(240)),
        "an unknown refresh assumes the provisional rate"
    );
    for intent in INTENTS {
        assert_eq!(frame_rate_target(intent, Unlimited, fixed_120), None);
        assert_eq!(
            frame_rate_target(intent, fixed(75), fixed_120),
            Some(hz(75))
        );
    }
}

/// Confirmed variable refresh keeps low-latency frames inside its range; other intents and
/// unconfirmed displays are left alone.
#[test]
fn variable_refresh_caps_low_latency_below_the_maximum() {
    let vrr_120 = display(120, VrrStatus::Active);
    assert_eq!(
        frame_rate_target(LowLatency, Automatic, vrr_120),
        Some(hz(116))
    );
    assert_eq!(
        frame_rate_target(LowLatency, Unlimited, vrr_120),
        Some(hz(116))
    );
    assert_eq!(
        frame_rate_target(LowLatency, fixed(200), vrr_120),
        Some(hz(116))
    );
    assert_eq!(
        frame_rate_target(LowLatency, fixed(90), vrr_120),
        Some(hz(90))
    );
    assert_eq!(frame_rate_target(AllowTearing, Unlimited, vrr_120), None);
    assert_eq!(frame_rate_target(Synchronized, Automatic, vrr_120), None);
    for unconfirmed in [VrrStatus::Inactive, VrrStatus::Unknown] {
        assert_eq!(
            frame_rate_target(LowLatency, Automatic, display(120, unconfirmed)),
            None
        );
    }
}

#[test]
fn variable_refresh_headroom_rounds_down_and_grows_with_completion_error() {
    for (max, ceiling) in [(60, 58), (120, 116), (144, 139), (240, 232)] {
        assert_eq!(vrr_ceiling(hz(max), None), hz(ceiling), "{max} Hz");
        assert_eq!(vrr_ceiling(hz(max), Some(0)), hz(ceiling), "{max} Hz");
    }
    let max = hz(240);
    let mut previous = vrr_ceiling(max, Some(0));
    for error_micros in [100, 250, 500, 1_000, 2_000] {
        let ceiling = vrr_ceiling(max, Some(error_micros * 1_000));
        assert!(ceiling <= previous, "{error_micros} µs");
        let period = 1_000_000_000_000 / u64::from(ceiling.millihertz());
        assert!(period >= max.period_nanos() + error_micros * 1_000 + 100_000);
        previous = ceiling;
    }
}

#[test]
fn capability_bits_round_trip_and_always_include_fifo() {
    for surface in all_surfaces() {
        assert_eq!(SurfacePresentModes::from_bits(surface.bits()), surface);
        assert!(surface.contains(Fifo));
    }
    assert_eq!(
        SurfacePresentModes::from_bits(0xF0),
        SurfacePresentModes::FIFO_ONLY
    );
}

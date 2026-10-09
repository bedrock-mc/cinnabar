use std::num::NonZeroU16;

use super::*;
use FrameRateLimit::{Automatic, Fixed, Unlimited};
use PresentModeKind::{Fifo, FifoRelaxed, Immediate, Mailbox};
use PresentationIntent::{LowLatency, Synchronized, Unpaced};

const INTENTS: [PresentationIntent; 3] = [Synchronized, LowLatency, Unpaced];

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

#[test]
fn selection_table_matches_backend_capability_sets() {
    let metal = modes(&[Fifo, Immediate]);
    let dx12_tearing = modes(&[Fifo, Mailbox, Immediate]);
    let dx12 = modes(&[Fifo, Mailbox]);
    let fifo_only = SurfacePresentModes::FIFO_ONLY;
    for (intent, surface, expected) in [
        (Synchronized, dx12_tearing, Fifo),
        (Synchronized, metal, Fifo),
        (LowLatency, dx12_tearing, Immediate),
        (LowLatency, metal, Immediate),
        (LowLatency, dx12, Mailbox),
        (LowLatency, fifo_only, Fifo),
        (Unpaced, metal, Immediate),
        (Unpaced, dx12, Mailbox),
        (Unpaced, modes(&[Fifo, FifoRelaxed]), Fifo),
    ] {
        assert_eq!(
            select_present_mode(intent, surface),
            expected,
            "{intent:?} on {surface:?}"
        );
    }
}

/// Synchronized presentation never tears; VSync off prefers Immediate whenever advertised.
#[test]
fn presentation_intents_respect_the_tearing_boundary() {
    for surface in all_surfaces() {
        assert_ne!(
            select_present_mode(Synchronized, surface),
            Immediate,
            "{surface:?}"
        );
        assert_eq!(
            select_present_mode(LowLatency, surface) == Immediate,
            surface.contains(Immediate),
            "{surface:?}"
        );
    }
}

#[test]
fn selection_never_requests_an_unadvertised_mode_or_relies_on_fallback() {
    for surface in all_surfaces() {
        for intent in INTENTS {
            let selected = select_present_mode(intent, surface);
            assert!(surface.contains(selected), "{intent:?} on {surface:?}");
            assert_eq!(configured_present_mode(selected, surface), selected);
        }
    }
}

#[test]
fn unprobed_player_requests_start_on_fifo() {
    assert_eq!(initial_present_mode(Synchronized), Fifo);
    assert_eq!(initial_present_mode(LowLatency), Fifo);
    assert_eq!(initial_present_mode(Unpaced), Immediate);
    let fifo_only = SurfacePresentModes::FIFO_ONLY;
    assert_eq!(configured_present_mode(Immediate, fifo_only), Fifo);
    assert_eq!(
        configured_present_mode(Mailbox, modes(&[Immediate])),
        Immediate
    );
    assert_eq!(configured_present_mode(FifoRelaxed, fifo_only), Fifo);
}

/// Automatic and unlimited add no application cadence; presentation or rendering determines it.
#[test]
fn automatic_and_unlimited_leave_pacing_to_the_display_or_rendering() {
    let fixed_120 = display(120, VrrStatus::Unknown);
    for intent in INTENTS {
        assert_eq!(frame_rate_target(intent, Automatic, fixed_120), None);
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
    assert_eq!(frame_rate_target(Unpaced, Unlimited, vrr_120), None);
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

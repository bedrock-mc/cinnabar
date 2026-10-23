use std::num::NonZeroU16;

use super::*;
use FrameRateLimit::{Fixed, Unlimited};
use PresentModeKind::{Fifo, FifoRelaxed, Immediate, Mailbox};
use PresentationIntent::{AllowTearing, LowLatency, Synchronized, Unpaced};

const GAMEPLAY: FrameRateContext = FrameRateContext::Gameplay { on_battery: false };

const INTENTS: [PresentationIntent; 4] = [Synchronized, LowLatency, AllowTearing, Unpaced];

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
        (LowLatency, dx12_tearing, Mailbox),
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

/// Low latency prefers Mailbox, while the diagnostic launch flag prefers Immediate.
#[test]
fn presentation_intents_respect_the_tearing_boundary() {
    for surface in all_surfaces() {
        assert_eq!(
            select_present_mode(Synchronized, surface),
            Fifo,
            "{surface:?}"
        );
        assert_eq!(
            select_present_mode(LowLatency, surface) == Immediate,
            surface.contains(Immediate) && !surface.contains(Mailbox),
            "{surface:?}"
        );
        assert_eq!(
            select_present_mode(AllowTearing, surface) == Immediate,
            surface.contains(Immediate)
        );
        if surface.contains(Mailbox) {
            assert_eq!(select_present_mode(LowLatency, surface), Mailbox);
        }
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
    assert_eq!(initial_present_mode(AllowTearing), Fifo);
    assert_eq!(initial_present_mode(Unpaced), Immediate);
    let fifo_only = SurfacePresentModes::FIFO_ONLY;
    assert_eq!(configured_present_mode(Immediate, fifo_only), Fifo);
    assert_eq!(
        configured_present_mode(Mailbox, modes(&[Immediate])),
        Immediate
    );
    assert_eq!(configured_present_mode(FifoRelaxed, fifo_only), Fifo);
}

#[test]
fn menus_bound_both_vsync_choices_by_the_display_and_retain_lower_player_caps() {
    for intent in INTENTS {
        let timing = display(144, VrrStatus::Inactive);
        for limit in [FrameRateLimit::Automatic, Unlimited, fixed(240)] {
            assert_eq!(
                frame_rate_target(intent, limit, timing, FrameRateContext::Menus),
                timing.refresh
            );
        }
        assert_eq!(
            frame_rate_target(intent, fixed(30), timing, FrameRateContext::Menus),
            Some(hz(30))
        );
        assert_eq!(
            frame_rate_target(
                intent,
                Unlimited,
                DisplayTiming::default(),
                FrameRateContext::Menus
            ),
            None
        );
    }
}

#[test]
fn only_the_untouched_gameplay_default_changes_on_battery() {
    for intent in INTENTS {
        let timing = display(120, VrrStatus::Inactive);
        for on_battery in [false, true] {
            let context = FrameRateContext::Gameplay { on_battery };
            assert_eq!(
                frame_rate_target(intent, FrameRateLimit::Automatic, timing, context),
                on_battery.then_some(timing.refresh).flatten()
            );
            assert_eq!(frame_rate_target(intent, Unlimited, timing, context), None);
            assert_eq!(
                frame_rate_target(intent, fixed(75), timing, context),
                Some(hz(75))
            );
        }
    }
}

#[test]
fn the_vrr_ceiling_survives_menu_and_battery_defaults() {
    let timing = display(144, VrrStatus::Active);
    for intent in [LowLatency, AllowTearing] {
        for context in [
            FrameRateContext::Menus,
            GAMEPLAY,
            FrameRateContext::Gameplay { on_battery: true },
        ] {
            for limit in [FrameRateLimit::Automatic, Unlimited, fixed(240)] {
                assert_eq!(
                    frame_rate_target(intent, limit, timing, context),
                    Some(vrr_ceiling(timing.refresh.unwrap(), None))
                );
            }
        }
    }
}

/// Unlimited adds no application cadence; presentation or rendering determines it.
#[test]
fn unlimited_leaves_pacing_to_the_display_or_rendering() {
    let fixed_120 = display(120, VrrStatus::Unknown);
    for intent in INTENTS {
        assert_eq!(
            frame_rate_target(intent, Unlimited, fixed_120, GAMEPLAY),
            None
        );
        assert_eq!(
            frame_rate_target(intent, fixed(75), fixed_120, GAMEPLAY),
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
        frame_rate_target(LowLatency, Unlimited, vrr_120, GAMEPLAY),
        Some(hz(116))
    );
    assert_eq!(
        frame_rate_target(LowLatency, fixed(200), vrr_120, GAMEPLAY),
        Some(hz(116))
    );
    assert_eq!(
        frame_rate_target(LowLatency, fixed(90), vrr_120, GAMEPLAY),
        Some(hz(90))
    );
    assert_eq!(
        frame_rate_target(Unpaced, Unlimited, vrr_120, GAMEPLAY),
        None
    );
    assert_eq!(
        frame_rate_target(Synchronized, Unlimited, vrr_120, GAMEPLAY),
        None
    );
    for unconfirmed in [VrrStatus::Inactive, VrrStatus::Unknown] {
        assert_eq!(
            frame_rate_target(LowLatency, Unlimited, display(120, unconfirmed), GAMEPLAY),
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

#[test]
fn vrr_preference_preserves_refresh_and_resolves_every_reported_state() {
    use render_api::VrrPreference;
    for status in [VrrStatus::Active, VrrStatus::Inactive, VrrStatus::Unknown] {
        for refresh in [None, FrameRate::from_hz(120)] {
            let timing = DisplayTiming {
                refresh,
                vrr: status,
            };
            for (preference, expected) in [
                (VrrPreference::Automatic, status),
                (VrrPreference::On, VrrStatus::Active),
                (VrrPreference::Off, VrrStatus::Inactive),
            ] {
                let selected = timing.with_vrr_preference(preference);
                assert_eq!(
                    selected,
                    DisplayTiming {
                        refresh,
                        vrr: expected
                    }
                );
                assert_eq!(
                    frame_rate_target(LowLatency, Unlimited, selected, GAMEPLAY),
                    if expected == VrrStatus::Active && refresh.is_some() {
                        Some(hz(116))
                    } else {
                        None
                    }
                );
            }
        }
    }
}

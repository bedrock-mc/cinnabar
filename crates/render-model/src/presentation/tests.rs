use super::*;
use PresentModeKind::{Fifo, FifoRelaxed, Immediate, Mailbox};
use PresentationIntent::{LowLatency, Synchronized};

fn modes(list: &[PresentModeKind]) -> SurfacePresentModes {
    list.iter().copied().collect()
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
        (Synchronized, metal, Fifo),
        (Synchronized, dx12_tearing, Fifo),
        (Synchronized, dx12, Fifo),
        (Synchronized, fifo_only, Fifo),
        (LowLatency, metal, Immediate),
        (LowLatency, dx12_tearing, Immediate),
        (LowLatency, dx12, Mailbox),
        (LowLatency, modes(&[Fifo, FifoRelaxed]), Fifo),
        (LowLatency, fifo_only, Fifo),
    ] {
        assert_eq!(
            select_present_mode(intent, surface),
            expected,
            "{intent:?} on {surface:?}"
        );
    }
}

#[test]
fn selection_never_requests_an_unadvertised_mode_or_relies_on_fallback() {
    for surface in all_surfaces() {
        for intent in [Synchronized, LowLatency] {
            let selected = select_present_mode(intent, surface);
            assert!(surface.contains(selected), "{intent:?} on {surface:?}");
            assert_eq!(configured_present_mode(selected, surface), selected);
        }
    }
}

#[test]
fn unprobed_requests_degrade_to_fifo_through_the_renderer_fallback() {
    assert_eq!(initial_present_mode(Synchronized), Fifo);
    assert_eq!(initial_present_mode(LowLatency), Immediate);
    let fifo_only = SurfacePresentModes::FIFO_ONLY;
    assert_eq!(configured_present_mode(Immediate, fifo_only), Fifo);
    assert_eq!(
        configured_present_mode(Mailbox, modes(&[Immediate])),
        Immediate
    );
    assert_eq!(configured_present_mode(FifoRelaxed, fifo_only), Fifo);
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

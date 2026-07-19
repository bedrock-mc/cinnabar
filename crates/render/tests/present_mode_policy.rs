use bevy::window::PresentMode;
use render::{
    Dx12PresentModePolicy, PresentModePreference, PresentModeRemedy,
    resolve_dx12_present_mode_remedy,
};
use wgpu::{Backend, PresentMode as SurfacePresentMode};

const AFFECTED_ADAPTER: &str = "Radeon RX 570 Series";
const AFFECTED_DRIVER: &str = "31.0.21924.61";
const POLICY_SOURCE: &str = include_str!("../src/present_mode.rs");

#[test]
fn exact_affected_dx12_fifo_path_uses_proven_immediate_mode() {
    assert_eq!(
        resolve_dx12_present_mode_remedy(
            PresentModePreference::Auto,
            Backend::Dx12,
            AFFECTED_ADAPTER,
            AFFECTED_DRIVER,
            PresentMode::Fifo,
            &[SurfacePresentMode::Fifo, SurfacePresentMode::Immediate],
        ),
        PresentModeRemedy::UseImmediate,
    );
}

#[test]
fn policy_does_not_generalize_beyond_the_measured_driver_and_capability() {
    for (backend, adapter, driver, supported) in [
        (
            Backend::Vulkan,
            AFFECTED_ADAPTER,
            AFFECTED_DRIVER,
            &[SurfacePresentMode::Fifo, SurfacePresentMode::Immediate][..],
        ),
        (
            Backend::Dx12,
            "Radeon RX 580 Series",
            AFFECTED_DRIVER,
            &[SurfacePresentMode::Fifo, SurfacePresentMode::Immediate][..],
        ),
        (
            Backend::Dx12,
            AFFECTED_ADAPTER,
            "new-driver",
            &[SurfacePresentMode::Fifo, SurfacePresentMode::Immediate][..],
        ),
        (
            Backend::Dx12,
            AFFECTED_ADAPTER,
            AFFECTED_DRIVER,
            &[SurfacePresentMode::Fifo][..],
        ),
    ] {
        assert_eq!(
            resolve_dx12_present_mode_remedy(
                PresentModePreference::Auto,
                backend,
                adapter,
                driver,
                PresentMode::Fifo,
                supported,
            ),
            PresentModeRemedy::KeepRequested,
        );
    }
}

#[test]
fn explicit_vsync_and_no_vsync_are_never_overridden() {
    let supported = &[SurfacePresentMode::Fifo, SurfacePresentMode::Immediate];
    for (preference, requested) in [
        (PresentModePreference::Vsync, PresentMode::Fifo),
        (PresentModePreference::NoVsync, PresentMode::Immediate),
    ] {
        assert_eq!(
            resolve_dx12_present_mode_remedy(
                preference,
                Backend::Dx12,
                AFFECTED_ADAPTER,
                AFFECTED_DRIVER,
                requested,
                supported,
            ),
            PresentModeRemedy::KeepRequested,
        );
    }
    assert_eq!(
        resolve_dx12_present_mode_remedy(
            PresentModePreference::Auto,
            Backend::Dx12,
            AFFECTED_ADAPTER,
            AFFECTED_DRIVER,
            PresentMode::Immediate,
            supported,
        ),
        PresentModeRemedy::KeepRequested,
        "Auto must not reinterpret an already-immediate user setting",
    );
}

#[test]
fn shared_policy_can_be_replaced_by_a_user_vsync_choice() {
    let policy = Dx12PresentModePolicy::new(PresentModePreference::Auto);
    let render_copy = policy.clone();

    render_copy.publish_remedy(PresentModeRemedy::UseImmediate);
    assert_eq!(policy.remedy(), PresentModeRemedy::UseImmediate);

    policy.set_preference(PresentModePreference::Vsync);
    assert_eq!(render_copy.preference(), PresentModePreference::Vsync);
    assert_eq!(render_copy.remedy(), PresentModeRemedy::KeepRequested);

    render_copy.publish_remedy(PresentModeRemedy::UseImmediate);
    policy.set_preference(PresentModePreference::NoVsync);
    assert_eq!(render_copy.preference(), PresentModePreference::NoVsync);
    assert_eq!(render_copy.remedy(), PresentModeRemedy::KeepRequested);
}

#[test]
fn automatic_remedy_emits_deterministic_identity_proof() {
    for field in [
        "preference=Auto",
        "startup_requested=Fifo",
        "requested=Fifo",
        "recommended=Immediate",
        "effective=Immediate",
        r#"adapter=\"{AFFECTED_DX12_ADAPTER}\""#,
        r#"driver=\"{AFFECTED_DX12_DRIVER}\""#,
    ] {
        assert!(
            POLICY_SOURCE.contains(field),
            "present-mode policy log lost {field}"
        );
    }
}

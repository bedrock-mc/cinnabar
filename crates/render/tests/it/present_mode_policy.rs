use bevy::window::PresentMode;
use render::{
    PresentModePolicy, PresentModePreference, PresentModeRemedy, resolve_dx12_present_mode_remedy,
};
use render_model::{PresentModeKind, SurfacePresentModes};
use wgpu::Backend;

const FIFO_AND_IMMEDIATE: SurfacePresentModes =
    SurfacePresentModes::FIFO_ONLY.with(PresentModeKind::Immediate);

const AFFECTED_ADAPTER: &str = "Radeon RX 570 Series";
const AFFECTED_DRIVER: &str = "31.0.21924.61";

#[test]
fn exact_affected_dx12_fifo_path_uses_proven_immediate_mode() {
    assert_eq!(
        resolve_dx12_present_mode_remedy(
            PresentModePreference::Auto,
            Backend::Dx12,
            AFFECTED_ADAPTER,
            AFFECTED_DRIVER,
            PresentMode::Fifo,
            FIFO_AND_IMMEDIATE,
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
            FIFO_AND_IMMEDIATE,
        ),
        (
            Backend::Dx12,
            "Radeon RX 580 Series",
            AFFECTED_DRIVER,
            FIFO_AND_IMMEDIATE,
        ),
        (
            Backend::Dx12,
            AFFECTED_ADAPTER,
            "new-driver",
            FIFO_AND_IMMEDIATE,
        ),
        (
            Backend::Dx12,
            AFFECTED_ADAPTER,
            AFFECTED_DRIVER,
            SurfacePresentModes::FIFO_ONLY,
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
    let supported = FIFO_AND_IMMEDIATE;
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
    let policy = PresentModePolicy::new(PresentModePreference::Auto);
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
fn review_render_republishing_the_same_preference_retains_the_remedy() {
    let policy = PresentModePolicy::default();
    policy.publish_remedy(PresentModeRemedy::UseImmediate);
    policy.set_preference(PresentModePreference::Auto);
    assert_eq!(policy.remedy(), PresentModeRemedy::UseImmediate);
}

#[test]
fn surface_capabilities_are_unknown_until_published_and_cleared_for_a_new_window() {
    let policy = PresentModePolicy::default();
    let render_copy = policy.clone();
    assert_eq!(policy.capabilities(), None);

    render_copy.publish_capabilities(Some(SurfacePresentModes::FIFO_ONLY));
    assert_eq!(policy.capabilities(), Some(SurfacePresentModes::FIFO_ONLY));
    render_copy.publish_capabilities(Some(FIFO_AND_IMMEDIATE));
    assert_eq!(policy.capabilities(), Some(FIFO_AND_IMMEDIATE));

    render_copy.publish_capabilities(None);
    assert_eq!(policy.capabilities(), None);
}

use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU16, Ordering},
};

use bevy::{
    app::{App, Plugin},
    ecs::schedule::SystemSet,
    prelude::Resource,
    render::RenderApp,
    window::PresentMode,
};
#[cfg(target_os = "windows")]
use bevy::{
    ecs::{entity::Entity, system::Local},
    prelude::{IntoScheduleConfigs, Res},
    render::{
        Render, RenderSystems,
        renderer::RenderAdapter,
        view::window::{ExtractedWindows, create_surfaces},
    },
};
use render_model::{PresentModeKind, PresentationIntent, SurfacePresentModes};

const AFFECTED_DX12_ADAPTER: &str = "Radeon RX 570 Series";
const AFFECTED_DX12_DRIVERS: &[&str] = &["31.0.21924.61", "31.0.21925.1001"];
/// Marks published capabilities so an empty FIFO-only set is distinct from "not yet probed".
const CAPABILITIES_KNOWN: u16 = 1 << 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum PresentModePreference {
    #[default]
    Auto = 0,
    Vsync = 1,
    NoVsync = 2,
}

impl PresentModePreference {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Vsync,
            2 => Self::NoVsync,
            _ => Self::Auto,
        }
    }

    #[must_use]
    pub const fn intent(self) -> PresentationIntent {
        match self {
            Self::Auto | Self::Vsync => PresentationIntent::Synchronized,
            Self::NoVsync => PresentationIntent::LowLatency,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PresentModeRemedy {
    KeepRequested = 0,
    UseImmediate = 1,
}

impl PresentModeRemedy {
    fn from_u8(value: u8) -> Self {
        if value == Self::UseImmediate as u8 {
            Self::UseImmediate
        } else {
            Self::KeepRequested
        }
    }
}

/// Presentation state shared by the main world, which picks the window's mode, and the render
/// world, which probes the surface and the driver remedy.
#[derive(Resource, Clone, Debug)]
pub struct PresentModePolicy {
    preference: Arc<AtomicU8>,
    remedy: Arc<AtomicU8>,
    capabilities: Arc<AtomicU16>,
}

impl Default for PresentModePolicy {
    fn default() -> Self {
        Self::new(PresentModePreference::Auto)
    }
}

impl PresentModePolicy {
    #[must_use]
    pub fn new(preference: PresentModePreference) -> Self {
        Self {
            preference: Arc::new(AtomicU8::new(preference as u8)),
            remedy: Arc::new(AtomicU8::new(PresentModeRemedy::KeepRequested as u8)),
            capabilities: Arc::default(),
        }
    }

    pub fn set_preference(&self, preference: PresentModePreference) {
        if self.preference.swap(preference as u8, Ordering::AcqRel) != preference as u8 {
            self.publish_remedy(PresentModeRemedy::KeepRequested);
        }
    }

    #[must_use]
    pub fn preference(&self) -> PresentModePreference {
        PresentModePreference::from_u8(self.preference.load(Ordering::Acquire))
    }

    pub fn publish_remedy(&self, remedy: PresentModeRemedy) {
        self.remedy.store(remedy as u8, Ordering::Release);
    }

    #[must_use]
    pub fn remedy(&self) -> PresentModeRemedy {
        PresentModeRemedy::from_u8(self.remedy.load(Ordering::Acquire))
    }

    /// Publishes the primary surface's modes; `None` while a new window awaits its probe.
    pub fn publish_capabilities(&self, modes: Option<SurfacePresentModes>) {
        let encoded = modes.map_or(0, |modes| CAPABILITIES_KNOWN | u16::from(modes.bits()));
        self.capabilities.store(encoded, Ordering::Release);
    }

    #[must_use]
    pub fn capabilities(&self) -> Option<SurfacePresentModes> {
        let encoded = self.capabilities.load(Ordering::Acquire);
        (encoded & CAPABILITIES_KNOWN != 0)
            .then(|| SurfacePresentModes::from_bits(encoded.to_le_bytes()[0]))
    }
}

/// Converts a selected mode into the window request Bevy configures.
#[must_use]
pub const fn window_present_mode(mode: PresentModeKind) -> PresentMode {
    match mode {
        PresentModeKind::Fifo => PresentMode::Fifo,
        PresentModeKind::FifoRelaxed => PresentMode::FifoRelaxed,
        PresentModeKind::Mailbox => PresentMode::Mailbox,
        PresentModeKind::Immediate => PresentMode::Immediate,
    }
}

/// The concrete mode a window requests; automatic requests have none.
#[must_use]
pub const fn requested_present_mode_kind(mode: PresentMode) -> Option<PresentModeKind> {
    match mode {
        PresentMode::Fifo => Some(PresentModeKind::Fifo),
        PresentMode::FifoRelaxed => Some(PresentModeKind::FifoRelaxed),
        PresentMode::Mailbox => Some(PresentModeKind::Mailbox),
        PresentMode::Immediate => Some(PresentModeKind::Immediate),
        PresentMode::AutoVsync | PresentMode::AutoNoVsync => None,
    }
}

#[must_use]
pub fn resolve_dx12_present_mode_remedy(
    preference: PresentModePreference,
    backend: wgpu::Backend,
    adapter: &str,
    driver: &str,
    requested: PresentMode,
    supported: SurfacePresentModes,
) -> PresentModeRemedy {
    if preference == PresentModePreference::Auto
        && backend == wgpu::Backend::Dx12
        && adapter.trim().eq_ignore_ascii_case(AFFECTED_DX12_ADAPTER)
        && AFFECTED_DX12_DRIVERS.contains(&driver.trim())
        && requested == PresentMode::Fifo
        && supported.contains(PresentModeKind::Immediate)
    {
        PresentModeRemedy::UseImmediate
    } else {
        PresentModeRemedy::KeepRequested
    }
}

/// Shares `policy` with the render world, which probes the surface and the driver remedy.
#[derive(Clone, Debug)]
pub struct PresentModePolicyPlugin {
    policy: PresentModePolicy,
}

impl PresentModePolicyPlugin {
    #[must_use]
    pub fn new(policy: PresentModePolicy) -> Self {
        Self { policy }
    }
}

impl Plugin for PresentModePolicyPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.insert_resource(self.policy.clone());
        crate::surface_lifecycle::install(render_app);
        crate::surface_capabilities::install(render_app);
        #[cfg(target_os = "windows")]
        render_app.add_systems(
            Render,
            apply_dx12_present_mode_policy
                .in_set(PresentModePolicySet)
                .after(crate::surface_capabilities::SurfaceCapabilitiesSet)
                .after(RenderSystems::ExtractCommands)
                .before(create_surfaces),
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub(crate) struct PresentModePolicySet;

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CachedResolution {
    window: Entity,
    preference: PresentModePreference,
    requested: PresentMode,
    remedy: PresentModeRemedy,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AutoRemedyLifecycleState {
    #[default]
    Idle,
    Pending {
        window: u64,
    },
    Proven {
        window: u64,
    },
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AutoRemedyLifecycleEvent {
    None,
    RecommendationPending,
    EffectiveProven,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Default)]
struct AutoRemedyLifecycle {
    state: AutoRemedyLifecycleState,
}

#[cfg(any(target_os = "windows", test))]
impl AutoRemedyLifecycle {
    fn observe_extraction(
        &mut self,
        window: u64,
        preference: PresentModePreference,
        requested: PresentMode,
    ) -> AutoRemedyLifecycleEvent {
        if preference != PresentModePreference::Auto {
            self.reset();
            return AutoRemedyLifecycleEvent::None;
        }
        match self.state {
            AutoRemedyLifecycleState::Pending {
                window: pending_window,
            } if pending_window == window && requested == PresentMode::Immediate => {
                self.state = AutoRemedyLifecycleState::Proven { window };
                AutoRemedyLifecycleEvent::EffectiveProven
            }
            AutoRemedyLifecycleState::Pending {
                window: pending_window,
            }
            | AutoRemedyLifecycleState::Proven {
                window: pending_window,
            } if pending_window != window => {
                self.reset();
                AutoRemedyLifecycleEvent::None
            }
            AutoRemedyLifecycleState::Pending { .. }
                if requested != PresentMode::Fifo && requested != PresentMode::Immediate =>
            {
                self.reset();
                AutoRemedyLifecycleEvent::None
            }
            _ => AutoRemedyLifecycleEvent::None,
        }
    }

    fn observe_resolution(
        &mut self,
        window: u64,
        preference: PresentModePreference,
        requested: PresentMode,
        remedy: PresentModeRemedy,
    ) -> AutoRemedyLifecycleEvent {
        if preference != PresentModePreference::Auto
            || requested != PresentMode::Fifo
            || remedy != PresentModeRemedy::UseImmediate
        {
            if !matches!(self.state, AutoRemedyLifecycleState::Proven { window: proven } if proven == window)
            {
                self.reset();
            }
            return AutoRemedyLifecycleEvent::None;
        }
        match self.state {
            AutoRemedyLifecycleState::Pending {
                window: pending_window,
            }
            | AutoRemedyLifecycleState::Proven {
                window: pending_window,
            } if pending_window == window => AutoRemedyLifecycleEvent::None,
            _ => {
                self.state = AutoRemedyLifecycleState::Pending { window };
                AutoRemedyLifecycleEvent::RecommendationPending
            }
        }
    }

    fn reset(&mut self) {
        self.state = AutoRemedyLifecycleState::Idle;
    }
}

#[cfg(target_os = "windows")]
fn apply_dx12_present_mode_policy(
    windows: Res<ExtractedWindows>,
    probed: Res<crate::surface_capabilities::ProbedSurface>,
    render_adapter: Res<RenderAdapter>,
    policy: Res<PresentModePolicy>,
    mut cached: Local<Option<CachedResolution>>,
    mut lifecycle: Local<AutoRemedyLifecycle>,
) {
    let preference = policy.preference();
    let Some(window_id) = windows.primary else {
        policy.publish_remedy(PresentModeRemedy::KeepRequested);
        *cached = None;
        lifecycle.reset();
        return;
    };
    let Some(window) = windows.windows.get(&window_id) else {
        policy.publish_remedy(PresentModeRemedy::KeepRequested);
        *cached = None;
        lifecycle.reset();
        return;
    };
    let requested = window.present_mode;
    let window_identity = window_id.to_bits();
    if lifecycle.observe_extraction(window_identity, preference, requested)
        == AutoRemedyLifecycleEvent::EffectiveProven
    {
        bevy::log::warn!(
            "present_mode_policy preference=Auto startup_requested=Fifo requested=Immediate recommended=Immediate effective=Immediate state=proven adapter=\"{AFFECTED_DX12_ADAPTER}\""
        );
    }
    let key_matches = cached.as_ref().is_some_and(|resolution| {
        resolution.window == window_id
            && resolution.preference == preference
            && resolution.requested == requested
    });
    if !key_matches {
        policy.publish_remedy(PresentModeRemedy::KeepRequested);
        *cached = None;
        let Some(supported) = probed.modes_for(window_id) else {
            return;
        };
        let adapter_info = render_adapter.get_info();
        let resolution = CachedResolution {
            window: window_id,
            preference,
            requested,
            remedy: resolve_dx12_present_mode_remedy(
                preference,
                adapter_info.backend,
                &adapter_info.name,
                &adapter_info.driver,
                requested,
                supported,
            ),
        };
        policy.publish_remedy(resolution.remedy);
        if lifecycle.observe_resolution(window_identity, preference, requested, resolution.remedy)
            == AutoRemedyLifecycleEvent::RecommendationPending
        {
            bevy::log::warn!(
                "present_mode_policy preference=Auto startup_requested=Fifo requested=Fifo recommended=Immediate state=pending adapter=\"{AFFECTED_DX12_ADAPTER}\"; use --vsync to force FIFO"
            );
        }
        *cached = Some(resolution);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_remedy_proof_requires_later_immediate_extraction_and_is_one_shot() {
        let mut lifecycle = AutoRemedyLifecycle::default();
        assert_eq!(
            lifecycle.observe_resolution(
                7,
                PresentModePreference::Auto,
                PresentMode::Fifo,
                PresentModeRemedy::UseImmediate,
            ),
            AutoRemedyLifecycleEvent::RecommendationPending
        );
        assert_eq!(
            lifecycle.observe_extraction(7, PresentModePreference::Auto, PresentMode::Fifo),
            AutoRemedyLifecycleEvent::None,
            "the FIFO extraction that requested the remedy is not effective proof"
        );
        assert_eq!(
            lifecycle.observe_extraction(8, PresentModePreference::Auto, PresentMode::Immediate),
            AutoRemedyLifecycleEvent::None,
            "another window cannot prove the pending recommendation"
        );

        assert_eq!(
            lifecycle.observe_resolution(
                7,
                PresentModePreference::Auto,
                PresentMode::Fifo,
                PresentModeRemedy::UseImmediate,
            ),
            AutoRemedyLifecycleEvent::RecommendationPending
        );
        assert_eq!(
            lifecycle.observe_extraction(7, PresentModePreference::Auto, PresentMode::Immediate),
            AutoRemedyLifecycleEvent::EffectiveProven
        );
        assert_eq!(
            lifecycle.observe_extraction(7, PresentModePreference::Auto, PresentMode::Immediate),
            AutoRemedyLifecycleEvent::None,
            "the same effective extraction must not emit duplicate proof"
        );
    }

    #[test]
    fn automatic_remedy_never_proves_when_adoption_does_not_occur() {
        let mut lifecycle = AutoRemedyLifecycle::default();
        assert_eq!(
            lifecycle.observe_resolution(
                11,
                PresentModePreference::Auto,
                PresentMode::Fifo,
                PresentModeRemedy::UseImmediate,
            ),
            AutoRemedyLifecycleEvent::RecommendationPending
        );
        for _ in 0..120 {
            assert_eq!(
                lifecycle.observe_extraction(11, PresentModePreference::Auto, PresentMode::Fifo),
                AutoRemedyLifecycleEvent::None
            );
        }
    }
}

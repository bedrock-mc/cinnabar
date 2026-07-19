use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use bevy::{
    app::{App, Plugin},
    ecs::{entity::Entity, schedule::SystemSet, system::Local},
    prelude::{IntoScheduleConfigs, Res, Resource},
    render::{
        Render, RenderApp, RenderSystems,
        renderer::{RenderAdapter, RenderInstance},
        view::window::{ExtractedWindows, create_surfaces},
    },
    window::PresentMode,
};

const AFFECTED_DX12_ADAPTER: &str = "Radeon RX 570 Series";
const AFFECTED_DX12_DRIVER: &str = "31.0.21924.61";
const INITIAL_PROBE_RETRY_FRAMES: u16 = 4;
const MAX_PROBE_RETRY_FRAMES: u16 = 60;
const MAX_BACKOFF_FAILURES: u8 = 5;

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

#[derive(Resource, Clone, Debug)]
pub struct Dx12PresentModePolicy {
    preference: Arc<AtomicU8>,
    remedy: Arc<AtomicU8>,
}

impl Default for Dx12PresentModePolicy {
    fn default() -> Self {
        Self::new(PresentModePreference::Auto)
    }
}

impl Dx12PresentModePolicy {
    #[must_use]
    pub fn new(preference: PresentModePreference) -> Self {
        Self {
            preference: Arc::new(AtomicU8::new(preference as u8)),
            remedy: Arc::new(AtomicU8::new(PresentModeRemedy::KeepRequested as u8)),
        }
    }

    pub fn set_preference(&self, preference: PresentModePreference) {
        self.preference.store(preference as u8, Ordering::Release);
        self.publish_remedy(PresentModeRemedy::KeepRequested);
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
}

#[must_use]
pub fn resolve_dx12_present_mode_remedy(
    preference: PresentModePreference,
    backend: wgpu::Backend,
    adapter: &str,
    driver: &str,
    requested: PresentMode,
    supported: &[wgpu::PresentMode],
) -> PresentModeRemedy {
    if preference == PresentModePreference::Auto
        && backend == wgpu::Backend::Dx12
        && adapter.trim().eq_ignore_ascii_case(AFFECTED_DX12_ADAPTER)
        && driver.trim() == AFFECTED_DX12_DRIVER
        && requested == PresentMode::Fifo
        && supported.contains(&wgpu::PresentMode::Immediate)
    {
        PresentModeRemedy::UseImmediate
    } else {
        PresentModeRemedy::KeepRequested
    }
}

#[derive(Clone, Debug)]
pub struct Dx12PresentModePolicyPlugin {
    policy: Dx12PresentModePolicy,
}

impl Dx12PresentModePolicyPlugin {
    #[must_use]
    pub fn new(policy: Dx12PresentModePolicy) -> Self {
        Self { policy }
    }
}

impl Plugin for Dx12PresentModePolicyPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.insert_resource(self.policy.clone()).add_systems(
            Render,
            apply_dx12_present_mode_policy
                .in_set(PresentModePolicySet)
                .after(RenderSystems::ExtractCommands)
                .before(create_surfaces),
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, SystemSet)]
pub(crate) struct PresentModePolicySet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CachedResolution {
    window: Entity,
    preference: PresentModePreference,
    requested: PresentMode,
    remedy: PresentModeRemedy,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AutoRemedyLifecycleEvent {
    None,
    RecommendationPending,
    EffectiveProven,
}

#[derive(Debug, Default)]
struct AutoRemedyLifecycle {
    state: AutoRemedyLifecycleState,
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SurfaceProbeKey {
    window: u64,
    preference: PresentModePreference,
    requested: PresentMode,
}

#[derive(Debug, Default)]
struct SurfaceProbeRetry {
    key: Option<SurfaceProbeKey>,
    consecutive_failures: u8,
    cooldown_frames: u16,
}

impl SurfaceProbeRetry {
    fn should_attempt(&mut self, key: SurfaceProbeKey) -> bool {
        if self.key != Some(key) {
            self.key = Some(key);
            self.consecutive_failures = 0;
            self.cooldown_frames = 0;
        }
        if self.cooldown_frames == 0 {
            true
        } else {
            self.cooldown_frames -= 1;
            false
        }
    }

    fn record_failure(&mut self, key: SurfaceProbeKey) {
        if self.key != Some(key) {
            self.key = Some(key);
            self.consecutive_failures = 0;
        }
        self.consecutive_failures = self
            .consecutive_failures
            .saturating_add(1)
            .min(MAX_BACKOFF_FAILURES);
        let shift = u32::from(self.consecutive_failures.saturating_sub(1));
        self.cooldown_frames = INITIAL_PROBE_RETRY_FRAMES
            .checked_shl(shift)
            .unwrap_or(MAX_PROBE_RETRY_FRAMES)
            .min(MAX_PROBE_RETRY_FRAMES);
    }

    fn record_success(&mut self, key: SurfaceProbeKey) {
        self.key = Some(key);
        self.consecutive_failures = 0;
        self.cooldown_frames = 0;
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

fn apply_dx12_present_mode_policy(
    #[cfg(any(target_os = "macos", target_os = "ios"))] _marker: bevy::ecs::system::NonSendMarker,
    windows: Res<ExtractedWindows>,
    render_instance: Res<RenderInstance>,
    render_adapter: Res<RenderAdapter>,
    policy: Res<Dx12PresentModePolicy>,
    mut cached: Local<Option<CachedResolution>>,
    mut lifecycle: Local<AutoRemedyLifecycle>,
    mut probe_retry: Local<SurfaceProbeRetry>,
) {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (
            &windows,
            &render_instance,
            &render_adapter,
            &policy,
            &mut cached,
            &mut lifecycle,
            &mut probe_retry,
        );
    }

    #[cfg(target_os = "windows")]
    {
        let preference = policy.preference();
        let Some(window_id) = windows.primary else {
            policy.publish_remedy(PresentModeRemedy::KeepRequested);
            *cached = None;
            lifecycle.reset();
            probe_retry.reset();
            return;
        };
        let Some(window) = windows.windows.get(&window_id) else {
            policy.publish_remedy(PresentModeRemedy::KeepRequested);
            *cached = None;
            lifecycle.reset();
            probe_retry.reset();
            return;
        };
        let requested = window.present_mode;
        let window_identity = window_id.to_bits();
        if lifecycle.observe_extraction(window_identity, preference, requested)
            == AutoRemedyLifecycleEvent::EffectiveProven
        {
            bevy::log::warn!(
                "present_mode_policy preference=Auto startup_requested=Fifo requested=Immediate recommended=Immediate effective=Immediate state=proven adapter=\"{AFFECTED_DX12_ADAPTER}\" driver=\"{AFFECTED_DX12_DRIVER}\""
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
            let probe_key = SurfaceProbeKey {
                window: window_identity,
                preference,
                requested,
            };
            if !probe_retry.should_attempt(probe_key) {
                return;
            }
            let surface_target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: window.handle.get_display_handle(),
                raw_window_handle: window.handle.get_window_handle(),
            };
            // SAFETY: The render-world extracted window owns valid handles and this
            // system is constrained to Bevy's main-thread render surface schedule.
            let Ok(surface) = (unsafe { render_instance.create_surface_unsafe(surface_target) })
            else {
                probe_retry.record_failure(probe_key);
                return;
            };
            probe_retry.record_success(probe_key);
            let capabilities = surface.get_capabilities(&render_adapter);
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
                    &capabilities.present_modes,
                ),
            };
            policy.publish_remedy(resolution.remedy);
            if lifecycle.observe_resolution(
                window_identity,
                preference,
                requested,
                resolution.remedy,
            ) == AutoRemedyLifecycleEvent::RecommendationPending
            {
                bevy::log::warn!(
                    "present_mode_policy preference=Auto startup_requested=Fifo requested=Fifo recommended=Immediate state=pending adapter=\"{AFFECTED_DX12_ADAPTER}\" driver=\"{AFFECTED_DX12_DRIVER}\"; use --vsync to force FIFO"
                );
            }
            *cached = Some(resolution);
        }
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

    #[test]
    fn failed_surface_probe_uses_capped_backoff_and_eventually_retries() {
        let mut retry = SurfaceProbeRetry::default();
        let key = SurfaceProbeKey {
            window: 13,
            preference: PresentModePreference::Auto,
            requested: PresentMode::Fifo,
        };
        assert!(retry.should_attempt(key));
        retry.record_failure(key);
        for _ in 0..INITIAL_PROBE_RETRY_FRAMES {
            assert!(!retry.should_attempt(key));
        }
        assert!(retry.should_attempt(key));

        for _ in 0..(MAX_BACKOFF_FAILURES + 2) {
            retry.record_failure(key);
        }
        assert_eq!(retry.cooldown_frames, MAX_PROBE_RETRY_FRAMES);
        for _ in 0..MAX_PROBE_RETRY_FRAMES {
            assert!(!retry.should_attempt(key));
        }
        assert!(retry.should_attempt(key));

        let replacement = SurfaceProbeKey { window: 14, ..key };
        retry.record_failure(key);
        assert!(
            retry.should_attempt(replacement),
            "a changed window/key must not inherit an unrelated failure cooldown"
        );
    }
}

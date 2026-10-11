//! One probe of the primary surface's present modes, shared by mode selection and telemetry.

use bevy::{
    app::SubApp,
    ecs::{entity::Entity, schedule::IntoScheduleConfigs},
    prelude::{Res, ResMut, Resource},
    render::{
        Render, RenderSystems,
        renderer::{RenderAdapter, RenderInstance},
        view::window::{ExtractedWindows, create_surfaces},
    },
};
use render_model::{PresentModeKind, SurfacePresentModes};

use crate::present_mode::PresentModePolicy;

const INITIAL_PROBE_RETRY_FRAMES: u16 = 4;
const MAX_PROBE_RETRY_FRAMES: u16 = 60;
const MAX_BACKOFF_FAILURES: u8 = 5;

/// The primary window's advertised present modes, once known.
#[derive(Resource, Debug, Default)]
pub(crate) struct ProbedSurface {
    window: Option<Entity>,
    modes: Option<SurfacePresentModes>,
    retry: SurfaceProbeRetry,
}

impl ProbedSurface {
    /// The modes advertised for `window`; `None` until it has been probed.
    pub(crate) fn modes_for(&self, window: Entity) -> Option<SurfacePresentModes> {
        (self.window == Some(window))
            .then_some(self.modes)
            .flatten()
    }
}

/// Installs the probe once, however many plugins depend on it.
pub(crate) fn install(render_app: &mut SubApp) {
    if render_app.world().contains_resource::<ProbedSurface>() {
        return;
    }
    render_app.init_resource::<ProbedSurface>().add_systems(
        Render,
        probe_surface_present_modes
            .run_if(probe_pending)
            .in_set(SurfaceCapabilitiesSet)
            .after(RenderSystems::ExtractCommands)
            .before(create_surfaces),
    );
}

/// Orders consumers after the probe within the same frame.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, bevy::ecs::schedule::SystemSet)]
pub(crate) struct SurfaceCapabilitiesSet;

/// Keeps the main-thread probe off the schedule once the current window is known.
fn probe_pending(windows: Res<ExtractedWindows>, probed: Res<ProbedSurface>) -> bool {
    windows
        .primary
        .is_some_and(|window| probed.modes_for(window).is_none())
}

fn probe_surface_present_modes(
    #[cfg(any(target_os = "macos", target_os = "ios"))] _marker: bevy::ecs::system::NonSendMarker,
    windows: Res<ExtractedWindows>,
    render_instance: Res<RenderInstance>,
    render_adapter: Res<RenderAdapter>,
    policy: Option<Res<PresentModePolicy>>,
    mut probed: ResMut<ProbedSurface>,
) {
    let Some(window_id) = windows.primary else {
        return;
    };
    if probed.window != Some(window_id) {
        probed.window = Some(window_id);
        probed.modes = None;
        probed.retry = SurfaceProbeRetry::default();
        if let Some(policy) = &policy {
            policy.publish_capabilities(None);
        }
    }
    let Some(window) = windows.windows.get(&window_id) else {
        return;
    };
    let modes = if render_adapter.get_info().backend == wgpu::Backend::Metal {
        // wgpu's Metal surfaces always offer FIFO and Immediate; a probe surface would leave an
        // extra CAMetalLayer attached to the window.
        SurfacePresentModes::FIFO_ONLY.with(PresentModeKind::Immediate)
    } else {
        if !probed.retry.should_attempt() {
            return;
        }
        let surface_target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(window.handle.get_display_handle()),
            raw_window_handle: window.handle.get_window_handle(),
        };
        #[cfg(feature = "tracy")]
        let _zone = bevy::log::info_span!("render.surface_probe").entered();
        // SAFETY: the extracted window owns valid handles, and this runs on the main thread
        // where a platform requires it, just before Bevy configures the same window.
        let Ok(surface) = (unsafe { render_instance.create_surface_unsafe(surface_target) }) else {
            probed.retry.record_failure();
            return;
        };
        surface
            .get_capabilities(&render_adapter)
            .present_modes
            .into_iter()
            .filter_map(present_mode_kind)
            .collect()
    };
    probed.modes = Some(modes);
    if let Some(policy) = &policy {
        policy.publish_capabilities(Some(modes));
    }
}

/// Maps a backend mode; automatic modes are requests, never advertised capabilities.
pub(crate) const fn present_mode_kind(mode: wgpu::PresentMode) -> Option<PresentModeKind> {
    match mode {
        wgpu::PresentMode::Fifo => Some(PresentModeKind::Fifo),
        wgpu::PresentMode::FifoRelaxed => Some(PresentModeKind::FifoRelaxed),
        wgpu::PresentMode::Mailbox => Some(PresentModeKind::Mailbox),
        wgpu::PresentMode::Immediate => Some(PresentModeKind::Immediate),
        wgpu::PresentMode::AutoVsync | wgpu::PresentMode::AutoNoVsync => None,
    }
}

/// Capped exponential backoff after a failed probe-surface creation.
#[derive(Debug, Default)]
struct SurfaceProbeRetry {
    consecutive_failures: u8,
    cooldown_frames: u16,
}

impl SurfaceProbeRetry {
    fn should_attempt(&mut self) -> bool {
        if self.cooldown_frames == 0 {
            true
        } else {
            self.cooldown_frames -= 1;
            false
        }
    }

    fn record_failure(&mut self) {
        self.consecutive_failures = self
            .consecutive_failures
            .saturating_add(1)
            .min(MAX_BACKOFF_FAILURES);
        let shift = u32::from(self.consecutive_failures - 1);
        self.cooldown_frames = INITIAL_PROBE_RETRY_FRAMES
            .checked_shl(shift)
            .unwrap_or(MAX_PROBE_RETRY_FRAMES)
            .min(MAX_PROBE_RETRY_FRAMES);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_surface_probe_uses_capped_backoff_and_eventually_retries() {
        let mut retry = SurfaceProbeRetry::default();
        assert!(retry.should_attempt());
        retry.record_failure();
        for _ in 0..INITIAL_PROBE_RETRY_FRAMES {
            assert!(!retry.should_attempt());
        }
        assert!(retry.should_attempt());

        for _ in 0..(MAX_BACKOFF_FAILURES + 2) {
            retry.record_failure();
        }
        assert_eq!(retry.cooldown_frames, MAX_PROBE_RETRY_FRAMES);
        for _ in 0..MAX_PROBE_RETRY_FRAMES {
            assert!(!retry.should_attempt());
        }
        assert!(retry.should_attempt());
    }

    #[test]
    fn capabilities_belong_to_the_probed_window_only() {
        let first = Entity::from_raw_u32(1).unwrap();
        let second = Entity::from_raw_u32(2).unwrap();
        let probed = ProbedSurface {
            window: Some(first),
            modes: Some(SurfacePresentModes::FIFO_ONLY),
            retry: SurfaceProbeRetry::default(),
        };
        assert_eq!(
            probed.modes_for(first),
            Some(SurfacePresentModes::FIFO_ONLY)
        );
        assert_eq!(probed.modes_for(second), None);
    }
}

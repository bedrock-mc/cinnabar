use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use bevy::{
    app::{App, Plugin},
    ecs::{entity::Entity, system::Local},
    prelude::{IntoScheduleConfigs, Res, ResMut, Resource},
    render::{
        Render, RenderApp, RenderSystems,
        renderer::{RenderAdapter, RenderInstance},
        view::window::{ExtractedWindows, create_surfaces},
    },
    window::PresentMode,
};

const AFFECTED_DX12_ADAPTER: &str = "Radeon RX 570 Series";
const AFFECTED_DX12_DRIVER: &str = "31.0.21924.61";

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
pub enum PresentModeRemedy {
    KeepRequested,
    UseImmediate,
}

#[derive(Resource, Clone, Debug)]
pub struct Dx12PresentModePolicy {
    preference: Arc<AtomicU8>,
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
        }
    }

    pub fn set_preference(&self, preference: PresentModePreference) {
        self.preference.store(preference as u8, Ordering::Release);
    }

    #[must_use]
    pub fn preference(&self) -> PresentModePreference {
        PresentModePreference::from_u8(self.preference.load(Ordering::Acquire))
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
        render_app
            .insert_resource(self.policy.clone())
            .add_systems(
                Render,
                apply_dx12_present_mode_policy
                    .after(RenderSystems::ExtractCommands)
                    .before(create_surfaces),
            );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CachedResolution {
    window: Entity,
    preference: PresentModePreference,
    requested: PresentMode,
    remedy: PresentModeRemedy,
}

fn apply_dx12_present_mode_policy(
    #[cfg(any(target_os = "macos", target_os = "ios"))] _marker: bevy::ecs::system::NonSendMarker,
    mut windows: ResMut<ExtractedWindows>,
    render_instance: Res<RenderInstance>,
    render_adapter: Res<RenderAdapter>,
    policy: Res<Dx12PresentModePolicy>,
    mut cached: Local<Option<CachedResolution>>,
    mut logged: Local<bool>,
) {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (
            &mut windows,
            &render_instance,
            &render_adapter,
            &policy,
            &mut cached,
            &mut logged,
        );
        return;
    }

    #[cfg(target_os = "windows")]
    {
        let preference = policy.preference();
        let Some(window_id) = windows.primary else {
            return;
        };
        let Some(window) = windows.windows.get_mut(&window_id) else {
            return;
        };
        let requested = window.present_mode;
        let key_matches = cached.as_ref().is_some_and(|resolution| {
            resolution.window == window_id
                && resolution.preference == preference
                && resolution.requested == requested
        });
        if !key_matches {
            let surface_target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: window.handle.get_display_handle(),
                raw_window_handle: window.handle.get_window_handle(),
            };
            // SAFETY: The render-world extracted window owns valid handles and this
            // system is constrained to Bevy's main-thread render surface schedule.
            let Ok(surface) = (unsafe { render_instance.create_surface_unsafe(surface_target) })
            else {
                return;
            };
            let capabilities = surface.get_capabilities(&render_adapter);
            let adapter_info = render_adapter.get_info();
            *cached = Some(CachedResolution {
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
            });
        }
        if cached.as_ref().is_some_and(|resolution| {
            resolution.remedy == PresentModeRemedy::UseImmediate
        }) {
            window.present_mode_changed |= window.present_mode != PresentMode::Immediate;
            window.present_mode = PresentMode::Immediate;
            if !*logged {
                bevy::log::warn!(
                    "using Immediate presentation for the measured RX 570 DX12 FIFO driver defect; use --vsync to force FIFO"
                );
                *logged = true;
            }
        }
    }
}

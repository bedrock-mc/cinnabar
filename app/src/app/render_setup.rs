//! Renderer setup shared by direct and launcher sessions.

use std::{ffi::OsStr, num::NonZeroU32};

use bevy::{
    render::{
        RenderPlugin,
        settings::{Backends, RenderCreation, WgpuSettings},
    },
    window::{PresentMode, Window},
};

/// The primary window; `frame_latency` must suit the session's VSync choice from the start.
pub(super) fn primary_window(
    title: String,
    present_mode: PresentMode,
    frame_latency: NonZeroU32,
) -> Window {
    Window {
        title,
        present_mode,
        desired_maximum_frame_latency: Some(frame_latency),
        ..Default::default()
    }
}

pub(super) fn render_plugin() -> RenderPlugin {
    let mut settings = WgpuSettings::default();
    settings.limits.max_storage_buffers_per_shader_stage = settings
        .limits
        .max_storage_buffers_per_shader_stage
        .max(render::required_vertex_storage_buffers());
    if let Some(backends) = preferred_render_backends(
        std::env::var_os("WGPU_BACKEND").as_deref(),
        dx12_hardware_adapter,
    ) {
        settings.backends = Some(backends);
    }
    RenderPlugin {
        render_creation: RenderCreation::Automatic(settings),
        ..Default::default()
    }
}

/// The backends the renderer may choose from, or `None` to keep wgpu's defaults. An explicit
/// `WGPU_BACKEND` keeps full operator control. Windows uses DX12 whenever `dx12_hardware`
/// reports a hardware adapter: its fixed-count GPU culling matches Vulkan's, while wgpu's
/// Windows Vulkan swapchain waits on a fence after every acquire without VSync. Vulkan remains
/// the fallback for systems without one.
fn preferred_render_backends(
    explicit: Option<&OsStr>,
    dx12_hardware: impl FnOnce() -> bool,
) -> Option<Backends> {
    if explicit.is_some() {
        return None;
    }
    if !cfg!(target_os = "windows") {
        return None;
    }
    Some(if dx12_hardware() {
        Backends::DX12
    } else {
        Backends::VULKAN | Backends::DX12
    })
}

/// Whether DX12 exposes a discrete or integrated GPU on this system.
fn dx12_hardware_adapter() -> bool {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
    });
    instance
        .enumerate_adapters(wgpu::Backends::DX12)
        .iter()
        .any(|adapter| {
            matches!(
                adapter.get_info().device_type,
                wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_prefers_dx12_hardware_and_falls_back_to_vulkan_without_overriding_an_explicit_backend()
     {
        use bevy::render::settings::Backends;
        use std::ffi::OsStr;

        let probed = std::cell::Cell::new(false);
        let probe = |available: bool| {
            let probed = &probed;
            move || {
                probed.set(true);
                available
            }
        };
        assert_eq!(
            preferred_render_backends(Some(OsStr::new("vulkan")), probe(true)),
            None
        );
        assert!(!probed.get(), "an explicit backend skips the adapter probe");
        if cfg!(target_os = "windows") {
            assert_eq!(
                preferred_render_backends(None, probe(true)),
                Some(Backends::DX12)
            );
            assert_eq!(
                preferred_render_backends(None, probe(false)),
                Some(Backends::VULKAN | Backends::DX12)
            );
        } else {
            assert_eq!(preferred_render_backends(None, probe(true)), None);
            assert!(!probed.get(), "other platforms keep wgpu's defaults");
        }
    }

    #[test]
    fn primary_window_queues_one_frame_with_vsync_and_two_without() {
        // Windows request FIFO until the surface is probed, whatever VSync choice they serve.
        for (vsync, queued) in [(true, 1), (false, 2)] {
            let window = primary_window(
                String::new(),
                PresentMode::Fifo,
                render::frame_latency_for_vsync(vsync),
            );
            assert_eq!(
                window.desired_maximum_frame_latency.map(NonZeroU32::get),
                Some(queued)
            );
            assert_eq!(window.present_mode, PresentMode::Fifo);
        }
    }
}

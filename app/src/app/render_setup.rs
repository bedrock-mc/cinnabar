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
    configure_render_settings(&mut settings);
    if let Some(backends) = preferred_render_backends(std::env::var_os("WGPU_BACKEND").as_deref()) {
        settings.backends = Some(backends);
    }
    #[cfg(windows)]
    {
        static LOG_COMPILER: std::sync::Once = std::sync::Once::new();
        // Renderer settings are selected before the log plugin is installed.
        LOG_COMPILER.call_once(|| {
            eprintln!(
                "Configured DX12 shader compiler: {:?}",
                settings.dx12_shader_compiler
            );
        });
    }
    RenderPlugin {
        render_creation: RenderCreation::Automatic(Box::new(settings)),
        ..Default::default()
    }
}

/// Applies renderer limits and compiler policy to Bevy's defaults.
fn configure_render_settings(settings: &mut WgpuSettings) {
    // Shipping installs use FXC; a DLL in the launch directory must not change that.
    settings.dx12_shader_compiler = wgpu::Dx12Compiler::Fxc;
    settings.limits.max_storage_buffers_per_shader_stage = settings
        .limits
        .max_storage_buffers_per_shader_stage
        .max(render::required_vertex_storage_buffers());
}

/// Preserves explicit backend choices; otherwise Windows admits Vulkan before DX12.
/// Vulkan uses count-driven GPU culling, with DX12 available as a fallback.
fn preferred_render_backends(explicit: Option<&OsStr>) -> Option<Backends> {
    if explicit.is_some() || !cfg!(target_os = "windows") {
        return None;
    }
    Some(Backends::VULKAN | Backends::DX12)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_compiler_does_not_inherit_a_launch_directory_dxc() {
        let mut settings = WgpuSettings {
            dx12_shader_compiler: wgpu::Dx12Compiler::DynamicDxc {
                dxc_path: "dxcompiler.dll".into(),
            },
            ..Default::default()
        };
        configure_render_settings(&mut settings);
        assert!(matches!(
            settings.dx12_shader_compiler,
            wgpu::Dx12Compiler::Fxc
        ));
    }

    #[test]
    fn windows_admits_vulkan_before_dx12_without_overriding_an_explicit_backend() {
        use bevy::render::settings::Backends;
        use std::ffi::OsStr;

        assert_eq!(preferred_render_backends(Some(OsStr::new("dx12"))), None);
        let expected = cfg!(target_os = "windows").then_some(Backends::VULKAN | Backends::DX12);
        assert_eq!(preferred_render_backends(None), expected);
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

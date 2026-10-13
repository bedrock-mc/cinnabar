//! Renderer setup shared by direct and launcher sessions.

#[cfg(not(windows))]
use std::ffi::OsStr;
use std::num::NonZeroU32;

#[cfg(any(windows, test))]
mod guarded;

#[cfg(not(windows))]
use bevy::render::settings::{Backends, RenderCreation};
use bevy::{
    render::{RenderPlugin, settings::WgpuSettings},
    window::{PresentMode, Window, WindowResizeConstraints},
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
        resize_constraints: WindowResizeConstraints {
            min_width: ui::MIN_WINDOW_LOGICAL_SIZE[0],
            min_height: ui::MIN_WINDOW_LOGICAL_SIZE[1],
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Configures the renderer and installs Windows startup recovery before GPU initialization.
pub(super) fn render_plugin(
    app: &mut bevy::prelude::App,
    data_root: &std::path::Path,
) -> anyhow::Result<RenderPlugin> {
    let mut settings = WgpuSettings::default();
    configure_render_settings(&mut settings);
    #[cfg(windows)]
    return guarded::render_plugin(app, data_root, settings);
    #[cfg(not(windows))]
    {
        let _ = (app, data_root);
        if let Some(backends) =
            preferred_render_backends(std::env::var_os("WGPU_BACKEND").as_deref())
        {
            settings.backends = Some(backends);
        }
        Ok(RenderPlugin {
            render_creation: RenderCreation::Automatic(Box::new(settings)),
            ..Default::default()
        })
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

/// Preserves explicit backend choices and restricts Android to Vulkan.
#[cfg(not(windows))]
fn preferred_render_backends(explicit: Option<&OsStr>) -> Option<Backends> {
    if explicit.is_some() {
        None
    } else if cfg!(target_os = "android") {
        // Android's GLES backend lacks the vertex-stage storage buffers the renderer needs.
        Some(Backends::VULKAN)
    } else {
        None
    }
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

    #[cfg(not(windows))]
    #[test]
    fn explicit_backend_is_preserved_and_android_requires_vulkan() {
        use bevy::render::settings::Backends;
        use std::ffi::OsStr;

        assert_eq!(preferred_render_backends(Some(OsStr::new("dx12"))), None);
        let expected = cfg!(target_os = "android").then_some(Backends::VULKAN);
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
            assert_eq!(
                window.resize_constraints.min_width,
                ui::MIN_WINDOW_LOGICAL_SIZE[0]
            );
            assert_eq!(
                window.resize_constraints.min_height,
                ui::MIN_WINDOW_LOGICAL_SIZE[1]
            );
        }
    }
}

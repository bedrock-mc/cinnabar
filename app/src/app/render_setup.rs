//! Renderer setup shared by direct and launcher sessions.

use std::num::NonZeroU32;

mod guarded;

use bevy::{
    render::settings::WgpuSettings,
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

/// Initializes the renderer after winit has supplied the real display connection.
pub(super) struct RendererSetup {
    pub data_root: std::path::PathBuf,
    pub video_override: Option<launcher::graphics_backend::Backend>,
}

impl bevy::prelude::Plugin for RendererSetup {
    /// Keeps GL on the same display as Bevy's windows and installs first-frame recovery.
    fn build(&self, app: &mut bevy::prelude::App) {
        let mut settings = WgpuSettings::default();
        crate::gpu_probe::device::configure_render_settings(&mut settings);
        let renderer = guarded::render_plugin(app, &self.data_root, self.video_override, settings)
            .unwrap_or_else(|error| panic!("Renderer startup failed: {error:#}"));
        app.add_plugins(renderer);
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
        crate::gpu_probe::device::configure_render_settings(&mut settings);
        assert!(matches!(
            settings.dx12_shader_compiler,
            wgpu::Dx12Compiler::Fxc
        ));
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

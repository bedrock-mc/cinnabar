//! Renderer setup shared by direct and launcher sessions.

use std::num::NonZeroU32;

mod device;
mod guarded;
mod selection;

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
    pub graphics_api_revision: u64,
    pub video_override: Option<launcher::graphics_backend::Backend>,
}

impl bevy::prelude::Plugin for RendererSetup {
    /// Keeps GL on the same display as Bevy's windows and installs first-frame recovery.
    fn build(&self, app: &mut bevy::prelude::App) {
        let mut settings = WgpuSettings::default();
        render::device_settings::configure(&mut settings);
        let renderer = guarded::render_plugin(
            app,
            &self.data_root,
            self.video_override,
            self.graphics_api_revision,
            settings,
        )
        .unwrap_or_else(|error| panic!("Renderer startup failed: {error:#}"));
        app.add_plugins(renderer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn browser_requests_portable_webgpu_device_settings() {
        let mut settings = WgpuSettings::default();
        render::device_settings::configure(&mut settings);
        assert!(matches!(
            settings.priority,
            bevy::render::settings::WgpuSettingsPriority::WebGPU
        ));
        assert_eq!(settings.backends, Some(wgpu::Backends::BROWSER_WEBGPU));
    }

    #[test]
    fn shader_compiler_does_not_inherit_a_launch_directory_dxc() {
        let mut settings = WgpuSettings {
            dx12_shader_compiler: wgpu::Dx12Compiler::DynamicDxc {
                dxc_path: "dxcompiler.dll".into(),
            },
            ..Default::default()
        };
        render::device_settings::configure(&mut settings);
        assert!(matches!(
            settings.dx12_shader_compiler,
            wgpu::Dx12Compiler::Fxc
        ));
    }

    #[test]
    fn primary_window_requests_the_vsync_frame_latency() {
        // Windows request FIFO until the surface is probed, whatever VSync choice they serve.
        for vsync in [true, false] {
            let latency = render::frame_latency_for_vsync(vsync);
            let window = primary_window(String::new(), PresentMode::Fifo, latency);
            assert_eq!(window.desired_maximum_frame_latency, Some(latency));
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

/// Cancels an unfinished renderer only when ordinary setup returns before the event loop.
pub(super) struct BootExit {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    armed: bool,
}

#[derive(bevy::prelude::Resource, Default)]
struct BootCancellation(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl BootExit {
    /// Links normal setup returns to the renderer guard without borrowing the app.
    pub(super) fn watch(app: &mut bevy::prelude::App) -> Self {
        app.init_resource::<BootCancellation>();
        Self {
            cancelled: app.world().resource::<BootCancellation>().0.clone(),
            armed: true,
        }
    }

    /// Runtime exits and crashes use the renderer's existing presentation and recovery rules.
    pub(super) fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for BootExit {
    /// Marks an orderly early return; unwinding still preserves crash recovery.
    fn drop(&mut self) {
        if self.armed && !std::thread::panicking() {
            self.cancelled
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
}

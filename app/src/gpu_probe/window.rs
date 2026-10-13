//! A hidden native surface for the isolated GPU helper, without Bevy or game assets.

use super::*;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

struct Probe {
    request: Request,
    result: Option<Result<String>>,
}

impl ApplicationHandler for Probe {
    /// Creates the hidden surface after the platform permits window creation.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.result.is_some() {
            return;
        }
        self.result = Some((|| {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title(launcher::PRODUCT_NAME)
                        .with_visible(false)
                        .with_inner_size(winit::dpi::PhysicalSize::new(16, 16)),
                )?,
            );
            bevy::tasks::block_on(execute(
                &self.request,
                Some(Box::new(window.clone())),
                Some(wgpu::SurfaceTarget::Window(Box::new(window))),
            ))
        })());
        event_loop.exit();
    }
    /// The helper does not process game input or redraw loops.
    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window: WindowId,
        _event: WindowEvent,
    ) {
    }
}

/// Runs one helper operation on the OS main thread and exits its event loop immediately.
pub(super) fn run(request: Request) -> Result<String> {
    let mut builder = EventLoop::builder();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_activate_ignoring_other_apps(false);
    }
    let event_loop = builder.build()?;
    let mut probe = Probe {
        request,
        result: None,
    };
    event_loop.run_app(&mut probe)?;
    probe.result.context("GPU helper window was not created")?
}

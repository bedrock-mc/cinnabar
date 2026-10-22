//! Event-loop owner and original loading artwork, independent of game initialization.

use crate::{
    canvas::{Canvas, Image, Rect, Text},
    gpu::Gpu,
};
use anyhow::{Context, Result};
use launcher::{
    install_layout::InstallLayout,
    startup::{Event, Message, Model, appearance},
};
use std::{
    io::{BufRead, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId, WindowLevel},
};

struct Screen {
    model: Model,
    initial_size: LogicalSize<f64>,
    text: Text,
    logo: Option<Image>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    epoch: Instant,
    next_redraw: Instant,
    shown: bool,
    submitted: bool,
    completed: Arc<AtomicBool>,
    failure: Option<anyhow::Error>,
}

/// Runs until readiness, parent EOF, cancellation, or a display error.
pub(super) fn run() -> Result<()> {
    let dimensions = std::env::args()
        .skip(2)
        .map(|value| value.parse::<f64>())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    anyhow::ensure!(
        dimensions.len() == 2
            && dimensions
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
        "startup screen needs a valid width and height"
    );
    let layout = InstallLayout::discover()?;
    let mut screen = Screen {
        model: Model::default(),
        initial_size: LogicalSize::new(dimensions[0], dimensions[1]),
        text: crate::text(&layout)?,
        logo: crate::decode_logo(),
        window: None,
        gpu: None,
        epoch: Instant::now(),
        next_redraw: Instant::now(),
        shown: false,
        submitted: false,
        completed: Arc::default(),
        failure: None,
    };
    let event_loop = EventLoop::<Option<Message>>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if let Ok(message) = serde_json::from_str(&line) {
                if proxy.send_event(Some(message)).is_err() {
                    return;
                }
            }
        }
        let _ = proxy.send_event(None);
    });
    event_loop.run_app(&mut screen)?;
    screen.failure.map_or(Ok(()), Err)
}

/// Sends only structured events on stdout; diagnostics belong on stderr.
fn report(event: Event) {
    let mut output = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut output, &event);
    let _ = writeln!(output);
    let _ = output.flush();
}

impl Screen {
    /// Uploads a restrained full-window loading screen with an indeterminate activity indicator.
    fn paint(&mut self) -> Result<bool> {
        let (Some(window), Some(gpu)) = (&self.window, &mut self.gpu) else {
            return Ok(false);
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(false);
        }
        let (w, h) = (size.width as f32, size.height as f32);
        let layout = &appearance().layout;
        let scale = (w / layout.design_size[0])
            .min(h / layout.design_size[1])
            .max(layout.minimum_scale);
        let mut canvas = Canvas::new(size.width, size.height);
        let colors = appearance();
        canvas.fill(
            Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
            colors.background,
        );
        if let Some(logo) = &self.logo {
            let width = (layout.logo_width * scale).min(w - layout.side_margin * scale);
            let height = width * logo.height as f32 / logo.width as f32;
            canvas.image(
                logo,
                Rect {
                    x: (w - width) * 0.5,
                    y: h * layout.logo_center_y - height * 0.5,
                    w: width,
                    h: height,
                },
            );
        }
        let label = self.model.stage().label();
        let px = layout.label_size * scale;
        let width = self.text.width(label, px);
        self.text.draw(
            &mut canvas,
            (w - width) * 0.5,
            h * layout.label_top,
            px,
            colors.foreground,
            label,
        );
        let active = (self.epoch.elapsed().as_millis() / u128::from(layout.step_millis)) % 3;
        for i in 0..3 {
            let mut color = colors.accent;
            if i != active {
                color = colors.foreground;
                color[3] = 70;
            }
            canvas.fill(
                Rect {
                    x: w * 0.5 + (i as f32 - 1.0) * layout.dot_spacing * scale
                        - layout.dot_size * 0.5 * scale,
                    y: h * layout.indicator_top,
                    w: layout.dot_size * scale,
                    h: layout.dot_size * scale,
                },
                color,
            );
        }
        gpu.set_overlay(size.width, size.height, &canvas.pixels);
        gpu.draw(&render_model::panorama::launcher_view(
            0.0,
            w / h,
            crate::PANORAMA_TINT,
        ))
    }
}

impl ApplicationHandler<Option<Message>> for Screen {
    /// Creates one independent native window, kept above the initializing game until ready.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.model.finished() {
            return;
        }
        let opened = (|| -> Result<_> {
            let window = Arc::new(
                event_loop.create_window(
                    crate::window::attributes(self.initial_size)
                        .with_window_level(WindowLevel::AlwaysOnTop),
                )?,
            );
            let faces = crate::built_in_faces().context("decode loading artwork")?;
            let gpu = Gpu::new(window.clone(), &faces)?;
            Ok((window, gpu))
        })();
        match opened {
            Ok((window, gpu)) => {
                self.window = Some(window);
                self.gpu = Some(gpu);
            }
            Err(error) => {
                self.failure = Some(error);
                event_loop.exit();
            }
        }
    }

    /// Parent EOF and readiness are terminal; late phase messages cannot revive this window.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Option<Message>) {
        if let Some(message) = event {
            self.model.apply(message);
        } else {
            self.model.finish();
        }
        if self.model.finished() {
            event_loop.exit();
        }
    }

    /// Closing the loading window cancels startup rather than silently hiding feedback.
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                report(Event::Cancelled);
                self.model.finish();
                event_loop.exit();
            }
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => match self.paint() {
                Ok(drawn) => {
                    if let Some(gpu) = &self.gpu {
                        if drawn && !self.submitted {
                            self.submitted = true;
                            let completed = self.completed.clone();
                            gpu.on_frame_complete(move || completed.store(true, Ordering::Release));
                        }
                        gpu.poll();
                    }
                    if !self.shown && self.completed.load(Ordering::Acquire) {
                        self.shown = true;
                        report(Event::Shown);
                    }
                }
                Err(error) => {
                    self.failure = Some(error);
                    event_loop.exit();
                }
            },
            _ => {}
        }
    }

    /// Limits feedback to twenty frames per second while initialization runs independently.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            self.next_redraw = now + Duration::from_millis(appearance().layout.frame_millis);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_redraw));
    }
}

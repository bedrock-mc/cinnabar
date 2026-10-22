//! The crash window's event loop: buttons, details scrolling and repaint pacing.

use std::{sync::Arc, time::Instant};

use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Window, WindowId},
};

use bootstrap_ui::{
    OVERLAY_INTERVAL, PANORAMA_TINT, Preferences,
    canvas::{Canvas, Image, Rect, Text},
    gpu,
    input::{self, Command, Input, Source, pointer::Pointer},
};

use super::{
    CRASH_SIZE, Choice, Copied, CrashAction, CrashScreen, CrashSummary, desktop,
    discord::DiscordMark,
    summary::DetailLine,
    view::{self, LogFrame},
};

/// Log lines one mouse-wheel notch scrolls.
const WHEEL_LINES: f32 = 3.0;

/// A held thumb: the pointer's offset from the thumb's top when it was grabbed.
#[derive(Clone, Copy)]
struct Drag {
    grab: f32,
}

pub(super) struct CrashApp {
    summary: CrashSummary,
    details: Vec<DetailLine>,
    discord: DiscordMark,
    text: Text,
    /// Uploaded once the GPU exists.
    faces: Option<render_model::PanoramaFaces>,
    logo: Option<Image>,
    window: Option<Arc<Window>>,
    gpu: Option<gpu::Gpu>,
    screen: CrashScreen,
    hits: Vec<(CrashAction, Rect)>,
    log: Option<LogFrame>,
    scroll: f32,
    drag: Option<Drag>,
    thumb_hovered: bool,
    pointer: Pointer,
    hovered: Option<CrashAction>,
    input: Input<CrashAction>,
    controllers: Option<gilrs::Gilrs>,
    preferences: Preferences,
    modifiers: ModifiersState,
    overlay_dirty: bool,
    overlay_at: Option<Instant>,
    epoch: Instant,
    choice: Option<Choice>,
    layout: Option<launcher::install_layout::InstallLayout>,
    upload: Option<std::thread::JoinHandle<()>>,
}

impl CrashApp {
    pub(super) fn new(
        summary: CrashSummary,
        text: Text,
        faces: render_model::PanoramaFaces,
        logo: Option<Image>,
        preferences: Preferences,
        layout: Option<launcher::install_layout::InstallLayout>,
    ) -> Self {
        let screen = CrashScreen {
            expanded: false,
            queued: false,
            copied: None,
        };
        let mut input = Input::default();
        input.reset(&screen);
        Self {
            details: summary.detail_lines(),
            discord: DiscordMark::new(),
            summary,
            text,
            faces: Some(faces),
            logo,
            window: None,
            gpu: None,
            screen,
            hits: Vec::new(),
            log: None,
            // The newest lines matter most: open scrolled to the end.
            scroll: f32::MAX,
            drag: None,
            thumb_hovered: false,
            pointer: Pointer::default(),
            hovered: None,
            input,
            controllers: bootstrap_ui::controllers(),
            preferences,
            modifiers: ModifiersState::default(),
            overlay_dirty: true,
            overlay_at: None,
            epoch: Instant::now(),
            choice: None,
            layout,
            upload: None,
        }
    }

    /// What the player chose, once the window has closed.
    pub(super) const fn choice(&self) -> Option<Choice> {
        self.choice
    }

    fn act(&mut self, action: CrashAction, event_loop: &ActiveEventLoop) {
        match action {
            CrashAction::Restart => self.finish(Choice::Restart, event_loop),
            CrashAction::Close => self.finish(Choice::Close, event_loop),
            CrashAction::OpenFolder => desktop::open_folder(&self.summary.folder),
            CrashAction::Discord => launcher_host::desktop::open_url(launcher::DISCORD_INVITE_URL),
            CrashAction::SendReport => {
                use launcher_host::lifecycle::crash::upload;
                if let Some(layout) = &self.layout
                    && upload::Policy::load(layout).is_some()
                    && upload::consent(
                        &self.summary.folder,
                        crash_report::upload::state::Consent::User,
                        upload::now_ms(),
                    )
                    .is_ok()
                {
                    self.screen.queued = true;
                    queue_upload(&mut self.upload, || upload::spawn_send_due(layout.clone()));
                }
            }
            CrashAction::CopyReport => {
                let outcome = match super::copy_report(&self.summary) {
                    Ok(()) => Copied::Done,
                    Err(error) => {
                        tracing::warn!("copy report failed: {error}");
                        Copied::Failed
                    }
                };
                self.screen.copied = Some((outcome, Instant::now()));
            }
            CrashAction::ToggleDetails => {
                self.screen.expanded = !self.screen.expanded;
                self.scroll = f32::MAX;
                self.drag = None;
            }
        }
        self.overlay_dirty = true;
    }

    /// Transfers the pending send so the window's caller can apply the choice before waiting.
    pub(super) fn take_upload(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.upload.take()
    }

    fn finish(&mut self, choice: Choice, event_loop: &ActiveEventLoop) {
        self.choice = Some(choice);
        event_loop.exit();
    }

    fn hit(&self) -> Option<CrashAction> {
        let (x, y) = self.pointer.hit_position()?;
        self.hits
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(action, _)| *action)
    }

    /// The details scrollbar under the pointer, while details are shown.
    fn track_hit(&self) -> Option<LogFrame> {
        let (x, y) = self.pointer.hit_position()?;
        self.log.filter(|log| {
            Rect {
                // A wider target than the drawn bar keeps the thin thumb easy to grab.
                x: log.track.x - log.track.w,
                w: log.track.w * 3.0,
                ..log.track
            }
            .contains(x, y)
        })
    }

    /// Scrolls the details log by `lines`; true when it moved.
    fn scroll_by(&mut self, lines: f32) -> bool {
        let Some(log) = self.log else {
            return false;
        };
        let next = (log.scroll + lines).clamp(0.0, log.max_scroll);
        let moved = next != log.scroll;
        self.scroll = next;
        self.log = Some(LogFrame {
            scroll: next,
            ..log
        });
        self.overlay_dirty |= moved;
        moved
    }

    /// Moves the thumb so its top sits `grab` above the pointer.
    fn drag_to(&mut self, y: f32, grab: f32) {
        let Some(log) = self.log else {
            return;
        };
        let travel = log.track.h - log.thumb.h;
        let fraction = if travel > 0.0 {
            ((y - grab - log.track.y) / travel).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let target = fraction * log.max_scroll;
        self.scroll_by(target - log.scroll);
    }

    /// Details scrolling keys; arrows stay with focus navigation as in the setup window.
    fn scroll_key(&mut self, key: &Key) -> bool {
        let Some(log) = self.log else {
            return false;
        };
        let page = (log.visible - 1.0).max(1.0);
        match key {
            Key::Named(NamedKey::PageUp) => self.scroll_by(-page),
            Key::Named(NamedKey::PageDown) => self.scroll_by(page),
            Key::Named(NamedKey::Home) => self.scroll_by(-log.scroll),
            Key::Named(NamedKey::End) => self.scroll_by(log.max_scroll - log.scroll),
            _ => return false,
        };
        true
    }

    fn repaint_overlay(&mut self) {
        let (Some(window), Some(gpu)) = (&self.window, &mut self.gpu) else {
            return;
        };
        let size = window.inner_size();
        let mut canvas = Canvas::new(size.width.max(1), size.height.max(1));
        let frame = view::draw(
            &mut canvas,
            &mut self.text,
            &self.summary,
            &self.details,
            &mut self.discord,
            &self.screen,
            &view::Style {
                rem: self.preferences.rem(size),
                appearance: self.preferences.appearance,
                hovered: self.hovered,
                focused: self.input.focused.filter(|_| self.input.focus_visible),
                pressed: self.input.pressed(self.hovered),
                logo: self.logo.as_ref(),
                scroll: self.scroll,
                thumb_hovered: self.thumb_hovered,
                thumb_dragged: self.drag.is_some(),
            },
        );
        gpu.set_overlay(canvas.width, canvas.height, &canvas.pixels);
        self.hits = frame.hits;
        self.log = frame.log;
        if let Some(log) = self.log {
            self.scroll = log.scroll;
        }
        let hovered = self.hit();
        let thumb_hovered = self.track_hit().is_some();
        self.overlay_dirty = hovered != self.hovered || thumb_hovered != self.thumb_hovered;
        self.hovered = hovered;
        self.thumb_hovered = thumb_hovered;
        self.overlay_at = Some(Instant::now());
    }

    /// Starts a thumb drag for a press on the scrollbar; true when the press was taken.
    fn press_scrollbar(&mut self) -> bool {
        let (Some(log), Some((_, y))) = (self.track_hit(), self.pointer.hit_position()) else {
            return false;
        };
        let grab = if (log.thumb.y..log.thumb.y + log.thumb.h).contains(&y) {
            y - log.thumb.y
        } else {
            log.thumb.h * 0.5
        };
        self.drag = Some(Drag { grab });
        self.drag_to(y, grab);
        self.overlay_dirty = true;
        true
    }
}

/// Reaps finished sends and starts a requested worker, keeping any active send in place.
pub(super) fn queue_upload(
    worker: &mut Option<std::thread::JoinHandle<()>>,
    spawn: impl FnOnce() -> Option<std::thread::JoinHandle<()>>,
) {
    if worker
        .as_ref()
        .is_some_and(std::thread::JoinHandle::is_finished)
        && let Some(finished) = worker.take()
    {
        let _ = finished.join();
    }
    if worker.is_none() {
        *worker = spawn();
    }
}

impl ApplicationHandler for CrashApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let Ok(window) = event_loop.create_window(bootstrap_ui::window::attributes(CRASH_SIZE))
        else {
            self.finish(Choice::Unavailable, event_loop);
            return;
        };
        let window = Arc::new(window);
        let Some(faces) = self.faces.take() else {
            return;
        };
        match gpu::Gpu::new(Arc::clone(&window), &faces) {
            Ok(gpu) => self.gpu = Some(gpu),
            Err(error) => {
                tracing::error!("crash window unavailable: {error:#}");
                self.finish(Choice::Unavailable, event_loop);
                return;
            }
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.finish(Choice::Close, event_loop),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                self.overlay_dirty = true;
            }
            WindowEvent::ScaleFactorChanged { .. } => self.overlay_dirty = true,
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer.position = Some((position.x as f32, position.y as f32));
                if let Some(Drag { grab }) = self.drag {
                    self.drag_to(position.y as f32, grab);
                    return;
                }
                let hovered = self.hit();
                let thumb_hovered = self.track_hit().is_some();
                if hovered != self.hovered || thumb_hovered != self.thumb_hovered {
                    self.hovered = hovered;
                    self.thumb_hovered = thumb_hovered;
                    self.input.focus_visible = false;
                    self.overlay_dirty = true;
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer.position = None;
                self.hovered = None;
                self.thumb_hovered = false;
                self.overlay_dirty = true;
            }
            WindowEvent::Focused(active) => {
                self.pointer.focus(active);
                self.hovered = self.hit();
                self.overlay_dirty = true;
                if !active {
                    self.input.blur();
                    self.drag = None;
                    self.modifiers = ModifiersState::default();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * WHEEL_LINES,
                    MouseScrollDelta::PixelDelta(position) => {
                        -(position.y as f32) / self.log.map_or(1.0, |log| log.line.max(1.0))
                    }
                };
                self.scroll_by(lines);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Released && self.drag.take().is_some() {
                    self.overlay_dirty = true;
                    return;
                }
                if state == ElementState::Pressed
                    && self.hovered.is_none()
                    && self.press_scrollbar()
                {
                    return;
                }
                let command = if state == ElementState::Pressed {
                    Command::Press
                } else {
                    Command::Release
                };
                let (action, changed) =
                    self.input
                        .apply(&self.screen, Source::Pointer, command, self.hovered);
                self.overlay_dirty |= changed;
                if let Some(action) = action {
                    self.act(action, event_loop);
                }
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    state, logical_key, ..
                },
                is_synthetic: false,
                ..
            } if state == ElementState::Pressed && self.scroll_key(&logical_key) => {}
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state,
                        logical_key,
                        repeat: false,
                        ..
                    },
                is_synthetic,
                ..
            } => {
                let (action, changed) = self.input.keyboard(
                    &self.screen,
                    &logical_key,
                    state,
                    self.modifiers.shift_key(),
                    is_synthetic,
                );
                self.overlay_dirty |= changed;
                if let Some(action) = action {
                    self.act(action, event_loop);
                }
            }
            WindowEvent::RedrawRequested => {
                let (Some(window), Some(gpu)) = (&self.window, &mut self.gpu) else {
                    return;
                };
                let size = window.inner_size();
                let aspect = size.width.max(1) as f32 / size.height.max(1) as f32;
                let seconds = self.epoch.elapsed().as_secs_f32();
                if let Err(error) = gpu.draw(&render_model::panorama::launcher_view(
                    seconds,
                    aspect,
                    PANORAMA_TINT,
                )) {
                    tracing::error!("crash window display failed: {error}");
                    self.finish(Choice::Unavailable, event_loop);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        while let Some(event) = self.controllers.as_mut().and_then(gilrs::Gilrs::next_event) {
            if !self.pointer.active {
                continue;
            }
            // Bumpers page through the details; everything else navigates like setup.
            if let gilrs::EventType::ButtonPressed(button, _) = event.event {
                let page = self.log.map_or(0.0, |log| (log.visible - 1.0).max(1.0));
                match button {
                    gilrs::Button::LeftTrigger => {
                        self.scroll_by(-page);
                        continue;
                    }
                    gilrs::Button::RightTrigger => {
                        self.scroll_by(page);
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(command) = input::controller(event.event, &self.preferences.settings) {
                let (action, changed) =
                    self.input
                        .apply(&self.screen, Source::Controller, command, None);
                self.overlay_dirty |= changed;
                if let Some(action) = action {
                    self.act(action, event_loop);
                }
            }
        }
        self.overlay_dirty |= self.screen.expire(Instant::now());
        let due = self
            .overlay_at
            .is_none_or(|at| at.elapsed() >= OVERLAY_INTERVAL);
        if self.overlay_dirty && due {
            self.repaint_overlay();
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

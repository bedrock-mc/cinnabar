//! The setup window's event loop: input, worker lifecycle and repaint pacing.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Window, WindowId},
};

use super::{
    super::{
        EULA_URL, prepare, record_consent, reporter,
        screen::{self, Action, Effect, Meter, Screen},
        status::Status,
    },
    EXIT_QUIT, EXIT_UNAVAILABLE,
    canvas::{Canvas, Image, Rect, Text},
    gpu,
    input::{Command, Input, Source},
    view,
};
use crate::install_layout::InstallLayout;

const PANORAMA_TINT: [f32; 4] = [0.0, 0.0, 0.0, 0.3];
/// Progress repaints at most this often; the panorama animates every frame.
const OVERLAY_INTERVAL: Duration = Duration::from_millis(66);
/// How long "Starting Cinnabar" stays up before the game window replaces it.
const DONE_LINGER: Duration = Duration::from_millis(700);
/// How long a cancelled worker gets to kill its step before the process exits anyway.
const CANCEL_GRACE: Duration = Duration::from_secs(3);

struct Worker {
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

pub(super) struct SetupApp {
    layout: InstallLayout,
    /// Earlier carriers exist, so screens say "Updating".
    updating: bool,
    text: Text,
    /// Uploaded once the GPU exists.
    faces: Option<render_model::PanoramaFaces>,
    logo: Option<Image>,
    window: Option<Arc<Window>>,
    gpu: Option<gpu::Gpu>,
    screen: Screen,
    meter: Meter,
    sender: Sender<Status>,
    updates: Receiver<Status>,
    worker: Option<Worker>,
    hits: Vec<(Action, Rect)>,
    cursor: Option<(f32, f32)>,
    hovered: Option<Action>,
    input: Input,
    controllers: Option<gilrs::Gilrs>,
    modifiers: ModifiersState,
    active: bool,
    appearance: client_ui::oreui_theme::Appearance,
    gui_scale_offset: i8,
    overlay_dirty: bool,
    overlay_at: Option<Instant>,
    done_at: Option<Instant>,
    epoch: Instant,
    exit: Option<i32>,
}

impl SetupApp {
    pub(super) fn new(
        layout: InstallLayout,
        text: Text,
        faces: render_model::PanoramaFaces,
        logo: Option<Image>,
        consented: bool,
        updating: bool,
    ) -> Self {
        let (sender, updates) = mpsc::channel();
        let screen = if consented {
            Screen::Starting
        } else {
            Screen::Consent
        };
        let mut input = Input::default();
        input.reset(&screen);
        let settings = launcher::menu::settings_options::SettingsOptions::load(
            &layout
                .server_file()
                .with_file_name(launcher::menu::settings_options::SETTINGS_FILE),
        );
        let appearance = client_ui::oreui_theme::Appearance::from_dark(settings.oreui_dark_mode());
        let gui_scale_offset = crate::menu::video_settings::load(&layout.user_config_root)
            .unwrap_or_default()
            .gui_scale_offset;
        let controllers = gilrs::Gilrs::new()
            .map_err(|error| {
                eprintln!("setup controller input unavailable: {error}");
            })
            .ok();
        Self {
            layout,
            text,
            updating,
            faces: Some(faces),
            logo,
            window: None,
            gpu: None,
            screen,
            meter: Meter::default(),
            sender,
            updates,
            worker: None,
            hits: Vec::new(),
            cursor: None,
            hovered: None,
            input,
            controllers,
            modifiers: ModifiersState::default(),
            active: true,
            appearance,
            gui_scale_offset,
            overlay_dirty: true,
            overlay_at: None,
            done_at: None,
            epoch: Instant::now(),
            exit: None,
        }
    }

    /// The exit code chosen by the user or the flow, if any.
    pub(super) const fn exit_code(&self) -> Option<i32> {
        self.exit
    }

    fn start_worker(&mut self) {
        let (layout, sender) = (self.layout.clone(), self.sender.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let handle = std::thread::spawn(move || {
            if let Err(error) = record_consent(&layout) {
                let _ = sender.send(Status::failed("Failed", &format!("{error:#}")));
                return;
            }
            let mut report = reporter(&layout, |status| {
                let _ = sender.send(status.clone());
            });
            let _ = prepare(&layout, &flag, &mut report);
        });
        self.meter = Meter::default();
        self.worker = Some(Worker { cancel, handle });
    }

    /// Cancels a running worker and waits briefly for it to kill its step.
    pub(super) fn stop_worker(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        worker.cancel.store(true, Ordering::Relaxed);
        let deadline = Instant::now() + CANCEL_GRACE;
        while !worker.handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn act(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        let (next, effect) = self.screen.on_action(action);
        if let Some(next) = next {
            self.input.reset(&next);
            self.hovered = None;
            self.screen = next;
            self.overlay_dirty = true;
        }
        match effect {
            Effect::None => {}
            Effect::StartWorker => self.start_worker(),
            Effect::OpenEula => crate::desktop::open_url(EULA_URL),
            Effect::Quit => self.finish(EXIT_QUIT, event_loop),
        }
    }

    fn finish(&mut self, code: i32, event_loop: &ActiveEventLoop) {
        self.stop_worker();
        self.exit = Some(code);
        event_loop.exit();
    }

    fn hit(&self) -> Option<Action> {
        let (x, y) = self.cursor?;
        self.hits
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(action, _)| *action)
    }

    fn repaint_overlay(&mut self) {
        let (Some(window), Some(gpu)) = (&self.window, &mut self.gpu) else {
            return;
        };
        let size = window.inner_size();
        let mut canvas = Canvas::new(size.width.max(1), size.height.max(1));
        let log = self.layout.log_dir().join("first-run.log");
        let log_hint = log.display().to_string();
        self.hits = view::draw(
            &mut canvas,
            &mut self.text,
            &self.screen,
            &view::Style {
                rem: f32::from(
                    ui::DesktopGuiScale::for_window([size.width, size.height])
                        .scale_for_offset(self.gui_scale_offset),
                ) * client_ui::oreui_theme::GUI_PIXELS_PER_REM,
                appearance: self.appearance,
                hovered: self.hovered,
                focused: self.input.focused.filter(|_| self.input.focus_visible),
                pressed: self.input.pressed(self.hovered),
                updating: self.updating,
                logo: self.logo.as_ref(),
                log_hint: &log_hint,
            },
        );
        gpu.set_overlay(canvas.width, canvas.height, &canvas.pixels);
        let hovered = self.hit();
        self.overlay_dirty = hovered != self.hovered;
        self.hovered = hovered;
        self.overlay_at = Some(Instant::now());
    }
}

impl ApplicationHandler for SetupApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(launcher::PRODUCT_NAME)
            .with_window_icon(Some(crate::window_icon::icon()))
            .with_inner_size(LogicalSize::new(1024.0, 640.0))
            .with_min_inner_size(LogicalSize::new(560.0, 420.0));
        let Ok(window) = event_loop.create_window(attributes) else {
            self.finish(EXIT_UNAVAILABLE, event_loop);
            return;
        };
        let window = Arc::new(window);
        let Some(faces) = self.faces.take() else {
            return;
        };
        match gpu::Gpu::new(Arc::clone(&window), &faces) {
            Ok(gpu) => self.gpu = Some(gpu),
            Err(error) => {
                eprintln!("first-run window unavailable: {error:#}");
                self.finish(EXIT_UNAVAILABLE, event_loop);
                return;
            }
        }
        self.window = Some(window);
        if self.screen == Screen::Starting {
            self.start_worker();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested if self.screen == Screen::Done => {
                self.finish(0, event_loop)
            }
            WindowEvent::CloseRequested => self.finish(EXIT_QUIT, event_loop),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                self.overlay_dirty = true;
            }
            WindowEvent::ScaleFactorChanged { .. } => self.overlay_dirty = true,
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some((position.x as f32, position.y as f32));
                let hovered = self.hit();
                if hovered != self.hovered {
                    self.hovered = hovered;
                    self.input.focus_visible = false;
                    self.overlay_dirty = true;
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.hovered = None;
                self.overlay_dirty = true;
            }
            WindowEvent::Focused(active) => {
                self.active = active;
                self.overlay_dirty = true;
                if !active {
                    self.input.blur();
                    self.cursor = None;
                    self.hovered = None;
                    self.modifiers = ModifiersState::default();
                    self.overlay_dirty = true;
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Pressed {
                    self.input
                        .press(&self.screen, Source::Pointer, self.hovered);
                } else if let Some(action) = self.input.release(Source::Pointer, self.hovered) {
                    self.act(action, event_loop);
                }
                self.overlay_dirty = true;
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state,
                        logical_key,
                        repeat: false,
                        ..
                    },
                ..
            } => {
                match logical_key {
                    Key::Named(NamedKey::Tab | NamedKey::ArrowLeft | NamedKey::ArrowUp)
                        if state == ElementState::Pressed =>
                    {
                        let backwards =
                            logical_key != Key::Named(NamedKey::Tab) || self.modifiers.shift_key();
                        self.input.navigate(&self.screen, backwards);
                    }
                    Key::Named(NamedKey::ArrowRight | NamedKey::ArrowDown)
                        if state == ElementState::Pressed =>
                    {
                        self.input.navigate(&self.screen, false)
                    }
                    Key::Named(NamedKey::Enter | NamedKey::Space) => {
                        if state == ElementState::Pressed {
                            self.input.press(&self.screen, Source::Keyboard, None);
                        } else if let Some(action) = self.input.release(Source::Keyboard, None) {
                            self.act(action, event_loop);
                        }
                    }
                    Key::Named(NamedKey::Escape) if state == ElementState::Pressed => {
                        self.act(Action::Quit, event_loop)
                    }
                    _ => {}
                }
                self.overlay_dirty = true;
            }
            WindowEvent::RedrawRequested => {
                let (Some(window), Some(gpu)) = (&self.window, &mut self.gpu) else {
                    return;
                };
                let size = window.inner_size();
                let aspect = size.width.max(1) as f32 / size.height.max(1) as f32;
                let seconds = self.epoch.elapsed().as_secs_f32();
                if let Err(error) =
                    gpu.draw(&client_ui::ui_runtime::presentation::forms::launcher_view(
                        seconds,
                        aspect,
                        PANORAMA_TINT,
                    ))
                {
                    eprintln!("setup display failed: {error}");
                    self.finish(EXIT_UNAVAILABLE, event_loop);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        while let Some(event) = self.controllers.as_mut().and_then(gilrs::Gilrs::next_event) {
            if !self.active {
                continue;
            }
            if let Some(command) = super::input::controller(event.event) {
                match command {
                    Command::Navigate(backwards) => self.input.navigate(&self.screen, backwards),
                    Command::Press => self.input.press(&self.screen, Source::Controller, None),
                    Command::Release => {
                        if let Some(action) = self.input.release(Source::Controller, None) {
                            self.act(action, event_loop);
                        }
                    }
                    Command::Cancel => self.act(Action::Quit, event_loop),
                    Command::Blur => self.input.blur(),
                }
                self.overlay_dirty = true;
            }
        }
        let now = Instant::now();
        while let Ok(status) = self.updates.try_recv() {
            let next = screen::from_status(&status, &mut self.meter, now);
            self.input.update_screen(&self.screen, &next);
            if self.screen.actions() != next.actions() {
                self.hovered = None;
            }
            self.screen = next;
            self.overlay_dirty = true;
        }
        if self.screen == Screen::Done {
            let done_at = *self.done_at.get_or_insert(now);
            if now.duration_since(done_at) >= DONE_LINGER {
                self.finish(0, event_loop);
                return;
            }
        }
        let due = self
            .overlay_at
            .is_none_or(|at| now.duration_since(at) >= OVERLAY_INTERVAL);
        if self.overlay_dirty && due {
            self.repaint_overlay();
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

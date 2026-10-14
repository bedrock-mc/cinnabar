//! The first-run setup window and the crash report window. Each runs in its own process: winit
//! allows one event loop per process, and the game opens its own once setup finishes.

mod app;
mod canvas;
mod crash;
mod gpu;
mod input;
mod view;

use std::process::Command;

use winit::{
    dpi::{LogicalSize, PhysicalSize},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowAttributes},
};

use canvas::{Image, Text};
pub use crash::{CrashSummary, EXIT_RESTART, run_crash_preview};
use launcher::{install_layout::InstallLayout, menu::settings_options::SettingsOptions};
use ui::oreui_theme::Appearance;

pub const SETUP_FLAG: &str = "--first-run-setup";
const EXIT_QUIT: i32 = 3;
/// No window could open; the parent falls back to native dialogs.
const EXIT_UNAVAILABLE: i32 = 4;
/// Where packaging installs the pinned UI font, under the resource root.
const FONT_DIR: &str = "fonts";
/// Cinnabar's own panorama behind the setup window, in pack face order (-Z, +X, +Z, -X, up,
/// down). Menus use the pack's faces once it is prepared.
const BUILT_IN_FACES: [&[u8]; 6] = [
    include_bytes!("../../../../assets/panorama/panorama_0.jpg"),
    include_bytes!("../../../../assets/panorama/panorama_1.jpg"),
    include_bytes!("../../../../assets/panorama/panorama_2.jpg"),
    include_bytes!("../../../../assets/panorama/panorama_3.jpg"),
    include_bytes!("../../../../assets/panorama/panorama_4.jpg"),
    include_bytes!("../../../../assets/panorama/panorama_5.jpg"),
];

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ChildOutcome {
    Prepared,
    Quit,
    Failed(i32),
    Unavailable,
}

/// Runs the setup window as `<current exe> --first-run-setup` and waits for it.
pub(super) fn run_in_child() -> ChildOutcome {
    let Ok(executable) = std::env::current_exe() else {
        return ChildOutcome::Unavailable;
    };
    match Command::new(executable).arg(SETUP_FLAG).status() {
        Ok(status) => outcome(status.code()),
        Err(_) => ChildOutcome::Unavailable,
    }
}

fn outcome(code: Option<i32>) -> ChildOutcome {
    match code {
        Some(0) => ChildOutcome::Prepared,
        Some(EXIT_QUIT) => ChildOutcome::Quit,
        Some(EXIT_UNAVAILABLE) => ChildOutcome::Unavailable,
        Some(code) => ChildOutcome::Failed(code),
        None => ChildOutcome::Failed(-1),
    }
}

/// The child-process entry point; returns its exit code.
pub fn run_setup_process() -> i32 {
    let Ok(layout) = InstallLayout::discover() else {
        return EXIT_UNAVAILABLE;
    };
    let font = super::prepare::ui_font_file(&layout.prep_kit())
        .map(|file| layout.resource_root.join(FONT_DIR).join(file));
    let Some(text) = font
        .as_ref()
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| Text::from_bytes(&bytes))
    else {
        eprintln!("first-run window unavailable: no usable UI font ({font:?})");
        return EXIT_UNAVAILABLE;
    };
    let Some(faces) = built_in_faces() else {
        return EXIT_UNAVAILABLE;
    };
    let Ok(event_loop) = EventLoop::new() else {
        return EXIT_UNAVAILABLE;
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let consented = super::env_consent() || super::consent_recorded(&layout);
    let updating = super::updating(&layout);
    let mut app = app::SetupApp::new(layout, text, faces, decode_logo(), consented, updating);
    let failed = event_loop.run_app(&mut app).is_err();
    app.stop_worker();
    app.exit_code()
        .unwrap_or(if failed { EXIT_UNAVAILABLE } else { EXIT_QUIT })
}

fn built_in_faces() -> Option<render_model::PanoramaFaces> {
    render_model::panorama::decode_faces(|face| Some(BUILT_IN_FACES[face]))
}

fn decode_logo() -> Option<Image> {
    let image = image::load_from_memory(launcher::branding::TITLE).ok()?;
    let rgba = image.into_rgba8();
    Some(Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// The setup window's default logical size.
const SETUP_SIZE: LogicalSize<f64> = LogicalSize::new(1024.0, 640.0);
/// The smallest window either bootstrap window allows.
const MIN_SIZE: LogicalSize<f64> = LogicalSize::new(560.0, 420.0);

/// A bootstrap window titled like the game window, so an embedding's `CINNABAR_WINDOW_TITLE`
/// renames both.
fn window_attributes(size: LogicalSize<f64>) -> WindowAttributes {
    titled_window_attributes(std::env::var("CINNABAR_WINDOW_TITLE").ok().as_deref(), size)
}

fn titled_window_attributes(title: Option<&str>, size: LogicalSize<f64>) -> WindowAttributes {
    Window::default_attributes()
        .with_title(launcher::window_title(title))
        .with_window_icon(Some(crate::window_icon::icon()))
        .with_inner_size(size)
        .with_min_inner_size(MIN_SIZE)
}

/// Opens connected controllers, or reports why controller input is unavailable.
fn controllers() -> Option<gilrs::Gilrs> {
    gilrs::Gilrs::new()
        .map_err(|error| eprintln!("bootstrap controller input unavailable: {error}"))
        .ok()
}

/// The saved menu choices the bootstrap windows honour without loading a resource pack.
#[derive(Default)]
struct Preferences {
    /// Controller confirm/back swapping.
    settings: SettingsOptions,
    appearance: Appearance,
    gui_scale_offset: i8,
}

impl Preferences {
    fn load(layout: &InstallLayout) -> Self {
        let settings = SettingsOptions::load(
            &layout
                .server_file()
                .with_file_name(launcher::menu::settings_options::SETTINGS_FILE),
        );
        let appearance = Appearance::from_dark(settings.oreui_dark_mode());
        let gui_scale_offset = launcher_host::video_settings::load(&layout.user_config_root)
            .unwrap_or_default()
            .gui_scale_offset;
        Self {
            settings,
            appearance,
            gui_scale_offset,
        }
    }

    /// Physical pixels per rem for a window of `size`, from the same GUI-scale authority as the
    /// menus.
    fn rem(&self, size: PhysicalSize<u32>) -> f32 {
        f32::from(
            ui::DesktopGuiScale::for_window([size.width, size.height])
                .scale_for_offset(self.gui_scale_offset),
        ) * ui::oreui_theme::GUI_PIXELS_PER_REM
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_exit_codes_map_to_outcomes() {
        assert_eq!(outcome(Some(0)), ChildOutcome::Prepared);
        assert_eq!(outcome(Some(EXIT_QUIT)), ChildOutcome::Quit);
        assert_eq!(outcome(Some(EXIT_UNAVAILABLE)), ChildOutcome::Unavailable);
        assert_eq!(outcome(Some(101)), ChildOutcome::Failed(101));
        assert_eq!(outcome(None), ChildOutcome::Failed(-1));
    }

    #[test]
    fn built_in_art_decodes() {
        assert!(decode_logo().is_some());
        assert!(built_in_faces().is_some());
    }

    #[test]
    fn windows_use_embedding_title_and_keep_default_for_blank_values() {
        assert_eq!(
            titled_window_attributes(Some("Zeno Client"), SETUP_SIZE).title,
            "Zeno Client"
        );
        for title in [None, Some(""), Some("  ")] {
            assert_eq!(
                titled_window_attributes(title, SETUP_SIZE).title,
                launcher::PRODUCT_NAME
            );
        }
    }
}

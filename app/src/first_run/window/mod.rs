//! The first-run setup window. It runs in a child process: winit allows one event loop per
//! process, and the game opens its own once setup finishes.

mod app;
mod canvas;
mod gpu;
mod view;

use std::process::Command;

use winit::event_loop::{ControlFlow, EventLoop};

use crate::install_layout::InstallLayout;
use canvas::{Image, Text};

pub(crate) const SETUP_FLAG: &str = "--first-run-setup";
const EXIT_QUIT: i32 = 3;
/// No window could open; the parent falls back to native dialogs.
const EXIT_UNAVAILABLE: i32 = 4;
/// Where packaging installs the pinned UI font, under the resource root.
const FONT_DIR: &str = "fonts";

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
pub(crate) fn run_setup_process() -> i32 {
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
    let Some(faces) = client_ui::ui_runtime::presentation::forms::built_in_faces() else {
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

fn decode_logo() -> Option<Image> {
    let image =
        image::load_from_memory(client_ui::ui_runtime::presentation::BUILT_IN_TITLE).ok()?;
    let rgba = image.into_rgba8();
    Some(Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
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
        assert!(client_ui::ui_runtime::presentation::forms::built_in_faces().is_some());
    }
}

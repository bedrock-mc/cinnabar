//! Server form states: overlapping and locked JSON-UI forms and data-driven script screens.
use super::*;

/// Server forms admitted through production form handling.
pub(super) const STATES: &[State] = &[
    #[cfg(feature = "test-support")]
    State {
        name: "form-overlap",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::FormOverlap,
    },
    #[cfg(feature = "test-support")]
    State {
        name: "ddui-custom-form",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::DataDriven(protocol::ddui::ScriptedScreen::CustomForm),
    },
    #[cfg(feature = "test-support")]
    State {
        name: "ddui-message-box",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::DataDriven(protocol::ddui::ScriptedScreen::MessageBox),
    },
    #[cfg(feature = "test-support")]
    State {
        name: "form-control-locked",
        size: [1280, 960],
        gui_scale: GUI_SCALE,
        fixture: || Fixture::ControlLocked,
    },
];

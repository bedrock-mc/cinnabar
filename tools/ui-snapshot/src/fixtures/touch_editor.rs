//! Touchscreen entry and layout editing states with anonymous controls.
use super::*;

/// Touchscreen settings and actual editor interactions at the standard capture viewport.
pub(super) const STATES: &[State] = &[
    #[cfg(feature = "test-support")]
    State {
        name: "touch-editor-narrow",
        size: [854, 480],
        gui_scale: GUI_SCALE,
        fixture: || Fixture::TouchEditor { phase: 1 },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "touch-editor-default",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::TouchEditor { phase: 0 },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "touch-editor-aligned",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::TouchEditor { phase: 1 },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "touch-editor-saved",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::TouchEditor { phase: 2 },
    },
    #[cfg(feature = "test-support")]
    State {
        name: "touch-editor-demo",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || Fixture::TouchEditor { phase: 3 },
    },
    State {
        name: "settings-touch",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || {
            let mut view = settings("touch_forced_index");
            view.touch_available = true;
            Fixture::Menu(Box::new(view))
        },
    },
];
